//! Directory reading, metadata extraction and sorting.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use crate::icons::{human_kind, icon_names_for, IconResolver};
use crate::natsort::natural_cmp;
use crate::store::RecentItem;
use crate::tags::TagStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Extension,
    Size,
    Kind,
    Modified,
}

impl SortKey {
    pub fn as_str(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Extension => "extension",
            SortKey::Size => "size",
            SortKey::Kind => "kind",
            SortKey::Modified => "modified",
        }
    }
}

impl FromStr for SortKey {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "name" => Ok(SortKey::Name),
            "extension" | "ext" => Ok(SortKey::Extension),
            "size" => Ok(SortKey::Size),
            "kind" | "type" => Ok(SortKey::Kind),
            "modified" | "date" | "mtime" => Ok(SortKey::Modified),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SortSpec {
    pub key: SortKey,
    pub ascending: bool,
    pub directories_first: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            key: SortKey::Name,
            ascending: true,
            directories_first: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_hidden: bool,
    pub size: u64,
    pub modified_ms: f64,
    pub kind: String,
    pub icon_url: String,
    pub extension: String,
    /// Device and inode identity, used for stable tag assignment.
    pub dev: u64,
    pub ino: u64,
    /// Zero-based tag index, or -1 when untagged.
    pub tag: i32,
    /// Path shown in search results; empty for entries of the current folder.
    pub display_path: String,
    pub tree_depth: usize,
    pub tree_expanded: bool,
}

impl Entry {
    pub fn size_text(&self) -> String {
        if self.is_dir {
            "—".to_owned()
        } else {
            crate::paths::format_size(self.size)
        }
    }

    pub fn modified_text(&self) -> String {
        if self.modified_ms <= 0.0 {
            "—".to_owned()
        } else {
            // The UI formats timestamps through QML's locale-aware Date. This
            // fallback is only used if QML cannot format it.
            let seconds = (self.modified_ms / 1000.0) as i64;
            seconds.to_string()
        }
    }
}

pub fn read_directory(
    path: &Path,
    spec: SortSpec,
    show_hidden: bool,
    icons: &Arc<IconResolver>,
) -> Result<Vec<Entry>, String> {
    let mut entries = read_directory_inner(path, show_hidden, Some(icons))?;
    sort_entries(&mut entries, spec);
    Ok(entries)
}

pub fn read_directory_for_search(path: &Path, show_hidden: bool) -> Result<Vec<Entry>, String> {
    read_directory_inner(path, show_hidden, None)
}

fn read_directory_inner(
    path: &Path,
    show_hidden: bool,
    icons: Option<&Arc<IconResolver>>,
) -> Result<Vec<Entry>, String> {
    let read = fs::read_dir(path).map_err(|error| friendly_io_error(path, &error))?;
    let mut entries = Vec::new();

    for item in read {
        let Ok(item) = item else {
            continue;
        };
        let name_os = item.file_name();
        let name = name_os.to_string_lossy().into_owned();
        if name == "." || name == ".." {
            continue;
        }
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let entry_path = path.join(&name);

        let symlink_meta = match fs::symlink_metadata(&entry_path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        let is_symlink = symlink_meta.file_type().is_symlink();
        let target_meta = is_symlink.then(|| fs::metadata(&entry_path).ok()).flatten();
        let meta = target_meta.as_ref().unwrap_or(&symlink_meta);
        let is_dir = meta.is_dir();
        let extension = entry_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();
        let kind = if is_symlink && target_meta.is_none() {
            "Broken link".to_owned()
        } else {
            human_kind(&name, is_dir, &extension)
        };
        let icon_url = icons
            .and_then(|icons| {
                let icon_names = if is_symlink && kind == "Broken link" {
                    vec!["emblem-broken-link", "text-x-generic"]
                } else {
                    icon_names_for(&name, is_dir, &extension)
                };
                icons.resolve_first(&icon_names)
            })
            .unwrap_or_default();
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as f64)
            .unwrap_or(0.0);
        use std::os::unix::fs::MetadataExt;

        entries.push(Entry {
            name,
            path: entry_path,
            is_dir,
            is_symlink,
            is_hidden: item.file_name().to_string_lossy().starts_with('.'),
            size: if is_dir { 0 } else { meta.len() },
            modified_ms,
            kind,
            icon_url,
            extension,
            dev: meta.dev(),
            ino: meta.ino(),
            tag: -1,
            display_path: String::new(),
            tree_depth: 0,
            tree_expanded: false,
        });
    }

    Ok(entries)
}

pub fn read_recent_files(
    items: &[RecentItem],
    spec: SortSpec,
    icons: &Arc<IconResolver>,
    tags: &TagStore,
) -> Vec<Entry> {
    use std::os::unix::fs::MetadataExt;

    let mut entries = Vec::new();
    for item in items {
        let path = PathBuf::from(&item.path);
        if is_excluded_recent_path(&path) {
            continue;
        }
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| item.path.clone());
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();
        let kind = human_kind(&name, false, &extension);
        let icon_url = icons
            .resolve_first(&icon_names_for(&name, false, &extension))
            .unwrap_or_default();
        let tag = tags.tag_for(&path, meta.dev(), meta.ino());
        entries.push(Entry {
            name,
            path: path.clone(),
            is_dir: false,
            is_symlink: false,
            is_hidden: path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.')),
            size: meta.len(),
            modified_ms: item.opened_at_ms as f64,
            kind,
            icon_url,
            extension,
            dev: meta.dev(),
            ino: meta.ino(),
            tag,
            display_path: path
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned())
                .unwrap_or_default(),
            tree_depth: 0,
            tree_expanded: false,
        });
    }

    sort_entries(&mut entries, spec);
    entries
}

pub fn read_tag_collection(
    paths: &[PathBuf],
    selected: &[usize],
    untagged: bool,
    spec: SortSpec,
    icons: &Arc<IconResolver>,
    tags: &TagStore,
) -> (Vec<Entry>, Vec<PathBuf>) {
    use std::os::unix::fs::MetadataExt;

    let mut entries = Vec::new();
    let mut missing = Vec::new();
    for path in paths {
        let symlink_meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(error) => {
                if error.kind() == std::io::ErrorKind::NotFound {
                    missing.push(path.clone());
                }
                continue;
            }
        };
        let is_symlink = symlink_meta.file_type().is_symlink();
        let target_meta = is_symlink.then(|| fs::metadata(path).ok()).flatten();
        let meta = target_meta.as_ref().unwrap_or(&symlink_meta);
        let assignments = tags.tags_for(path, meta.dev(), meta.ino());
        let matches = if untagged {
            assignments.is_empty()
        } else if selected.is_empty() {
            !assignments.is_empty()
        } else {
            selected
                .iter()
                .any(|tag| assignments.contains(&(*tag as i32)))
        };
        if !matches {
            continue;
        }

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let is_dir = meta.is_dir();
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();
        let kind = if is_symlink && target_meta.is_none() {
            "Broken link".to_owned()
        } else {
            human_kind(&name, is_dir, &extension)
        };
        let icon_names = if is_symlink && target_meta.is_none() {
            vec!["emblem-broken-link", "text-x-generic"]
        } else {
            icon_names_for(&name, is_dir, &extension)
        };
        let icon_url = icons.resolve_first(&icon_names).unwrap_or_default();
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as f64)
            .unwrap_or(0.0);
        entries.push(Entry {
            name,
            path: path.clone(),
            is_dir,
            is_symlink,
            is_hidden: path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.')),
            size: if is_dir { 0 } else { meta.len() },
            modified_ms,
            kind,
            icon_url,
            extension,
            dev: meta.dev(),
            ino: meta.ino(),
            tag: assignments.first().copied().unwrap_or(-1),
            display_path: path
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned())
                .unwrap_or_default(),
            tree_depth: 0,
            tree_expanded: false,
        });
    }

    sort_entries(&mut entries, spec);
    (entries, missing)
}

fn is_excluded_recent_path(path: &Path) -> bool {
    let excluded_dir = path.components().any(|component| {
        let component = component.as_os_str().to_string_lossy();
        matches!(component.as_ref(), ".cache" | "cache" | "tmp" | "temp")
    });
    if excluded_dir {
        return true;
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    name.ends_with('~')
        || name.starts_with(".#")
        || name.starts_with("~$")
        || name.ends_with(".swp")
        || name.ends_with(".tmp")
}

pub fn sort_entries(entries: &mut [Entry], spec: SortSpec) {
    entries.sort_by(|a, b| compare_entries(a, b, spec));
}

pub fn compare_entries(a: &Entry, b: &Entry, spec: SortSpec) -> Ordering {
    if spec.directories_first {
        let directory_order = b.is_dir.cmp(&a.is_dir);
        if directory_order != Ordering::Equal {
            return directory_order;
        }
    }

    let key_order = match spec.key {
        SortKey::Name => natural_cmp(&a.name, &b.name),
        SortKey::Extension => {
            natural_cmp(&a.extension, &b.extension).then_with(|| natural_cmp(&a.name, &b.name))
        }
        SortKey::Size => a
            .size
            .cmp(&b.size)
            .then_with(|| natural_cmp(&a.name, &b.name)),
        SortKey::Kind => natural_cmp(&a.kind, &b.kind).then_with(|| natural_cmp(&a.name, &b.name)),
        SortKey::Modified => a
            .modified_ms
            .partial_cmp(&b.modified_ms)
            .unwrap_or(Ordering::Equal)
            .then_with(|| natural_cmp(&a.name, &b.name)),
    };

    if spec.ascending {
        key_order
    } else {
        key_order.reverse()
    }
}

pub fn friendly_io_error(path: &Path, error: &std::io::Error) -> String {
    let display = path.to_string_lossy();
    match error.kind() {
        std::io::ErrorKind::NotFound => format!("{display} no longer exists."),
        std::io::ErrorKind::PermissionDenied => {
            format!("Permission denied while reading {display}.")
        }
        std::io::ErrorKind::NotADirectory => format!("{display} is not a directory."),
        _ => format!("Could not read {display}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        name: &str,
        is_dir: bool,
        size: u64,
        modified_ms: f64,
        kind: &str,
        ext: &str,
    ) -> Entry {
        Entry {
            name: name.to_owned(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink: false,
            is_hidden: name.starts_with('.'),
            size,
            modified_ms,
            kind: kind.to_owned(),
            icon_url: String::new(),
            extension: ext.to_owned(),
            dev: 0,
            ino: 0,
            tag: -1,
            display_path: String::new(),
            tree_depth: 0,
            tree_expanded: false,
        }
    }

    #[test]
    fn natural_name_sort_puts_directories_first() {
        let mut entries = vec![
            entry("file10", false, 0, 0.0, "File", ""),
            entry("file2", false, 0, 0.0, "File", ""),
            entry("zdir", true, 0, 0.0, "Folder", ""),
            entry("adir", true, 0, 0.0, "Folder", ""),
        ];
        sort_entries(&mut entries, SortSpec::default());
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["adir", "zdir", "file2", "file10"]);
    }

    #[test]
    fn descending_keeps_directories_first() {
        let mut entries = vec![
            entry("a", false, 1, 0.0, "File", ""),
            entry("b", false, 2, 0.0, "File", ""),
            entry("d", true, 0, 0.0, "Folder", ""),
        ];
        sort_entries(
            &mut entries,
            SortSpec {
                key: SortKey::Name,
                ascending: false,
                directories_first: true,
            },
        );
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["d", "b", "a"]);
    }

    #[test]
    fn can_sort_by_size_and_extension() {
        let mut entries = vec![
            entry("big.txt", false, 100, 0.0, "Text file", "txt"),
            entry("small.rs", false, 1, 0.0, "Rust source", "rs"),
            entry("medium.txt", false, 50, 0.0, "Text file", "txt"),
        ];
        sort_entries(
            &mut entries,
            SortSpec {
                key: SortKey::Size,
                ascending: true,
                directories_first: true,
            },
        );
        assert_eq!(entries[0].name, "small.rs");
        assert_eq!(entries[2].name, "big.txt");

        sort_entries(
            &mut entries,
            SortSpec {
                key: SortKey::Extension,
                ascending: true,
                directories_first: true,
            },
        );
        assert_eq!(entries[0].extension, "rs");
        assert_eq!(entries[1].extension, "txt");
    }

    #[test]
    fn hidden_flag_is_detected() {
        let e = entry(".secret", false, 0, 0.0, "File", "");
        assert!(e.is_hidden);
    }
}
