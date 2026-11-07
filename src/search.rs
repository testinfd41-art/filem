//! Query parsing, ranked filename search and bounded filesystem traversal.
//!
//! Search is on demand: no index, no daemon, no whole-home scan on every
//! keystroke. The caller debounces input and cancels obsolete requests. The
//! walker stays inside one filesystem, skips heavy build/cache directories and
//! never follows directory symlinks.

use std::cmp::Ordering as CmpOrdering;
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::fs_entry::{read_directory_for_search, Entry};
use crate::icons::{icon_names_for, IconResolver};
use crate::natsort::natural_cmp;
use crate::tags::TagStore;

const SKIP_DIRS: [&str; 10] = [
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "__pycache__",
    ".cache",
    ".npm",
    ".cargo",
    ".local",
];
const MAX_DEPTH: usize = 24;
const MAX_VISITED: usize = 150_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeFilter {
    Image,
    Video,
    Audio,
    Archive,
    Folder,
    Text,
    Document,
}

impl TypeFilter {
    fn parse(value: &str) -> Option<Self> {
        match value.to_lowercase().as_str() {
            "image" | "images" | "picture" => Some(Self::Image),
            "video" | "videos" => Some(Self::Video),
            "audio" | "music" => Some(Self::Audio),
            "archive" | "archives" => Some(Self::Archive),
            "folder" | "dir" | "directory" => Some(Self::Folder),
            "text" | "code" | "source" => Some(Self::Text),
            "document" | "doc" | "pdf" => Some(Self::Document),
            _ => None,
        }
    }

    fn matches(self, entry: &Entry) -> bool {
        if self == Self::Folder {
            return entry.is_dir;
        }
        if entry.is_dir {
            return false;
        }
        let ext = entry.extension.as_str();
        match self {
            Self::Folder => entry.is_dir,
            Self::Image => matches!(
                ext,
                "png"
                    | "jpg"
                    | "jpeg"
                    | "webp"
                    | "gif"
                    | "svg"
                    | "bmp"
                    | "tif"
                    | "tiff"
                    | "avif"
                    | "ico"
            ),
            Self::Video => matches!(
                ext,
                "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "mpg" | "mpeg" | "ogv"
            ),
            Self::Audio => matches!(
                ext,
                "mp3" | "flac" | "ogg" | "opus" | "m4a" | "wav" | "aac" | "wma"
            ),
            Self::Archive => matches!(
                ext,
                "zip" | "tar" | "tar.gz" | "tgz" | "gz" | "bz2" | "xz" | "zst" | "7z" | "rar"
            ),
            Self::Document => matches!(
                ext,
                "pdf"
                    | "doc"
                    | "docx"
                    | "odt"
                    | "rtf"
                    | "xls"
                    | "xlsx"
                    | "ods"
                    | "ppt"
                    | "pptx"
                    | "odp"
            ),
            Self::Text => matches!(
                ext,
                "txt"
                    | "md"
                    | "markdown"
                    | "log"
                    | "json"
                    | "toml"
                    | "yaml"
                    | "yml"
                    | "nix"
                    | "rs"
                    | "py"
                    | "js"
                    | "ts"
                    | "tsx"
                    | "jsx"
                    | "c"
                    | "h"
                    | "cpp"
                    | "hpp"
                    | "sh"
                    | "bash"
                    | "zsh"
                    | "css"
                    | "html"
                    | "xml"
                    | "sql"
                    | "conf"
                    | "ini"
                    | "csv"
            ),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub terms: Vec<String>,
    pub tags: Vec<String>,
    pub extensions: Vec<String>,
    pub types: Vec<TypeFilter>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
}

pub fn parse_size(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (number, multiplier) = match value.chars().last()?.to_ascii_lowercase() {
        'k' => (&value[..value.len() - 1], 1024u64),
        'm' => (&value[..value.len() - 1], 1024 * 1024),
        'g' => (&value[..value.len() - 1], 1024 * 1024 * 1024),
        't' => (&value[..value.len() - 1], 1024u64.pow(4)),
        _ => (value, 1),
    };
    let number: f64 = number.parse().ok()?;
    if !number.is_finite() || number < 0.0 {
        return None;
    }
    Some((number * multiplier as f64).round() as u64)
}

impl SearchQuery {
    pub fn parse(input: &str) -> Self {
        let mut query = SearchQuery::default();
        for token in query_tokens(input) {
            let token = token.as_str();
            if let Some(value) = token
                .strip_prefix("tag:")
                .or_else(|| token.strip_prefix("#"))
            {
                if !value.is_empty() {
                    query.tags.push(value.to_lowercase());
                }
                continue;
            }
            if let Some(value) = token
                .strip_prefix("ext:")
                .or_else(|| token.strip_prefix("*."))
            {
                let value = value.trim_start_matches('.').to_lowercase();
                if !value.is_empty() {
                    query.extensions.push(value);
                }
                continue;
            }
            if let Some(value) = token.strip_prefix("type:") {
                if let Some(kind) = TypeFilter::parse(value) {
                    query.types.push(kind);
                }
                continue;
            }
            if let Some(value) = token.strip_prefix("size:") {
                let (min, max) = parse_size_range(value);
                if min.is_some() {
                    query.min_size = min;
                }
                if max.is_some() {
                    query.max_size = max;
                }
                continue;
            }
            if let Some(value) = token.strip_prefix("name:") {
                if !value.is_empty() {
                    query.terms.push(value.to_lowercase());
                }
                continue;
            }
            query.terms.push(token.to_lowercase());
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
            && self.tags.is_empty()
            && self.extensions.is_empty()
            && self.types.is_empty()
            && self.min_size.is_none()
            && self.max_size.is_none()
    }
}

fn query_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in input.chars() {
        if escaped {
            token.push(character);
            escaped = false;
        } else if character == '\\' && quoted {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character.is_whitespace() && !quoted {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
        } else {
            token.push(character);
        }
    }
    if escaped {
        token.push('\\');
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

fn parse_size_range(value: &str) -> (Option<u64>, Option<u64>) {
    let value = value.trim();
    for (prefix, is_min) in [
        (">=", true),
        ("<=", false),
        (">", true),
        ("<", false),
        ("=", true),
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            let size = parse_size(rest);
            return if is_min { (size, None) } else { (None, size) };
        }
    }
    if let Some((min, max)) = value.split_once("..") {
        return (parse_size(min), parse_size(max));
    }
    (parse_size(value), None)
}

/// Subsequence match with a mild gap penalty. Lower rank is better.
fn fuzzy_score(haystack: &str, needle: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = haystack.chars().collect();
    let need: Vec<char> = needle.chars().collect();
    if hay == need {
        return Some(1200);
    }
    if haystarts(haystack, needle) {
        return Some(1000 - (hay.len().saturating_sub(need.len()) as i32 / 8));
    }
    if let Some(position) = haystack.find(needle) {
        let boundary = position == 0
            || haystack[..position]
                .chars()
                .last()
                .map(|c| !c.is_alphanumeric())
                .unwrap_or(true);
        let bonus = if boundary { 120 } else { 0 };
        return Some(800 + bonus - (position as i32).min(200));
    }
    // Subsequence: all characters in order.
    let mut indices = Vec::with_capacity(need.len());
    let mut cursor = 0usize;
    for wanted in &need {
        let mut found = None;
        while cursor < hay.len() {
            if hay[cursor] == *wanted {
                found = Some(cursor);
                cursor += 1;
                break;
            }
            cursor += 1;
        }
        indices.push(found?);
    }
    let span = indices.last()? - indices.first()? + 1;
    let gaps = span.saturating_sub(need.len()) as i32;
    Some(400 - gaps.min(300))
}

fn haystarts(haystack: &str, needle: &str) -> bool {
    haystack.len() >= needle.len()
        && haystack.is_char_boundary(needle.len())
        && &haystack[..needle.len()] == needle
}

fn tag_matches(query: &[String], tag_names: &TagStore, entry: &Entry) -> bool {
    if query.is_empty() {
        return true;
    }
    let assigned = tag_names.tags_for(&entry.path, entry.dev, entry.ino);
    query.iter().any(|wanted| {
        if wanted == "untagged" {
            return assigned.is_empty();
        }
        assigned.iter().any(|index| {
            let index = *index as usize;
            wanted == &tag_names.name(index).to_lowercase() || wanted == &(index + 1).to_string()
        })
    })
}

impl Entry {
    fn matches_query(&self, query: &SearchQuery, tags: &TagStore) -> bool {
        if !tag_matches(&query.tags, tags, self) {
            return false;
        }
        if !query.extensions.is_empty() && !query.extensions.contains(&self.extension) {
            return false;
        }
        if !query.types.is_empty() && !query.types.iter().any(|kind| kind.matches(self)) {
            return false;
        }
        if let Some(min) = query.min_size {
            if self.is_dir || self.size < min {
                return false;
            }
        }
        if let Some(max) = query.max_size {
            if self.is_dir || self.size > max {
                return false;
            }
        }
        true
    }

    fn score(&self, terms: &[String]) -> Option<i32> {
        let mut total = 0i32;
        let name = self.name.to_lowercase();
        let extension = self.extension.to_lowercase();

        for term in terms {
            total += fuzzy_score(&name, term)?;
            if extension.contains(term) {
                total += 80;
            }
        }
        Some(total)
    }
}

struct ScoredEntry {
    score: i32,
    entry: Entry,
}

impl PartialEq for ScoredEntry {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score && self.entry.path == other.entry.path
    }
}

impl Eq for ScoredEntry {}

impl PartialOrd for ScoredEntry {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredEntry {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        other
            .score
            .cmp(&self.score)
            .then_with(|| natural_cmp(&self.entry.name, &other.entry.name))
            .then_with(|| natural_cmp(&self.entry.display_path, &other.entry.display_path))
            .then_with(|| self.entry.path.cmp(&other.entry.path))
    }
}

#[derive(Debug, Default)]
pub struct SearchOutcome {
    pub hits: Vec<Entry>,
    pub truncated: bool,
    pub cancelled: bool,
    pub visited: usize,
}

pub fn search(
    root: &Path,
    raw_query: &str,
    recursive: bool,
    show_hidden: bool,
    tags: &TagStore,
    icons: &Arc<IconResolver>,
    cancel: &AtomicBool,
    max_results: usize,
) -> SearchOutcome {
    let query = SearchQuery::parse(raw_query);
    let mut outcome = SearchOutcome::default();
    if query.is_empty() {
        return outcome;
    }
    let Ok(root_meta) = std::fs::metadata(root) else {
        return outcome;
    };
    let root_device = {
        use std::os::unix::fs::MetadataExt;
        root_meta.dev()
    };
    let mut scored = BinaryHeap::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];

    while let Some((directory, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            break;
        }
        if !recursive && depth > 0 {
            continue;
        }
        let entries = match read_directory_for_search(directory.as_path(), show_hidden) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        outcome.visited += entries.len();
        for mut entry in entries {
            if cancel.load(Ordering::Relaxed) {
                outcome.cancelled = true;
                break;
            }
            entry.tag = tags.tag_for(&entry.path, entry.dev, entry.ino);
            let entry_path = entry.path.clone();
            // Compute same-device early, before entry might be moved below.
            let same_device = if recursive
                && entry.is_dir
                && !entry.is_symlink
                && depth < MAX_DEPTH
                && !SKIP_DIRS.contains(&entry.name.as_str())
            {
                entry.dev == root_device
            } else {
                false
            };
            if entry.matches_query(&query, tags) {
                if let Some(score) = entry.score(&query.terms) {
                    entry.display_path = crate::ops::display_path(&entry_path);
                    let icon_names = if entry.is_symlink && entry.kind == "Broken link" {
                        vec!["emblem-broken-link", "text-x-generic"]
                    } else {
                        icon_names_for(&entry.name, entry.is_dir, &entry.extension)
                    };
                    entry.icon_url = icons.resolve_first(&icon_names).unwrap_or_default();
                    if max_results == 0 {
                        outcome.truncated = true;
                    } else {
                        let candidate = ScoredEntry { score, entry };
                        if scored.len() < max_results {
                            scored.push(candidate);
                        } else {
                            outcome.truncated = true;
                            if scored
                                .peek()
                                .is_some_and(|worst| candidate.cmp(worst) == CmpOrdering::Less)
                            {
                                scored.pop();
                                scored.push(candidate);
                            }
                        }
                    }
                }
                if same_device {
                    stack.push((entry_path, depth + 1));
                }
            } else if same_device {
                stack.push((entry_path, depth + 1));
            }
        }
        if outcome.visited >= MAX_VISITED {
            outcome.truncated = true;
            break;
        }
    }

    let mut scored = scored.into_vec();
    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| natural_cmp(&a.entry.name, &b.entry.name))
            .then_with(|| natural_cmp(&a.entry.display_path, &b.entry.display_path))
            .then_with(|| a.entry.path.cmp(&b.entry.path))
    });
    outcome.hits = scored.into_iter().map(|hit| hit.entry).collect();
    outcome
}

pub fn search_entries(
    entries: &[Entry],
    raw_query: &str,
    tags: &TagStore,
    cancel: &AtomicBool,
    max_results: usize,
) -> SearchOutcome {
    let query = SearchQuery::parse(raw_query);
    let mut outcome = SearchOutcome::default();
    if query.is_empty() {
        return outcome;
    }

    let mut scored = BinaryHeap::new();
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            break;
        }
        outcome.visited += 1;
        if !entry.matches_query(&query, tags) {
            continue;
        }
        let Some(score) = entry.score(&query.terms) else {
            continue;
        };
        scored.push(ScoredEntry {
            score,
            entry: entry.clone(),
        });
        if scored.len() > max_results {
            scored.pop();
            outcome.truncated = true;
        }
    }
    outcome.hits = scored
        .into_sorted_vec()
        .into_iter()
        .map(|hit| hit.entry)
        .collect();
    outcome
}

/// A light-weight suggestion helper used by the empty search state.
pub fn describe_query(query: &SearchQuery) -> String {
    let mut parts = Vec::new();
    if !query.terms.is_empty() {
        parts.push(format!("name: {}", query.terms.join(" ")));
    }
    if !query.tags.is_empty() {
        parts.push(format!("tag: {}", query.tags.join(", ")));
    }
    if !query.extensions.is_empty() {
        parts.push(format!("extension: {}", query.extensions.join(", ")));
    }
    if !query.types.is_empty() {
        parts.push(format!("{} type filter(s)", query.types.len()));
    }
    if parts.is_empty() {
        "Everything".to_owned()
    } else {
        parts.join(" · ")
    }
}

/// Path used by tests and diagnostics.
#[allow(dead_code)]
pub fn search_root(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_entry::Entry;
    use std::fs;
    use std::path::PathBuf;

    fn entry(name: &str, ext: &str, is_dir: bool, size: u64, tag: i32) -> Entry {
        Entry {
            name: name.to_owned(),
            path: PathBuf::from(format!("/tmp/{name}")),
            is_dir,
            is_symlink: false,
            is_hidden: false,
            size,
            modified_ms: 0.0,
            kind: String::new(),
            icon_url: String::new(),
            extension: ext.to_owned(),
            dev: 0,
            ino: 0,
            tag,
            display_path: String::new(),
            tree_depth: 0,
            tree_expanded: false,
        }
    }

    #[test]
    fn parses_filters_and_terms() {
        let query = SearchQuery::parse("report tag:red ext:pdf type:document size:>10M");
        assert_eq!(query.terms, ["report"]);
        assert_eq!(query.tags, ["red"]);
        assert_eq!(query.extensions, ["pdf"]);
        assert_eq!(query.types, [TypeFilter::Document]);
        assert_eq!(query.min_size, Some(10 * 1024 * 1024));
        assert!(!query.is_empty());
        assert!(SearchQuery::parse("   ").is_empty());
    }

    #[test]
    fn quoted_names_are_kept_as_single_search_terms() {
        let query = SearchQuery::parse(r#""project report" name:"final draft""#);
        assert_eq!(query.terms, ["project report", "final draft"]);
        assert_eq!(
            query_tokens(r#""escaped \"quote\"""#),
            [r#"escaped "quote""#]
        );
    }

    #[test]
    fn size_parser_handles_units_and_ranges() {
        assert_eq!(parse_size("2K"), Some(2048));
        assert_eq!(parse_size("1.5M"), Some(1_572_864));
        assert_eq!(parse_size_range(">5M").0, Some(5 * 1024 * 1024));
        assert_eq!(parse_size_range("<5M").1, Some(5 * 1024 * 1024));
        assert_eq!(
            parse_size_range("1M..2M"),
            (Some(1_048_576), Some(2_097_152))
        );
    }

    #[test]
    fn fuzzy_ranking_prefers_exact_prefix_and_substring() {
        let exact = fuzzy_score("report.pdf", "report.pdf").unwrap();
        let prefix = fuzzy_score("report-2026.pdf", "report").unwrap();
        let contains = fuzzy_score("my-report.pdf", "report").unwrap();
        let subseq = fuzzy_score("r-e-p-o-r-t.pdf", "report").unwrap();
        assert!(exact > prefix && prefix > contains && contains > subseq);
        assert!(fuzzy_score("report.pdf", "zzz").is_none());
    }

    #[test]
    fn filename_ranking_prefers_exact_prefix_and_fuzzy_matches() {
        let query = SearchQuery::parse("report");
        let exact = entry("report.txt", "txt", false, 1, -1);
        let prefix = entry("report-old.txt", "txt", false, 1, -1);
        let weak = entry("r-e-p-o-r-t.txt", "txt", false, 1, -1);
        let mut unrelated = entry("notes.txt", "txt", false, 1, -1);
        unrelated.path = PathBuf::from("/tmp/report/notes.txt");

        assert!(exact.score(&query.terms).unwrap() > prefix.score(&query.terms).unwrap());
        assert!(prefix.score(&query.terms).unwrap() > weak.score(&query.terms).unwrap());
        assert_eq!(unrelated.score(&query.terms), None);
    }

    #[test]
    fn recent_entry_search_uses_names_and_metadata_filters() {
        let entries = vec![
            entry("report.pdf", "pdf", false, 10, -1),
            entry("report.txt", "txt", false, 10, -1),
            entry("notes.pdf", "pdf", false, 10, -1),
        ];
        let outcome = search_entries(
            &entries,
            "report type:document",
            &TagStore::default(),
            &AtomicBool::new(false),
            50,
        );

        assert_eq!(outcome.hits.len(), 1);
        assert_eq!(outcome.hits[0].name, "report.pdf");
    }

    #[test]
    fn tag_and_type_filters_restrict_matches() {
        let mut tags = TagStore::default();
        tags.set_tag(Path::new("/tmp/a.pdf"), 0, 0, Some(0));
        tags.set_tag(Path::new("/tmp/a.pdf"), 0, 0, Some(4));
        tags.by_path.insert("/tmp/a.pdf".to_owned(), 0);
        let query = SearchQuery::parse("tag:red type:document");
        let multi_tag_query = SearchQuery::parse("tag:blue");
        let untagged_query = SearchQuery::parse("tag:untagged");
        let hit = entry("a.pdf", "pdf", false, 10, 0);
        let miss_tag = entry("b.pdf", "pdf", false, 10, -1);
        let miss_type = entry("c.txt", "txt", false, 10, 0);
        assert!(hit.matches_query(&query, &tags));
        assert!(hit.matches_query(&multi_tag_query, &tags));
        assert!(!miss_tag.matches_query(&query, &tags));
        assert!(miss_tag.matches_query(&untagged_query, &tags));
        assert!(!miss_type.matches_query(&query, &tags));
    }

    #[test]
    fn search_finds_nested_files_but_skips_build_dirs() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("docs/deep")).unwrap();
        fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        fs::write(dir.path().join("docs/deep/report.txt"), b"hello").unwrap();
        fs::write(dir.path().join("node_modules/pkg/report.txt"), b"hidden").unwrap();
        let icons = Arc::new(IconResolver::new());
        let tags = TagStore::default();
        let cancel = AtomicBool::new(false);
        let outcome = search(
            dir.path(),
            "report",
            true,
            false,
            &tags,
            &icons,
            &cancel,
            50,
        );
        assert_eq!(outcome.hits.len(), 1, "{:?}", outcome.hits);
        assert!(outcome.hits[0].path.ends_with("docs/deep/report.txt"));
        assert!(!outcome.hits[0].display_path.is_empty());
    }

    #[test]
    fn quoted_phrase_search_finds_names_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("project report final.txt"), b"").unwrap();
        fs::write(dir.path().join("project-reports.txt"), b"").unwrap();
        let outcome = search(
            dir.path(),
            r#""project report""#,
            false,
            false,
            &TagStore::default(),
            &Arc::new(IconResolver::new()),
            &AtomicBool::new(false),
            50,
        );

        assert_eq!(outcome.hits.len(), 1);
        assert_eq!(outcome.hits[0].name, "project report final.txt");
    }

    #[test]
    fn non_recursive_search_only_looks_at_the_top_level() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("nested")).unwrap();
        fs::write(dir.path().join("top.txt"), b"x").unwrap();
        fs::write(dir.path().join("nested/top.txt"), b"x").unwrap();
        let icons = Arc::new(IconResolver::new());
        let tags = TagStore::default();
        let cancel = AtomicBool::new(false);
        let outcome = search(dir.path(), "top", false, false, &tags, &icons, &cancel, 50);
        assert_eq!(outcome.hits.len(), 1);
    }

    #[test]
    fn search_ranks_matches_beyond_the_initial_result_buffer() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "a-target.txt",
            "b-target.txt",
            "c-target.txt",
            "d-target.txt",
        ] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        fs::write(dir.path().join("target"), b"").unwrap();

        let outcome = search(
            dir.path(),
            "target",
            false,
            false,
            &TagStore::default(),
            &Arc::new(IconResolver::new()),
            &AtomicBool::new(false),
            1,
        );

        assert_eq!(outcome.hits.len(), 1);
        assert_eq!(outcome.hits[0].name, "target");
        assert!(outcome.truncated);
    }
}
