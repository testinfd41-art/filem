//! XDG paths, user directories, breadcrumb construction and small path helpers.

use std::env;
use std::path::{Component, Path, PathBuf};

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

/// Characters that must be escaped in a local file URL path.
const URL_PATH_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    pub label: String,
    pub path: PathBuf,
}

pub fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn config_dir() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".config"))
}

pub fn data_dir() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".local/share"))
}

pub fn state_dir() -> PathBuf {
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".local/state"))
}

#[allow(dead_code)]
pub fn cache_dir() -> PathBuf {
    env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".cache"))
}

pub fn app_config_dir() -> PathBuf {
    config_dir().join("filem")
}

pub fn theme_file() -> PathBuf {
    app_config_dir().join("theme.json")
}

pub fn appearance_file() -> PathBuf {
    app_config_dir().join("appearance.json")
}

pub fn config_file() -> PathBuf {
    app_config_dir().join("config.toml")
}

pub fn trash_dir() -> PathBuf {
    data_dir().join("Trash/files")
}

/// Expand `~` and make a user-entered path absolute relative to `cwd`.
///
/// This is a lexical operation. It deliberately does not resolve symlinks, so
/// navigating through a symlinked directory keeps the user's visible path.
pub fn expand_path(input: &str, cwd: &Path) -> Option<PathBuf> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let decoded;
    let input = if let Some(rest) = input.strip_prefix("file://") {
        // Local file URLs from desktop launchers. A non-empty host is not a
        // local path and is rejected rather than silently misread.
        if !rest.starts_with('/') {
            return None;
        }
        decoded = percent_decode_str(rest).decode_utf8().ok()?.into_owned();
        decoded.as_str()
    } else {
        input
    };

    let path = if input == "~" {
        home_dir()
    } else if let Some(rest) = input.strip_prefix("~/") {
        home_dir().join(rest)
    } else if input.starts_with('~') {
        // Named-user expansion is intentionally unsupported rather than
        // guessed. The location bar shows an ordinary error for it.
        return None;
    } else {
        PathBuf::from(input)
    };

    let absolute = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    Some(clean_lexically(&absolute))
}

/// Collapse `.` and `..` components without touching the filesystem.
pub fn clean_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("/");
                }
            }
            Component::Normal(part) => out.push(part),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        out
    }
}

/// Build breadcrumbs for an absolute path. Paths under the user's home start
/// with a friendly "Home" crumb; other paths start at the filesystem root.
pub fn breadcrumbs(path: &Path) -> Vec<Crumb> {
    let home = home_dir();
    let mut result = Vec::new();

    let (start, relative) = if path == home {
        (home.clone(), PathBuf::new())
    } else if let Ok(rest) = path.strip_prefix(&home) {
        (home.clone(), rest.to_path_buf())
    } else {
        (
            PathBuf::from("/"),
            path.strip_prefix("/").unwrap_or(path).to_path_buf(),
        )
    };

    result.push(Crumb {
        label: if start == Path::new("/") {
            "Root".to_owned()
        } else {
            "Home".to_owned()
        },
        path: start.clone(),
    });

    let mut current = start;
    for component in relative.components() {
        if let Component::Normal(part) = component {
            current.push(part);
            result.push(Crumb {
                label: part.to_string_lossy().into_owned(),
                path: current.clone(),
            });
        }
    }

    result
}

pub fn path_to_file_url(path: &Path) -> String {
    let text = path.to_string_lossy();
    let encoded = utf8_percent_encode(&text, URL_PATH_SET);
    format!("file://{encoded}")
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn parse_shell_value(value: &str, home: &Path) -> Option<PathBuf> {
    let mut expanded = value.trim().to_owned();
    if expanded.is_empty() {
        return None;
    }
    expanded = expanded
        .replace("${HOME}", &home.to_string_lossy())
        .replace("$HOME", &home.to_string_lossy());
    if let Some(rest) = expanded.strip_prefix("~/") {
        expanded = home.join(rest).to_string_lossy().into_owned();
    } else if expanded == "~" {
        expanded = home.to_string_lossy().into_owned();
    }
    let path = PathBuf::from(expanded);
    if path.is_absolute() {
        Some(clean_lexically(&path))
    } else {
        None
    }
}

/// Read the XDG user directories without assuming English directory names.
/// Missing entries fall back to the conventional directory only if it exists.
pub fn user_places() -> Vec<Place> {
    let home = home_dir();
    let mut values: Vec<(String, PathBuf)> = Vec::new();
    let user_dirs = config_dir().join("user-dirs.dirs");

    if let Ok(contents) = std::fs::read_to_string(&user_dirs) {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            if let Some(path) = parse_shell_value(value, &home) {
                values.push((key.to_owned(), path));
            }
        }
    }

    let lookup = |key: &str| -> Option<PathBuf> {
        values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, p)| p.clone())
    };

    let fallback = |name: &str| -> Option<PathBuf> {
        let path = home.join(name);
        path.exists().then_some(path)
    };

    let mut places = vec![Place {
        id: "home".to_owned(),
        label: "Home".to_owned(),
        path: home.clone(),
    }];

    let wanted = [
        ("XDG_DESKTOP_DIR", "Desktop", "Desktop"),
        ("XDG_DOCUMENTS_DIR", "Documents", "Documents"),
        ("XDG_DOWNLOAD_DIR", "Downloads", "Downloads"),
        ("XDG_MUSIC_DIR", "Music", "Music"),
        ("XDG_PICTURES_DIR", "Pictures", "Pictures"),
        ("XDG_VIDEOS_DIR", "Videos", "Videos"),
    ];

    for (key, label, fallback_name) in wanted {
        if let Some(path) = lookup(key).or_else(|| fallback(fallback_name)) {
            places.push(Place {
                id: label.to_lowercase(),
                label: label.to_owned(),
                path,
            });
        }
    }

    let trash = trash_dir();
    if trash.exists() {
        places.push(Place {
            id: "trash".to_owned(),
            label: "Trash".to_owned(),
            path: trash,
        });
    }

    places
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_formatting_is_human_readable() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(10 * 1024 * 1024), "10 MiB");
    }

    #[test]
    fn file_urls_escape_spaces_and_specials() {
        let url = path_to_file_url(Path::new("/tmp/a b#c?.txt"));
        assert_eq!(url, "file:///tmp/a%20b%23c%3F.txt");
    }

    #[test]
    fn lexical_cleanup_keeps_absolute_paths() {
        assert_eq!(
            clean_lexically(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(clean_lexically(Path::new("/")), PathBuf::from("/"));
        assert_eq!(clean_lexically(Path::new("a/b")), PathBuf::from("a/b"));
    }

    #[test]
    fn breadcrumbs_for_home_and_root() {
        let home = home_dir();
        let crumbs = breadcrumbs(&home);
        assert_eq!(crumbs[0].label, "Home");
        assert_eq!(crumbs[0].path, home);

        let crumbs = breadcrumbs(Path::new("/usr/local"));
        assert_eq!(crumbs[0].label, "Root");
        assert_eq!(crumbs[1].label, "usr");
        assert_eq!(crumbs[2].label, "local");
        assert_eq!(crumbs[2].path, PathBuf::from("/usr/local"));
    }

    #[test]
    fn expansion_rejects_named_tilde_accounts() {
        assert!(expand_path("~someone/files", Path::new("/")).is_none());
        assert_eq!(
            expand_path("relative", Path::new("/tmp")),
            Some(PathBuf::from("/tmp/relative"))
        );
    }

    #[test]
    fn local_file_urls_are_decoded() {
        assert_eq!(
            expand_path("file:///tmp/a%20b", Path::new("/")),
            Some(PathBuf::from("/tmp/a b"))
        );
        assert!(expand_path("file://localhost/tmp", Path::new("/")).is_none());
    }
}
