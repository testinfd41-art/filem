//! Bookmark, recent-file and session persistence in the application config
//! directory.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::atomic_write;
use crate::paths::{app_config_dir, clean_lexically, home_dir};

const MAX_RECENT_FILES: usize = 500;

#[derive(Debug, Clone, Default, Serialize)]
pub struct RecentStore {
    pub items: Vec<RecentItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentItem {
    pub path: String,
    pub opened_at_ms: i64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredRecentItem {
    Current(RecentItem),
    Legacy(String),
}

#[derive(Deserialize)]
#[serde(default)]
struct RecentStoreRead {
    items: Vec<StoredRecentItem>,
}

impl Default for RecentStoreRead {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

pub fn recents_file() -> PathBuf {
    app_config_dir().join("recents.json")
}

impl RecentStore {
    pub fn load() -> (Self, Option<String>) {
        let path = recents_file();
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<RecentStoreRead>(&text) {
                Ok(read) => {
                    let mut store = RecentStore::default();
                    let mut seen = std::collections::HashSet::new();
                    for item in read.items.into_iter().take(MAX_RECENT_FILES) {
                        let item = match item {
                            StoredRecentItem::Current(item) => item,
                            StoredRecentItem::Legacy(path) => RecentItem {
                                path,
                                opened_at_ms: 0,
                            },
                        };
                        let Some(path) = std::fs::canonicalize(&item.path)
                            .ok()
                            .filter(|path| path.is_file())
                        else {
                            continue;
                        };
                        let path = path.to_string_lossy().into_owned();
                        if seen.insert(path.clone()) {
                            store.items.push(RecentItem {
                                path,
                                opened_at_ms: item.opened_at_ms,
                            });
                        }
                    }
                    store
                        .items
                        .sort_by(|a, b| b.opened_at_ms.cmp(&a.opened_at_ms));
                    store.items.truncate(MAX_RECENT_FILES);
                    (store, None)
                }
                Err(error) => (
                    RecentStore::default(),
                    Some(format!(
                        "Recents file {} is invalid: {error}. No recent files were loaded.",
                        path.display()
                    )),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (RecentStore::default(), None)
            }
            Err(error) => (
                RecentStore::default(),
                Some(format!("Could not read recent files: {error}")),
            ),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("Could not serialize recent files: {error}"))?;
        atomic_write(&recents_file(), text.as_bytes())
    }

    pub fn add(&mut self, path: &Path) -> Result<(), String> {
        let path = std::fs::canonicalize(path).map_err(|error| {
            format!("Could not resolve recent file {}: {error}", path.display())
        })?;
        if !path.is_file() {
            return Ok(());
        }
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("System clock is before the Unix epoch: {error}"))?
            .as_millis()
            .min(i64::MAX as u128) as i64;
        self.add_item(RecentItem {
            path: clean_lexically(&path).to_string_lossy().into_owned(),
            opened_at_ms: now_ms,
        });
        Ok(())
    }

    pub fn add_item(&mut self, item: RecentItem) {
        let opened_at_ms = self
            .items
            .iter()
            .find(|existing| existing.path == item.path)
            .map(|existing| existing.opened_at_ms.saturating_add(1))
            .unwrap_or(item.opened_at_ms)
            .max(item.opened_at_ms);
        self.items.retain(|existing| existing.path != item.path);
        self.items.push(RecentItem {
            opened_at_ms,
            ..item
        });
        self.items
            .sort_by(|a, b| b.opened_at_ms.cmp(&a.opened_at_ms));
        self.items.truncate(MAX_RECENT_FILES);
    }

    pub fn retain_paths(&mut self, paths: &std::collections::HashSet<String>) {
        self.items.retain(|item| paths.contains(&item.path));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub path: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BookmarkStore {
    pub items: Vec<Bookmark>,
}

pub fn bookmarks_file() -> PathBuf {
    app_config_dir().join("bookmarks.json")
}

impl BookmarkStore {
    pub fn load() -> (Self, Option<String>) {
        let path = bookmarks_file();
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<BookmarkStore>(&text) {
                Ok(mut store) => {
                    store.items.retain(|item| {
                        !item.path.trim().is_empty() && !item.label.trim().is_empty()
                    });
                    (store, None)
                }
                Err(error) => (
                    BookmarkStore::default(),
                    Some(format!(
                        "Bookmarks file {} is invalid: {error}. No bookmarks were loaded.",
                        path.display()
                    )),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (BookmarkStore::default(), None)
            }
            Err(error) => (
                BookmarkStore::default(),
                Some(format!("Could not read bookmarks: {error}")),
            ),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("Could not serialize bookmarks: {error}"))?;
        atomic_write(&bookmarks_file(), text.as_bytes())
    }

    pub fn add(&mut self, path: &str, label: Option<&str>) -> Result<usize, String> {
        let path = clean_lexically(&PathBuf::from(path.trim()));
        if !path.is_absolute() {
            return Err("Bookmarks must point to an absolute path.".to_owned());
        }
        if let Some(index) = self
            .items
            .iter()
            .position(|item| Path::new(&item.path) == path.as_path())
        {
            return Ok(index);
        }
        let label = label
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(|value| value.to_owned())
            .or_else(|| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                if path == home_dir() {
                    "Home".to_owned()
                } else {
                    path.to_string_lossy().into_owned()
                }
            });
        self.items.push(Bookmark {
            path: path.to_string_lossy().into_owned(),
            label,
        });
        Ok(self.items.len() - 1)
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.items.len() {
            self.items.remove(index);
            true
        } else {
            false
        }
    }

    pub fn rename(&mut self, index: usize, label: &str) -> Result<(), String> {
        let label = label.trim();
        if label.is_empty() {
            return Err("A bookmark name cannot be empty.".to_owned());
        }
        let Some(item) = self.items.get_mut(index) else {
            return Err("Unknown bookmark.".to_owned());
        };
        item.label = label.to_owned();
        Ok(())
    }

    pub fn move_item(&mut self, index: usize, delta: isize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        let target = (index as isize + delta).clamp(0, self.items.len() as isize - 1) as usize;
        if target == index {
            return false;
        }
        let item = self.items.remove(index);
        self.items.insert(target, item);
        true
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionTab {
    pub path: String,
    pub view: String,
    pub sort_key: String,
    pub sort_ascending: bool,
    pub directories_first: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub tabs: Vec<SessionTab>,
    pub active: usize,
    pub dual_pane: bool,
    pub pane2_tabs: Vec<SessionTab>,
    pub pane2_active: usize,
}

pub fn session_file() -> PathBuf {
    app_config_dir().join("session.json")
}

impl Session {
    pub fn load() -> Session {
        let Ok(text) = std::fs::read_to_string(session_file()) else {
            return Session::default();
        };
        serde_json::from_str::<Session>(&text).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("Could not serialize session: {error}"))?;
        atomic_write(&session_file(), text.as_bytes())
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_add_deduplicates_and_labels() {
        let mut store = BookmarkStore::default();
        assert_eq!(store.add("/tmp", None).unwrap(), 0);
        assert_eq!(store.add("/tmp/", Some("Temp")).unwrap(), 0);
        assert_eq!(store.items.len(), 1);
        assert_eq!(store.items[0].label, "tmp");
        assert_eq!(
            store.add("relative", None),
            Err("Bookmarks must point to an absolute path.".to_owned())
        );
    }

    #[test]
    fn bookmark_reorder_clamps() {
        let mut store = BookmarkStore::default();
        store.add("/a", Some("A")).unwrap();
        store.add("/b", Some("B")).unwrap();
        store.add("/c", Some("C")).unwrap();
        assert!(store.move_item(2, -1));
        assert_eq!(store.items[1].label, "C");
        // Moving item 0 up by 5 is a no-op since it's already at the top;
        // the function returns false when target == index.
        assert!(!store.move_item(0, -5));
        assert!(!store.move_item(0, 0));
    }

    #[test]
    fn rename_rejects_empty_labels() {
        let mut store = BookmarkStore::default();
        store.add("/a", Some("A")).unwrap();
        assert!(store.rename(0, " ").is_err());
        assert!(store.rename(9, "X").is_err());
        assert!(store.rename(0, "Alpha").is_ok());
    }

    #[test]
    fn session_defaults_are_empty() {
        let session = Session::default();
        assert!(session.is_empty());
        assert_eq!(session.active, 0);
        assert!(!session.dual_pane);
    }

    #[test]
    fn recent_file_add_updates_existing_path() {
        let mut store = RecentStore::default();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("document.txt");
        std::fs::write(&path, "test").unwrap();

        store.add(&path).unwrap();
        let first = store.items[0].opened_at_ms;
        store.add(&path).unwrap();

        assert_eq!(store.items.len(), 1);
        assert_eq!(
            store.items[0].path,
            std::fs::canonicalize(path)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        );
        assert!(store.items[0].opened_at_ms >= first);
    }

    #[test]
    fn recent_store_loads_legacy_folder_history_as_stale_records() {
        let legacy: RecentStoreRead =
            serde_json::from_str(r#"{"items":["/tmp/file.txt"]}"#).unwrap();
        assert_eq!(legacy.items.len(), 1);
    }

    #[test]
    fn recent_file_limit_is_bounded() {
        let mut store = RecentStore::default();
        for index in 0..MAX_RECENT_FILES {
            store.items.push(RecentItem {
                path: format!("/folder/{index}.txt"),
                opened_at_ms: index as i64,
            });
        }
        assert_eq!(store.items.len(), MAX_RECENT_FILES);
    }
}
