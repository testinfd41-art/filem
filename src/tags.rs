//! Persistent seven-colour tag metadata.
//!
//! Tags are stored in `$XDG_STATE_HOME/filem/tags.json`. An assignment is keyed
//! by device and inode so that renaming or moving a file inside the application
//! keeps its tag. A path key is stored as a fallback for filesystems where the
//! inode is not stable across remounts (for example some FUSE mounts).
//!
//! Limitations, by design: a file copied outside Filem does not inherit a tag,
//! and tags on removable media are tied to the inode of that medium. Tags never
//! modify the file itself.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::atomic_write;
use crate::paths::state_dir;

pub const TAG_COUNT: usize = 7;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagDef {
    pub name: String,
    /// Optional user override. When absent the palette's tag colour is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

fn default_tags() -> Vec<TagDef> {
    ["Red", "Orange", "Yellow", "Green", "Blue", "Purple", "Gray"]
        .into_iter()
        .map(|name| TagDef {
            name: name.to_owned(),
            color: None,
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TagStore {
    pub tags: Vec<TagDef>,
    /// `device:inode` -> zero-based tag index.
    pub by_inode: BTreeMap<String, u8>,
    /// Absolute path -> zero-based tag index (fallback).
    pub by_path: BTreeMap<String, u8>,
    /// Additional tag assignments; the original maps remain the primary tag
    /// for backward compatibility with existing metadata files.
    #[serde(default)]
    pub extra_by_inode: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    pub extra_by_path: BTreeMap<String, Vec<u8>>,
    /// Paths encountered while browsing, used to provide a bounded Untagged view.
    #[serde(default)]
    pub known_paths: BTreeMap<String, (u64, u64)>,
    #[serde(default)]
    pub known_dirs: BTreeSet<String>,
}

impl Default for TagStore {
    fn default() -> Self {
        Self {
            tags: default_tags(),
            by_inode: BTreeMap::new(),
            by_path: BTreeMap::new(),
            extra_by_inode: BTreeMap::new(),
            extra_by_path: BTreeMap::new(),
            known_paths: BTreeMap::new(),
            known_dirs: BTreeSet::new(),
        }
    }
}

pub fn tags_file() -> std::path::PathBuf {
    state_dir().join("filem/tags.json")
}

fn inode_key(dev: u64, ino: u64) -> Option<String> {
    // Inode zero is not a usable identity on most filesystems.
    (ino != 0).then(|| format!("{dev}:{ino}"))
}

impl TagStore {
    pub fn load() -> (Self, Option<String>) {
        Self::load_from(&tags_file())
    }

    fn load_from(path: &Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<TagStore>(&text) {
                Ok(mut store) => {
                    store.normalize();
                    (store, None)
                }
                Err(error) => (
                    TagStore::default(),
                    Some(format!(
                        "Tag metadata {} is invalid: {error}. Tags were not loaded.",
                        path.display()
                    )),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (TagStore::default(), None)
            }
            Err(error) => (
                TagStore::default(),
                Some(format!("Could not read tags {}: {error}", path.display())),
            ),
        }
    }

    fn normalize(&mut self) {
        if self.tags.len() < TAG_COUNT {
            self.tags = default_tags();
        }
        self.tags.truncate(TAG_COUNT);
        for tag in &mut self.tags {
            if tag.name.trim().is_empty() {
                tag.name = "Tag".to_owned();
            }
        }
        self.by_inode
            .retain(|_, index| (*index as usize) < TAG_COUNT);
        self.by_path
            .retain(|_, index| (*index as usize) < TAG_COUNT);
        for assignments in self
            .extra_by_inode
            .values_mut()
            .chain(self.extra_by_path.values_mut())
        {
            assignments.retain(|index| (*index as usize) < TAG_COUNT);
            assignments.sort_unstable();
            assignments.dedup();
        }
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&tags_file())
    }

    fn save_to(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("Could not serialize tags: {error}"))?;
        atomic_write(path, text.as_bytes())
    }

    pub fn name(&self, index: usize) -> &str {
        self.tags
            .get(index)
            .map(|tag| tag.name.as_str())
            .unwrap_or("Tag")
    }

    pub fn color(&self, index: usize) -> Option<&str> {
        self.tags
            .get(index)
            .and_then(|tag| tag.color.as_deref())
            .filter(|color| crate::theme::is_valid_color(color))
    }

    pub fn tag_for(&self, path: &Path, dev: u64, ino: u64) -> i32 {
        self.tags_for(path, dev, ino).first().copied().unwrap_or(-1)
    }

    pub fn tags_for(&self, path: &Path, dev: u64, ino: u64) -> Vec<i32> {
        let mut assignments = Vec::new();
        if let Some(key) = inode_key(dev, ino) {
            if let Some(index) = self.by_inode.get(&key) {
                assignments.push(*index);
            }
            if let Some(extra) = self.extra_by_inode.get(&key) {
                assignments.extend(extra.iter().copied());
            }
        }
        let path_key = path.to_string_lossy().into_owned();
        if let Some(index) = self.by_path.get(&path_key) {
            assignments.push(*index);
        }
        if let Some(extra) = self.extra_by_path.get(&path_key) {
            assignments.extend(extra.iter().copied());
        }
        assignments.sort_unstable();
        assignments.dedup();
        assignments.into_iter().map(i32::from).collect()
    }

    /// Add a tag (0..=6), or clear all assignments (`None`).
    pub fn set_tag(&mut self, path: &Path, dev: u64, ino: u64, tag: Option<usize>) -> bool {
        let Some(tag) = tag.filter(|tag| *tag < TAG_COUNT) else {
            self.clear_tags(path, dev, ino);
            return true;
        };
        let path_key = path.to_string_lossy().into_owned();
        let index = tag as u8;
        if !self.tags_for(path, dev, ino).contains(&(tag as i32)) {
            if let Some(key) = inode_key(dev, ino) {
                if !self.by_inode.contains_key(&key) {
                    self.by_inode.insert(key.clone(), index);
                } else {
                    let extra = self.extra_by_inode.entry(key).or_default();
                    extra.push(index);
                    extra.sort_unstable();
                    extra.dedup();
                }
            }
            if !self.by_path.contains_key(&path_key) {
                self.by_path.insert(path_key.clone(), index);
            } else {
                let extra = self.extra_by_path.entry(path_key.clone()).or_default();
                extra.push(index);
                extra.sort_unstable();
                extra.dedup();
            }
        }
        self.known_paths.insert(path_key, (dev, ino));
        true
    }

    pub fn forget(&mut self, path: &Path, dev: u64, ino: u64) {
        if let Some(key) = inode_key(dev, ino) {
            self.by_inode.remove(&key);
            self.extra_by_inode.remove(&key);
        }
        let path = path.to_string_lossy().into_owned();
        self.by_path.remove(&path);
        self.extra_by_path.remove(&path);
        self.known_paths.remove(&path);
    }

    pub fn forget_missing(&mut self, path: &Path) -> bool {
        let path_key = path.to_string_lossy().into_owned();
        let identity = self.known_paths.remove(&path_key);
        let mut changed = self.by_path.remove(&path_key).is_some();
        changed |= self.extra_by_path.remove(&path_key).is_some();
        changed |= identity.is_some();
        changed |= self.known_dirs.remove(&path_key);
        if let Some((dev, ino)) = identity {
            let has_known_alias = self
                .known_paths
                .values()
                .any(|candidate| *candidate == (dev, ino));
            if !has_known_alias {
                if let Some(key) = inode_key(dev, ino) {
                    self.by_inode.remove(&key);
                    self.extra_by_inode.remove(&key);
                }
            }
        }
        changed
    }

    pub fn clear_tags(&mut self, path: &Path, dev: u64, ino: u64) {
        if let Some(key) = inode_key(dev, ino) {
            self.by_inode.remove(&key);
            self.extra_by_inode.remove(&key);
        }
        let path = path.to_string_lossy().into_owned();
        self.by_path.remove(&path);
        self.extra_by_path.remove(&path);
    }

    pub fn toggle_tag(&mut self, path: &Path, dev: u64, ino: u64, tag: usize) {
        if tag >= TAG_COUNT {
            return;
        }
        let path_key = path.to_string_lossy().into_owned();
        let mut assignments = self.tags_for(path, dev, ino);
        if assignments.contains(&(tag as i32)) {
            assignments.retain(|index| *index != tag as i32);
        } else {
            assignments.push(tag as i32);
        }
        assignments.sort_unstable();
        let path_values = assignments
            .iter()
            .map(|index| *index as u8)
            .collect::<Vec<_>>();
        self.by_path.remove(&path_key);
        self.extra_by_path.remove(&path_key);
        if let Some(primary) = path_values.first() {
            self.by_path.insert(path_key.clone(), *primary);
            if path_values.len() > 1 {
                self.extra_by_path
                    .insert(path_key.clone(), path_values[1..].to_vec());
            }
        }
        if let Some(key) = inode_key(dev, ino) {
            self.by_inode.remove(&key);
            self.extra_by_inode.remove(&key);
            if let Some(primary) = path_values.first() {
                self.by_inode.insert(key.clone(), *primary);
                if path_values.len() > 1 {
                    self.extra_by_inode.insert(key, path_values[1..].to_vec());
                }
            }
        }
        self.known_paths.insert(path_key, (dev, ino));
    }

    pub fn record_seen(&mut self, path: &Path, dev: u64, ino: u64) {
        self.known_paths
            .insert(path.to_string_lossy().into_owned(), (dev, ino));
    }

    pub fn record_directory(&mut self, path: &Path) {
        self.known_dirs.insert(path.to_string_lossy().into_owned());
    }

    pub fn count(&self, tag: Option<usize>) -> usize {
        let (tagged, untagged) = self.collection_counts();
        match tag {
            Some(tag) => tagged.get(tag).copied().unwrap_or_default(),
            None => untagged,
        }
    }

    pub fn collection_counts(&self) -> ([usize; TAG_COUNT], usize) {
        let mut tagged = [0; TAG_COUNT];
        let mut untagged = 0;
        let mut paths = std::collections::BTreeSet::new();
        paths.extend(self.known_paths.keys());
        paths.extend(self.by_path.keys());
        paths.extend(self.extra_by_path.keys());
        for path in paths {
            let identity = self.known_paths.get(path).copied().unwrap_or_default();
            let assignments = self.tags_for(Path::new(path), identity.0, identity.1);
            let is_untagged = assignments.is_empty();
            for tag in assignments {
                if let Some(count) = tagged.get_mut(tag as usize) {
                    *count += 1;
                }
            }
            if self.known_paths.contains_key(path) && is_untagged {
                untagged += 1;
            }
        }
        (tagged, untagged)
    }

    pub fn collection_paths(&self, selected: &[usize], untagged: bool) -> Vec<PathBuf> {
        let mut paths = std::collections::BTreeSet::new();
        for (path, identity) in &self.known_paths {
            let assigned = self.tags_for(Path::new(path), identity.0, identity.1);
            let matches = if untagged {
                assigned.is_empty()
            } else if selected.is_empty() {
                !assigned.is_empty()
            } else {
                selected.iter().any(|tag| assigned.contains(&(*tag as i32)))
            };
            if matches {
                paths.insert(PathBuf::from(path));
            }
        }
        for path in self.by_path.keys().chain(self.extra_by_path.keys()) {
            let identity = self.known_paths.get(path).copied().unwrap_or_default();
            let assigned = self.tags_for(Path::new(path), identity.0, identity.1);
            if !untagged
                && ((selected.is_empty() && !assigned.is_empty())
                    || selected.iter().any(|tag| assigned.contains(&(*tag as i32))))
            {
                paths.insert(PathBuf::from(path));
            }
        }
        paths.into_iter().collect()
    }

    /// Keep the path fallback accurate after an in-application rename or move.
    pub fn note_move(&mut self, from: &Path, to: &Path, dev: u64, ino: u64) {
        let old = from.to_string_lossy().into_owned();
        let new = to.to_string_lossy().into_owned();
        if let Some(index) = self.by_path.remove(&old) {
            self.by_path.insert(new.clone(), index);
        }
        if let Some(extra) = self.extra_by_path.remove(&old) {
            self.extra_by_path.insert(new.clone(), extra);
        }
        if let Some(identity) = self.known_paths.remove(&old) {
            self.known_paths.insert(new.clone(), identity);
        }
        if self.known_dirs.remove(&old) {
            self.known_dirs.insert(new.clone());
        }
        let tags = self.tags_for(Path::new(&new), dev, ino);
        if !tags.is_empty() {
            if let Some(key) = inode_key(dev, ino) {
                self.by_inode.insert(key.clone(), tags[0] as u8);
                if tags.len() > 1 {
                    self.extra_by_inode
                        .insert(key, tags[1..].iter().map(|index| *index as u8).collect());
                }
            }
        }
        self.known_paths.insert(new, (dev, ino));
    }

    pub fn rename(&mut self, index: usize, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A tag name cannot be empty.".to_owned());
        }
        if name.chars().count() > 24 {
            return Err("Tag names are limited to 24 characters.".to_owned());
        }
        let Some(tag) = self.tags.get_mut(index) else {
            return Err("Unknown tag.".to_owned());
        };
        tag.name = name.to_owned();
        Ok(())
    }

    pub fn set_color(&mut self, index: usize, color: Option<&str>) -> Result<(), String> {
        let Some(tag) = self.tags.get_mut(index) else {
            return Err("Unknown tag.".to_owned());
        };
        match color {
            Some(value) if crate::theme::is_valid_color(value) => {
                tag.color = Some(value.to_owned());
            }
            Some(_) => return Err("Tag colours must be #rrggbb.".to_owned()),
            None => tag.color = None,
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn defaults_have_seven_named_tags() {
        let store = TagStore::default();
        assert_eq!(store.tags.len(), 7);
        assert_eq!(store.name(0), "Red");
        assert_eq!(store.name(6), "Gray");
    }

    #[test]
    fn assignment_survives_rename_via_inode() {
        let mut store = TagStore::default();
        let path = PathBuf::from("/tmp/a.txt");
        store.set_tag(&path, 10, 42, Some(4));
        assert_eq!(store.tag_for(&path, 10, 42), 4);
        store.note_move(&path, Path::new("/tmp/b.txt"), 10, 42);
        assert_eq!(store.tag_for(Path::new("/tmp/b.txt"), 10, 42), 4);
        assert_eq!(store.by_path.get("/tmp/b.txt"), Some(&4));
    }

    #[test]
    fn removing_a_tag_clears_both_keys() {
        let mut store = TagStore::default();
        let path = PathBuf::from("/tmp/a.txt");
        store.set_tag(&path, 10, 42, Some(2));
        store.set_tag(&path, 10, 42, None);
        assert_eq!(store.tag_for(&path, 10, 42), -1);
        assert!(store.by_path.is_empty());
    }

    #[test]
    fn invalid_index_is_ignored() {
        let mut store = TagStore::default();
        store.set_tag(Path::new("/tmp/x"), 1, 1, Some(99));
        assert_eq!(store.tag_for(Path::new("/tmp/x"), 1, 1), -1);
    }

    #[test]
    fn renames_and_colours_are_validated() {
        let mut store = TagStore::default();
        assert!(store.rename(0, "  ").is_err());
        assert!(store.set_color(0, Some("red")).is_err());
        assert!(store.rename(0, "Urgent").is_ok());
        assert!(store.set_color(0, Some("#ff8800")).is_ok());
        assert_eq!(store.color(0), Some("#ff8800"));
    }

    #[test]
    fn tag_assignments_and_custom_labels_persist_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tags.json");
        let path = PathBuf::from("/mnt/data/report.pdf");
        let mut store = TagStore::default();
        store.rename(4, "Research").unwrap();
        store.set_tag(&path, 12, 34, Some(4));
        store.save_to(&file).unwrap();

        let (loaded, warning) = TagStore::load_from(&file);
        assert!(warning.is_none());
        assert_eq!(loaded.name(4), "Research");
        assert_eq!(loaded.tag_for(&path, 12, 34), 4);
    }

    #[test]
    fn legacy_single_tag_metadata_loads_without_losing_assignments() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tags.json");
        std::fs::write(
            &file,
            r#"{"tags":[{"name":"Red"},{"name":"Orange"},{"name":"Yellow"},{"name":"Green"},{"name":"Blue"},{"name":"Purple"},{"name":"Gray"}],"by_inode":{"12:34":4},"by_path":{"/mnt/data/report.pdf":4}}"#,
        )
        .unwrap();

        let (loaded, warning) = TagStore::load_from(&file);
        assert!(warning.is_none());
        assert_eq!(
            loaded.tags_for(Path::new("/mnt/data/report.pdf"), 12, 34),
            [4]
        );
    }

    #[test]
    fn multiple_tags_toggle_and_visited_collections_are_consistent() {
        let mut store = TagStore::default();
        let tagged = PathBuf::from("/mnt/data/report.pdf");
        let untagged = PathBuf::from("/mnt/data/notes.txt");
        store.set_tag(&tagged, 12, 34, Some(0));
        store.set_tag(&tagged, 12, 34, Some(4));
        store.record_seen(&tagged, 12, 34);
        store.record_seen(&untagged, 12, 35);

        assert_eq!(store.tags_for(&tagged, 12, 34), [0, 4]);
        assert_eq!(store.count(Some(4)), 1);
        assert_eq!(store.count(None), 1);
        assert_eq!(store.collection_paths(&[0, 4], false), [tagged.clone()]);
        assert_eq!(store.collection_paths(&[], false), [tagged.clone()]);
        assert_eq!(store.collection_paths(&[], true), [untagged.clone()]);

        store.toggle_tag(&tagged, 12, 34, 0);
        assert_eq!(store.tags_for(&tagged, 12, 34), [4]);
    }

    #[test]
    fn missing_path_cleanup_keeps_other_inode_aliases() {
        let mut store = TagStore::default();
        let first = PathBuf::from("/mnt/data/a");
        let second = PathBuf::from("/mnt/data/b");
        store.set_tag(&first, 12, 34, Some(2));
        store.record_seen(&second, 12, 34);

        assert!(store.forget_missing(&first));
        assert_eq!(store.tags_for(&second, 12, 34), [2]);
        assert!(store.by_inode.contains_key("12:34"));
    }

    #[test]
    fn multiple_tags_persist_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tags.json");
        let path = PathBuf::from("/mnt/data/report.pdf");
        let mut store = TagStore::default();
        store.set_tag(&path, 12, 34, Some(1));
        store.set_tag(&path, 12, 34, Some(6));
        store.save_to(&file).unwrap();

        let (loaded, warning) = TagStore::load_from(&file);
        assert!(warning.is_none());
        assert_eq!(loaded.tags_for(&path, 12, 34), [1, 6]);
    }
}
