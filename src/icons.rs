//! Freedesktop icon lookup for file rows.
//!
//! Qt's QML `Image` can load SVG when the Qt SVG image plugin is present. We
//! resolve icon files ourselves so the UI does not depend on a Qt icon provider
//! or a particular icon theme being installed.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::paths::{format_size, home_dir, path_to_file_url};

const MAX_ICON_CACHE_ENTRIES: usize = 1024;

#[derive(Debug, Clone)]
pub struct IconResolver {
    theme_chain: Vec<PathBuf>,
    cache: Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl IconResolver {
    pub fn shared() -> Arc<Self> {
        static RESOLVER: OnceLock<Arc<IconResolver>> = OnceLock::new();
        Arc::clone(RESOLVER.get_or_init(|| Arc::new(Self::new())))
    }

    pub fn new() -> Self {
        let roots = icon_roots();
        let requested = read_gtk_icon_theme().unwrap_or_else(|| "Papirus-Dark".to_owned());
        let mut theme_chain = Vec::new();
        let mut seen = HashSet::new();

        let mut queue = vec![requested];
        // These are sensible fallbacks for a minimal NixOS install.
        queue.extend(
            [
                "Papirus-Dark",
                "Papirus",
                "Adwaita",
                "breeze-dark",
                "breeze",
                "hicolor",
            ]
            .iter()
            .map(|s| (*s).to_owned()),
        );

        while let Some(theme) = queue.pop() {
            if !seen.insert(theme.clone()) {
                continue;
            }
            let Some(dir) = roots.iter().map(|r| r.join(&theme)).find(|p| p.is_dir()) else {
                continue;
            };
            let inherits = read_inherits(&dir);
            theme_chain.push(dir);
            queue.extend(inherits);
        }

        if theme_chain.is_empty() {
            if let Some(home) = roots.first() {
                theme_chain.push(home.join("hicolor"));
            }
        }

        Self {
            theme_chain,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn resolve(&self, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        if let Ok(cache) = self.cache.lock() {
            if let Some(value) = cache.get(name) {
                return value.clone();
            }
        }

        let found = self.find(name).map(|path| path_to_file_url(&path));
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= MAX_ICON_CACHE_ENTRIES {
                cache.clear();
            }
            cache.insert(name.to_owned(), found.clone());
        }
        found
    }

    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    pub fn resolve_first(&self, names: &[&str]) -> Option<String> {
        names.iter().find_map(|name| self.resolve(name))
    }

    fn find(&self, name: &str) -> Option<PathBuf> {
        const CATEGORIES: [&str; 6] = [
            "places",
            "mimetypes",
            "apps",
            "devices",
            "actions",
            "status",
        ];
        const SIZES: [&str; 9] = [
            "256x256", "128x128", "96x96", "64x64", "48x48", "32x32", "24x24", "22x22", "16x16",
        ];

        for theme in &self.theme_chain {
            for format in ["svg", "png"] {
                let candidate = theme
                    .join("scalable")
                    .join("places")
                    .join(format!("{name}.{format}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
                for category in CATEGORIES {
                    let candidate = theme
                        .join("scalable")
                        .join(category)
                        .join(format!("{name}.{format}"));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
                for size in SIZES {
                    for scale in ["", "@2x"] {
                        let dir = format!("{size}{scale}");
                        for category in CATEGORIES {
                            let candidate = theme
                                .join(&dir)
                                .join(category)
                                .join(format!("{name}.{format}"));
                            if candidate.is_file() {
                                return Some(candidate);
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

pub fn clear_shared_cache() {
    IconResolver::shared().clear_cache();
}

fn icon_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |path: PathBuf| {
        if seen.insert(path.clone()) {
            roots.push(path);
        }
    };

    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        add(PathBuf::from(data_home).join("icons"));
    } else {
        add(home_dir().join(".local/share/icons"));
    }
    add(home_dir().join(".nix-profile/share/icons"));
    if let Ok(user) = env::var("USER") {
        add(PathBuf::from(format!(
            "/etc/profiles/per-user/{user}/share/icons"
        )));
    }
    if let Some(data_dirs) = env::var_os("XDG_DATA_DIRS") {
        for dir in env::split_paths(&data_dirs) {
            add(dir.join("icons"));
        }
    }
    add(PathBuf::from("/run/current-system/sw/share/icons"));
    add(PathBuf::from("/usr/share/icons"));
    add(PathBuf::from("/usr/local/share/icons"));
    roots
}

fn read_gtk_icon_theme() -> Option<String> {
    let path = home_dir().join(".config/gtk-3.0/settings.ini");
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("gtk-icon-theme-name=") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn read_inherits(theme_dir: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(theme_dir.join("index.theme")) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Inherits=") {
            for item in value.split(',') {
                let item = item.trim();
                if !item.is_empty() {
                    result.push(item.to_owned());
                }
            }
        }
    }
    result
}

pub fn icon_names_for(name: &str, is_dir: bool, extension: &str) -> Vec<&'static str> {
    if is_dir {
        let special: &[&str] = match name {
            "Desktop" => &["folder-desktop", "folder"],
            "Documents" => &["folder-documents", "folder"],
            "Downloads" => &["folder-download", "folder"],
            "Music" => &["folder-music", "folder"],
            "Pictures" => &["folder-pictures", "folder"],
            "Videos" => &["folder-videos", "folder"],
            ".git" => &["folder-git", "folder"],
            ".config" => &["folder-config", "folder"],
            _ => &["folder"],
        };
        return special.to_vec();
    }

    match extension {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "avif" | "heic"
        | "svg" => {
            vec!["image-x-generic"]
        }
        "pdf" => vec!["application-pdf"],
        "zip" | "tar" | "gz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "cab" | "lz4" => {
            vec!["package-x-generic", "application-x-archive"]
        }
        "iso" | "img" => vec!["application-x-cd-image", "package-x-generic"],
        "mp3" | "flac" | "ogg" | "opus" | "m4a" | "wav" | "aac" | "wma" => {
            vec!["audio-x-generic"]
        }
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "mpg" | "mpeg" | "ogv" => {
            vec!["video-x-generic"]
        }
        "ttf" | "otf" | "woff" | "woff2" => vec!["font-x-generic"],
        "doc" | "docx" | "odt" | "rtf" => vec!["x-office-document", "text-x-generic"],
        "xls" | "xlsx" | "ods" | "csv" => vec!["x-office-spreadsheet", "text-x-generic"],
        "ppt" | "pptx" | "odp" => vec!["x-office-presentation", "text-x-generic"],
        "desktop" => vec!["application-x-desktop", "text-x-generic"],
        "sh" | "bash" | "zsh" | "fish" | "py" | "rs" | "js" | "ts" | "go" | "c" | "h" | "cpp"
        | "hpp" | "lua" | "rb" | "pl" => {
            vec!["text-x-script", "text-x-generic"]
        }
        "txt" | "md" | "markdown" | "log" | "json" | "toml" | "yaml" | "yml" | "nix" | "conf"
        | "ini" | "xml" | "html" | "css" | "sql" => {
            vec!["text-x-generic"]
        }
        "deb" | "rpm" | "pkg" | "apk" => vec!["package-x-generic"],
        "exe" => vec!["application-x-executable"],
        _ => vec!["text-x-generic", "application-x-generic"],
    }
}

pub fn human_kind(_name: &str, is_dir: bool, extension: &str) -> String {
    if is_dir {
        return "Folder".to_owned();
    }
    match extension {
        "png" => "PNG image",
        "jpg" | "jpeg" => "JPEG image",
        "webp" => "WebP image",
        "gif" => "GIF image",
        "svg" => "SVG image",
        "avif" => "AVIF image",
        "tif" | "tiff" => "TIFF image",
        "bmp" => "Bitmap image",
        "pdf" => "PDF document",
        "txt" => "Text file",
        "md" | "markdown" => "Markdown document",
        "log" => "Log file",
        "json" => "JSON file",
        "toml" => "TOML file",
        "yaml" | "yml" => "YAML file",
        "nix" => "Nix expression",
        "rs" => "Rust source",
        "py" => "Python source",
        "js" => "JavaScript source",
        "ts" => "TypeScript source",
        "html" => "HTML document",
        "css" => "Stylesheet",
        "sh" | "bash" | "zsh" => "Shell script",
        "zip" => "ZIP archive",
        "tar" => "TAR archive",
        "gz" => "Gzip archive",
        "xz" => "XZ archive",
        "zst" => "Zstandard archive",
        "7z" => "7-Zip archive",
        "rar" => "RAR archive",
        "iso" | "img" => "Disk image",
        "mp3" => "MP3 audio",
        "flac" => "FLAC audio",
        "ogg" | "opus" => "Ogg audio",
        "wav" => "WAV audio",
        "m4a" => "M4A audio",
        "mp4" => "MP4 video",
        "mkv" => "Matroska video",
        "webm" => "WebM video",
        "avi" => "AVI video",
        "mov" => "QuickTime video",
        "ttf" | "otf" => "Font",
        "desktop" => "Desktop entry",
        "deb" | "rpm" => "Package",
        _ => "File",
    }
    .to_owned()
}

/// Formatting helper used in unit tests and by the backend.
#[allow(dead_code)]
pub fn entry_size_text(size: u64, is_dir: bool) -> String {
    if is_dir {
        "—".to_owned()
    } else {
        format_size(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directories_have_folder_icons() {
        let names = icon_names_for("Documents", true, "");
        assert!(names.contains(&"folder-documents"));
        let names = icon_names_for("anything", true, "");
        assert!(names.contains(&"folder"));
    }

    #[test]
    fn common_file_types_have_icon_fallbacks() {
        assert!(icon_names_for("x", false, "pdf").contains(&"application-pdf"));
        assert!(icon_names_for("x", false, "png").contains(&"image-x-generic"));
        assert!(icon_names_for("x", false, "zip").contains(&"package-x-generic"));
        assert!(icon_names_for("x", false, "rs").contains(&"text-x-script"));
        assert!(icon_names_for("x", false, "weird").contains(&"text-x-generic"));
    }

    #[test]
    fn size_text_hides_directory_sizes() {
        assert_eq!(entry_size_text(1234, true), "—");
        assert_eq!(entry_size_text(1234, false), "1.2 KiB");
    }
}
