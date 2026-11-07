//! Persistent app configuration. Kept intentionally small and human-editable.

use std::fs;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::fs_entry::{SortKey, SortSpec};
use crate::paths::config_file;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub show_hidden: bool,
    pub sort_key: String,
    pub sort_ascending: bool,
    pub directories_first: bool,
    pub single_click_open: bool,
    pub sidebar_visible: bool,
    pub sidebar_width: i32,
    pub tab_bar_visible: bool,
    pub show_size: bool,
    pub show_modified: bool,
    pub show_kind: bool,
    pub compact: bool,
    pub view_mode: String,
    pub restore_session: bool,
    pub dual_pane: bool,
    pub terminal: String,
    pub grid_icon_size: i32,
    pub show_tags: bool,
    pub preview_visible: bool,
    /// Search descends into subfolders by default; the search bar can turn it off.
    pub search_recursive: bool,
    pub drag_hover_delay_ms: u32,
    pub window_width: i32,
    pub window_height: i32,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_hidden: false,
            sort_key: "name".to_owned(),
            sort_ascending: true,
            directories_first: true,
            single_click_open: false,
            sidebar_visible: true,
            sidebar_width: 220,
            tab_bar_visible: true,
            show_size: true,
            show_modified: true,
            show_kind: true,
            compact: false,
            view_mode: "list".to_owned(),
            restore_session: true,
            dual_pane: false,
            terminal: "foot".to_owned(),
            grid_icon_size: 96,
            show_tags: true,
            preview_visible: true,
            search_recursive: true,
            drag_hover_delay_ms: 700,
            window_width: 1120,
            window_height: 760,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub ui: UiConfig,
    pub performance: PerformanceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    pub keep_running_background: bool,
    /// Zero means keep running until logout.
    pub background_idle_timeout_seconds: u32,
    pub startup_behavior: String,
    pub thumbnail_cache_limit_mb: u32,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            keep_running_background: true,
            background_idle_timeout_seconds: 30,
            startup_behavior: "balanced".to_owned(),
            thumbnail_cache_limit_mb: 512,
        }
    }
}

impl Config {
    pub fn load() -> (Self, Option<String>) {
        let path = config_file();
        match fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(config) => (config, None),
                Err(error) => (
                    Config::default(),
                    Some(format!(
                        "Config file {} is invalid: {error}",
                        path.display()
                    )),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Config::default(), None),
            Err(error) => (
                Config::default(),
                Some(format!("Could not read {}: {error}", path.display())),
            ),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        static SAVE_LOCK: Mutex<()> = Mutex::new(());
        let _guard = SAVE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = config_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|error| format!("Could not serialize config: {error}"))?;
        atomic_write(&path, text.as_bytes())
    }

    pub fn sort_spec(&self) -> SortSpec {
        SortSpec {
            key: self.ui.sort_key.parse::<SortKey>().unwrap_or(SortKey::Name),
            ascending: self.ui.sort_ascending,
            directories_first: self.ui.directories_first,
        }
    }

    pub fn set_sort(&mut self, key: SortKey, ascending: bool) {
        self.ui.sort_key = key.as_str().to_owned();
        self.ui.sort_ascending = ascending;
    }

    pub fn set_view_mode(&mut self, mode: &str) {
        if matches!(mode, "list" | "grid" | "column") {
            self.ui.view_mode = mode.to_owned();
        }
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Invalid path: {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("Invalid path: {}", path.display()))?
        .to_string_lossy();
    let temp = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    fs::write(&temp, bytes)
        .map_err(|error| format!("Could not write {}: {error}", temp.display()))?;
    match fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(format!("Could not replace {}: {error}", path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_are_sane() {
        let config = Config::default();
        assert_eq!(config.sort_spec().key, SortKey::Name);
        assert!(config.sort_spec().ascending);
        assert!(config.ui.directories_first);
        assert!(!config.ui.show_hidden);
        assert!(config.ui.preview_visible);
        assert_eq!(config.ui.drag_hover_delay_ms, 700);
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let config: Config = toml::from_str(
            r#"
            [ui]
            show_hidden = true
            sort_key = "size"
            sort_ascending = false
            "#,
        )
        .expect("valid config");
        assert!(config.ui.show_hidden);
        assert_eq!(config.sort_spec().key, SortKey::Size);
        assert!(!config.sort_spec().ascending);
        assert!(config.ui.show_modified);
        assert!(config.ui.preview_visible);
        assert_eq!(config.ui.drag_hover_delay_ms, 700);
    }

    #[test]
    fn drag_hover_delay_is_persisted() {
        let config: Config = toml::from_str("[ui]\ndrag_hover_delay_ms = 1200\n").unwrap();
        let saved = toml::to_string(&config).unwrap();
        let restored: Config = toml::from_str(&saved).unwrap();
        assert_eq!(restored.ui.drag_hover_delay_ms, 1200);
    }

    #[test]
    fn invalid_sort_key_falls_back_to_name() {
        let config: Config = toml::from_str("[ui]\nsort_key = \"nonsense\"\n").unwrap();
        assert_eq!(config.sort_spec().key, SortKey::Name);
    }

    #[test]
    fn performance_settings_default_and_persist() {
        let defaults = Config::default();
        assert!(defaults.performance.keep_running_background);
        assert_eq!(defaults.performance.background_idle_timeout_seconds, 30);
        assert_eq!(defaults.performance.startup_behavior, "balanced");
        assert_eq!(defaults.performance.thumbnail_cache_limit_mb, 512);

        let config: Config = toml::from_str(
            r#"
            [performance]
            keep_running_background = false
            background_idle_timeout_seconds = 0
            startup_behavior = "fast_reopen"
            thumbnail_cache_limit_mb = 1024
            "#,
        )
        .unwrap();
        let restored: Config = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert!(!restored.performance.keep_running_background);
        assert_eq!(restored.performance.background_idle_timeout_seconds, 0);
        assert_eq!(restored.performance.startup_behavior, "fast_reopen");
        assert_eq!(restored.performance.thumbnail_cache_limit_mb, 1024);
        assert!(restored.ui.preview_visible);
    }
}
