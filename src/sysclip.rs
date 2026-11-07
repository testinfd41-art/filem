//! Clipboard interoperability through the standard `text/uri-list` format.
//!
//! On Wayland, wl-copy/wl-paste are the portable tools available without
//! linking a new toolkit. If they are missing, Filem falls back to its internal
//! clipboard and says so in the UI rather than pretending interoperability.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::paths::{expand_path, home_dir};

pub fn uri_list(paths: &[PathBuf]) -> String {
    let mut text = String::new();
    for path in paths {
        text.push_str(&crate::paths::path_to_file_url(path));
        text.push_str("\r\n");
    }
    text
}

fn tool_available(binary: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(binary).is_file())
}

fn wl_copy(mime: &str, payload: &str) -> Result<(), String> {
    if !tool_available("wl-copy") {
        return Err(
            "wl-copy is not available; using Filem's internal clipboard instead.".to_owned(),
        );
    }
    let mut child = Command::new("wl-copy")
        .args(["--type", mime])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not start wl-copy: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload.as_bytes())
            .map_err(|error| format!("Could not write to the clipboard: {error}"))?;
    }
    // wl-copy keeps running as the selection owner. Reap it in the background.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Put file URLs on the system clipboard. The internal clipboard remains the
/// source of truth for cut vs copy.
pub fn write_paths(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    wl_copy("text/uri-list", &uri_list(paths))
}

/// Put plain text on the system clipboard, used by "Copy path".
pub fn write_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    wl_copy("text/plain", text)
}

/// Read file URLs from the system clipboard, returning local paths only.
pub fn read_paths() -> Vec<PathBuf> {
    if !tool_available("wl-paste") {
        return Vec::new();
    }
    let output = Command::new("wl-paste")
        .args(["--no-newline", "--type", "text/uri-list"])
        .output();
    let text = match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        _ => {
            let fallback = Command::new("wl-paste").args(["--no-newline"]).output();
            match fallback {
                Ok(output) if output.status.success() => {
                    String::from_utf8_lossy(&output.stdout).into_owned()
                }
                _ => return Vec::new(),
            }
        }
    };
    parse_uri_list(&text)
}

pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(path) = expand_path(line, &home_dir()) {
            if path.is_absolute() {
                paths.push(path);
            }
        }
    }
    paths
}

pub fn paths_text(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(dead_code)]
pub fn is_local(path: &Path) -> bool {
    path.is_absolute() && !path.as_os_str().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_lists_round_trip_spaces_and_crlf() {
        let paths = vec![PathBuf::from("/tmp/a b.txt")];
        let text = uri_list(&paths);
        assert!(text.contains("file:///tmp/a%20b.txt"));
        assert!(text.ends_with("\r\n"));
        assert_eq!(parse_uri_list(&text), paths);
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let text = "# comment\n\nfile:///tmp/x\n/tmp/y\n";
        assert_eq!(
            parse_uri_list(text),
            vec![PathBuf::from("/tmp/x"), PathBuf::from("/tmp/y")]
        );
    }
}
