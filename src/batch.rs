//! Side-effect-free batch rename planning and staged filesystem execution.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use regex::{Regex, RegexBuilder};

use crate::ops::{self, validate_file_name};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LetterCase {
    Upper,
    Lower,
    Title,
}

#[derive(Debug, Clone)]
pub enum RenameRule {
    Replace {
        find: String,
        replace: String,
        case_sensitive: bool,
        whole_word: bool,
        regex: bool,
        include_extension: bool,
    },
    Prefix(String),
    Suffix(String),
    RemoveText {
        text: String,
        case_sensitive: bool,
        whole_word: bool,
    },
    RemoveRange {
        start: usize,
        length: usize,
    },
    ChangeCase(LetterCase),
    CleanCharacters,
    NormalizeWhitespace,
    ReplaceSpaces(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionMode {
    Preserve,
    Change,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberPlacement {
    Prefix,
    Suffix,
    Template,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Selection,
    Name,
    Modified,
    Created,
}

#[derive(Debug, Clone)]
pub struct BatchOptions {
    pub rules: Vec<RenameRule>,
    pub template: String,
    pub extension_mode: ExtensionMode,
    pub extension: String,
    pub numbering: bool,
    pub start: i64,
    pub step: i64,
    pub padding: usize,
    pub number_placement: NumberPlacement,
    pub number_separator: String,
    pub sort: SortMode,
    pub descending: bool,
    pub case_insensitive: bool,
}

impl Default for BatchOptions {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            template: String::new(),
            extension_mode: ExtensionMode::Preserve,
            extension: String::new(),
            numbering: false,
            start: 1,
            step: 1,
            padding: 3,
            number_placement: NumberPlacement::Suffix,
            number_separator: "_".to_owned(),
            sort: SortMode::Selection,
            descending: false,
            case_insensitive: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSnapshot {
    device: u64,
    inode: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

impl SourceSnapshot {
    fn read(path: &Path) -> std::io::Result<Self> {
        Self::read_with_times(path).map(|(snapshot, _, _)| snapshot)
    }

    fn read_with_times(
        path: &Path,
    ) -> std::io::Result<(Self, Option<SystemTime>, Option<SystemTime>)> {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::symlink_metadata(path)?;
        Ok((
            Self {
                device: metadata.dev(),
                inode: metadata.ino(),
                size: metadata.size(),
                modified_seconds: metadata.mtime(),
                modified_nanoseconds: metadata.mtime_nsec(),
                changed_seconds: metadata.ctime(),
                changed_nanoseconds: metadata.ctime_nsec(),
            },
            metadata.modified().ok(),
            metadata.created().ok(),
        ))
    }

    fn identity(&self) -> (u64, u64) {
        (self.device, self.inode)
    }
}

#[derive(Debug, Clone)]
pub struct BatchRow {
    pub origin: PathBuf,
    pub old_name: String,
    pub new_name: String,
    pub error: String,
    pub conflict: bool,
    pub included: bool,
    pub unchanged: bool,
    pub result: String,
    pub old_prefix: String,
    pub old_changed: String,
    pub old_suffix: String,
    pub new_prefix: String,
    pub new_changed: String,
    pub new_suffix: String,
    pub snapshot: Option<SourceSnapshot>,
}

#[derive(Clone)]
enum PreparedRule {
    LiteralReplace {
        find: String,
        replace: String,
        case_sensitive: bool,
        whole_word: bool,
        include_extension: bool,
    },
    RegexReplace {
        regex: Regex,
        replace: String,
        include_extension: bool,
    },
    Other(RenameRule),
}

struct PlanInput {
    path: PathBuf,
    old_name: String,
    sort_name: String,
    snapshot: Option<SourceSnapshot>,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    included: bool,
    initial_error: String,
}

fn split_extension(name: &str) -> (&str, &str) {
    const COMPOUND_EXTENSIONS: [&str; 7] = [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".user.js", ".min.js", ".nii.gz",
    ];
    let lower = name.to_ascii_lowercase();
    for extension in COMPOUND_EXTENSIONS {
        if lower.ends_with(extension) && name.len() > extension.len() {
            return name.split_at(name.len() - extension.len());
        }
    }
    match name.rfind('.') {
        Some(index) if index > 0 => name.split_at(index),
        _ => (name, ""),
    }
}

fn number_text(number: i64, width: usize) -> String {
    if width == 0 {
        number.to_string()
    } else {
        format!("{number:0width$}")
    }
}

fn prepare_rules(rules: &[RenameRule]) -> Result<Vec<PreparedRule>, String> {
    rules
        .iter()
        .map(|rule| match rule {
            RenameRule::Replace {
                find,
                replace,
                case_sensitive,
                whole_word,
                regex,
                include_extension,
            } if find.is_empty() => Err("Find text cannot be empty.".to_owned()),
            RenameRule::Replace {
                find,
                replace,
                case_sensitive,
                whole_word,
                regex: true,
                include_extension,
            } => {
                let pattern = if *whole_word {
                    format!(r"\b(?:{find})\b")
                } else {
                    find.clone()
                };
                let regex = RegexBuilder::new(&pattern)
                    .case_insensitive(!case_sensitive)
                    .build()
                    .map_err(|error| format!("Invalid regular expression: {error}"))?;
                Ok(PreparedRule::RegexReplace {
                    regex,
                    replace: replace.clone(),
                    include_extension: *include_extension,
                })
            }
            RenameRule::Replace {
                find,
                replace,
                case_sensitive,
                whole_word,
                include_extension,
                ..
            } => Ok(PreparedRule::LiteralReplace {
                find: find.clone(),
                replace: replace.clone(),
                case_sensitive: *case_sensitive,
                whole_word: *whole_word,
                include_extension: *include_extension,
            }),
            other => Ok(PreparedRule::Other(other.clone())),
        })
        .collect()
}

fn word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn replace_literal(
    input: &str,
    needle: &str,
    replacement: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> String {
    if needle.is_empty() {
        return input.to_owned();
    }

    let (search_text, byte_ranges) = if case_sensitive {
        let mut ranges = Vec::with_capacity(input.len());
        for (start, character) in input.char_indices() {
            let range = (start, start + character.len_utf8());
            ranges.extend(std::iter::repeat_n(range, character.len_utf8()));
        }
        (input.to_owned(), ranges)
    } else {
        let mut folded = String::new();
        let mut ranges = Vec::new();
        for (start, character) in input.char_indices() {
            let end = start + character.len_utf8();
            for folded_character in character.to_lowercase() {
                let mut encoded = [0; 4];
                for _ in folded_character.encode_utf8(&mut encoded).bytes() {
                    ranges.push((start, end));
                }
                folded.push(folded_character);
            }
        }
        (folded, ranges)
    };
    let search_needle = if case_sensitive {
        needle.to_owned()
    } else {
        needle.to_lowercase()
    };
    if search_needle.is_empty() {
        return input.to_owned();
    }

    let mut result = String::with_capacity(input.len());
    let mut original_cursor = 0;
    let mut search_cursor = 0;
    while let Some(relative) = search_text[search_cursor..].find(&search_needle) {
        let start = search_cursor + relative;
        let end = start + search_needle.len();
        if end > byte_ranges.len() {
            break;
        }
        let original_start = byte_ranges[start].0;
        let original_end = byte_ranges[end - 1].1;
        let before = input[..original_start].chars().next_back();
        let after = input[original_end..].chars().next();
        let boundary_ok = !whole_word
            || (!before.is_some_and(word_character) && !after.is_some_and(word_character));
        if original_start >= original_cursor && boundary_ok {
            result.push_str(&input[original_cursor..original_start]);
            result.push_str(replacement);
            original_cursor = original_end;
        }
        search_cursor = end.max(start + 1);
        if search_cursor >= search_text.len() {
            break;
        }
    }
    result.push_str(&input[original_cursor..]);
    result
}

fn apply_text_rule(input: &str, rule: &PreparedRule) -> Result<String, String> {
    match rule {
        PreparedRule::LiteralReplace {
            find,
            replace,
            case_sensitive,
            whole_word,
            ..
        } => Ok(replace_literal(
            input,
            find,
            replace,
            *case_sensitive,
            *whole_word,
        )),
        PreparedRule::RegexReplace { regex, replace, .. } => {
            Ok(regex.replace_all(input, replace.as_str()).into_owned())
        }
        PreparedRule::Other(rule) => match rule {
            RenameRule::Prefix(prefix) => Ok(format!("{prefix}{input}")),
            RenameRule::Suffix(suffix) => Ok(format!("{input}{suffix}")),
            RenameRule::RemoveText {
                text,
                case_sensitive,
                whole_word,
            } => Ok(replace_literal(
                input,
                text,
                "",
                *case_sensitive,
                *whole_word,
            )),
            RenameRule::RemoveRange { start, length } => {
                let start_byte = input
                    .char_indices()
                    .nth(*start)
                    .map(|(index, _)| index)
                    .unwrap_or(input.len());
                let end_byte = input
                    .char_indices()
                    .nth(start.saturating_add(*length))
                    .map(|(index, _)| index)
                    .unwrap_or(input.len());
                let mut result = String::with_capacity(input.len());
                result.push_str(&input[..start_byte]);
                result.push_str(&input[end_byte.max(start_byte)..]);
                Ok(result)
            }
            RenameRule::ChangeCase(case) => Ok(change_case(input, *case)),
            RenameRule::CleanCharacters => Ok(input
                .chars()
                .filter(|character| !character.is_control())
                .collect::<String>()
                .trim()
                .to_owned()),
            RenameRule::NormalizeWhitespace => {
                Ok(input.split_whitespace().collect::<Vec<_>>().join(" "))
            }
            RenameRule::ReplaceSpaces(separator) => Ok(input.replace(' ', separator)),
            RenameRule::Replace { .. } => Err("Invalid batch rule.".to_owned()),
        },
    }
}

fn change_case(input: &str, case: LetterCase) -> String {
    match case {
        LetterCase::Upper => input.to_uppercase(),
        LetterCase::Lower => input.to_lowercase(),
        LetterCase::Title => {
            let mut at_word_start = true;
            input
                .chars()
                .flat_map(|character| {
                    if character.is_alphanumeric() {
                        let converted = if at_word_start {
                            character.to_uppercase().collect::<Vec<_>>()
                        } else {
                            character.to_lowercase().collect::<Vec<_>>()
                        };
                        at_word_start = false;
                        converted
                    } else {
                        at_word_start = true;
                        vec![character]
                    }
                })
                .collect()
        }
    }
}

fn render_template(
    template: &str,
    name: &str,
    extension: &str,
    number: i64,
    date: &str,
    modified: Option<&str>,
    parent: &str,
) -> Result<(String, bool), String> {
    let mut output = String::with_capacity(template.len() + name.len());
    let mut cursor = 0;
    let mut uses_extension = false;
    while let Some(relative_open) = template[cursor..].find('{') {
        let open = cursor + relative_open;
        output.push_str(&template[cursor..open]);
        let Some(relative_close) = template[open + 1..].find('}') else {
            return Err("Template has an opening brace without a closing brace.".to_owned());
        };
        let close = open + 1 + relative_close;
        let variable = &template[open + 1..close];
        let value = match variable {
            "name" => name.to_owned(),
            "ext" => {
                uses_extension = true;
                extension.to_owned()
            }
            "index" => number.to_string(),
            "date" => date.to_owned(),
            "modified" => modified.unwrap_or("").to_owned(),
            "parent" => parent.to_owned(),
            _ if variable.starts_with("counter:") => {
                let width = variable["counter:".len()..]
                    .parse::<usize>()
                    .ok()
                    .filter(|width| *width <= 128)
                    .ok_or_else(|| format!("Invalid template variable {{{variable}}}."))?;
                number_text(number, width)
            }
            _ => return Err(format!("Unsupported template variable {{{variable}}}.")),
        };
        output.push_str(&value);
        cursor = close + 1;
    }
    if template[cursor..].contains('}') {
        return Err("Template has a closing brace without an opening brace.".to_owned());
    }
    output.push_str(&template[cursor..]);
    Ok((output, uses_extension))
}

fn format_date(time: SystemTime) -> Option<String> {
    let seconds = time.duration_since(UNIX_EPOCH).ok()?.as_secs() as libc::time_t;
    let mut local_time = std::mem::MaybeUninit::<libc::tm>::zeroed();
    let result = unsafe { libc::localtime_r(&seconds, local_time.as_mut_ptr()) };
    if result.is_null() {
        return None;
    }
    let local_time = unsafe { local_time.assume_init() };
    Some(format!(
        "{:04}-{:02}-{:02}",
        local_time.tm_year + 1900,
        local_time.tm_mon + 1,
        local_time.tm_mday
    ))
}

fn detect_case_insensitive_directory(directory: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let mut toggled = name.to_owned();
        let Some((index, character)) = name
            .char_indices()
            .find(|(_, character)| character.is_ascii_alphabetic())
        else {
            continue;
        };
        let replacement = if character.is_ascii_lowercase() {
            character.to_ascii_uppercase()
        } else {
            character.to_ascii_lowercase()
        };
        toggled.replace_range(
            index..index + character.len_utf8(),
            &replacement.to_string(),
        );
        let alternate = directory.join(toggled);
        let Ok(current_metadata) = fs::symlink_metadata(directory.join(name)) else {
            continue;
        };
        let Ok(alternate_metadata) = fs::symlink_metadata(alternate) else {
            return false;
        };
        return current_metadata.dev() == alternate_metadata.dev()
            && current_metadata.ino() == alternate_metadata.ino();
    }
    false
}

fn validate_proposed_name(name: &str) -> Result<(), String> {
    validate_file_name(name)?;
    if name.as_bytes().len() > 255 {
        return Err("That name exceeds the filesystem's 255-byte component limit.".to_owned());
    }
    #[cfg(windows)]
    {
        if name.ends_with('.') || name.ends_with(' ') {
            return Err("Windows filenames cannot end with a period or space.".to_owned());
        }
        let device = name
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if matches!(
            device.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        ) {
            return Err("That name is reserved by Windows.".to_owned());
        }
    }
    Ok(())
}

fn changed_portion(old: &str, new: &str) -> (String, String, String, String, String, String) {
    let old_characters = old.char_indices().collect::<Vec<_>>();
    let new_characters = new.char_indices().collect::<Vec<_>>();
    let prefix_count = old_characters
        .iter()
        .zip(&new_characters)
        .take_while(|((_, old_char), (_, new_char))| old_char == new_char)
        .count();
    let suffix_count = old_characters[prefix_count..]
        .iter()
        .rev()
        .zip(new_characters[prefix_count..].iter().rev())
        .take_while(|((_, old_char), (_, new_char))| old_char == new_char)
        .count();
    let old_prefix_end = old_characters
        .get(prefix_count)
        .map(|(index, _)| *index)
        .unwrap_or(old.len());
    let new_prefix_end = new_characters
        .get(prefix_count)
        .map(|(index, _)| *index)
        .unwrap_or(new.len());
    let old_changed_end = old_characters
        .get(old_characters.len().saturating_sub(suffix_count))
        .map(|(index, _)| *index)
        .unwrap_or(old.len());
    let new_changed_end = new_characters
        .get(new_characters.len().saturating_sub(suffix_count))
        .map(|(index, _)| *index)
        .unwrap_or(new.len());
    (
        old[..old_prefix_end].to_owned(),
        old[old_prefix_end..old_changed_end].to_owned(),
        old[old_changed_end..].to_owned(),
        new[..new_prefix_end].to_owned(),
        new[new_prefix_end..new_changed_end].to_owned(),
        new[new_changed_end..].to_owned(),
    )
}

pub fn plan(
    paths: &[PathBuf],
    options: &BatchOptions,
    excluded: &HashSet<PathBuf>,
) -> Vec<BatchRow> {
    plan_inner(paths, options, excluded, None).expect("an uncancellable plan cannot be cancelled")
}

pub fn plan_cancellable(
    paths: &[PathBuf],
    options: &BatchOptions,
    excluded: &HashSet<PathBuf>,
    cancel: &AtomicBool,
) -> Result<Vec<BatchRow>, String> {
    plan_inner(paths, options, excluded, Some(cancel))
}

fn plan_inner(
    paths: &[PathBuf],
    options: &BatchOptions,
    excluded: &HashSet<PathBuf>,
    cancel: Option<&AtomicBool>,
) -> Result<Vec<BatchRow>, String> {
    let prepared = prepare_rules(&options.rules);
    let today = format_date(SystemTime::now()).unwrap_or_default();
    let mut inputs = Vec::with_capacity(paths.len());
    for path in paths {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err("The batch preview was superseded.".to_owned());
        }
        let old_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let sort_name = old_name.to_lowercase();
        let collected = SourceSnapshot::read_with_times(path).ok();
        let missing_source = collected.is_none();
        let (snapshot, modified, created) = match collected {
            Some((snapshot, modified, created)) => (Some(snapshot), modified, created),
            None => (None, None, None),
        };
        inputs.push(PlanInput {
            path: path.clone(),
            old_name,
            sort_name,
            snapshot,
            modified,
            created,
            included: !excluded.contains(path),
            initial_error: if missing_source {
                "The source file no longer exists or cannot be read.".to_owned()
            } else {
                String::new()
            },
        });
    }

    match options.sort {
        SortMode::Selection => {}
        SortMode::Name => inputs.sort_by(|left, right| {
            left.sort_name
                .cmp(&right.sort_name)
                .then_with(|| left.old_name.cmp(&right.old_name))
        }),
        SortMode::Modified => {
            inputs.sort_by(|left, right| compare_optional_time(left.modified, right.modified))
        }
        SortMode::Created => {
            inputs.sort_by(|left, right| compare_optional_time(left.created, right.created))
        }
    }
    if options.descending {
        inputs.reverse();
    }

    let mut rows = Vec::with_capacity(inputs.len());
    let mut sequence_index = 0usize;
    for input in inputs {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err("The batch preview was superseded.".to_owned());
        }
        let index = sequence_index;
        if input.included {
            sequence_index = sequence_index.saturating_add(1);
        }
        let mut error = input.initial_error;
        let counter = options
            .step
            .checked_mul(index as i64)
            .and_then(|offset| options.start.checked_add(offset));
        let template_uses_sequence =
            options.template.contains("{index}") || options.template.contains("{counter:");
        if (options.numbering || template_uses_sequence)
            && (options.start < 0 || options.step <= 0 || counter.is_none())
        {
            error = "Sequence numbers require a non-negative start and positive step.".to_owned();
        }
        let number = counter.unwrap_or(options.start);
        let (original_stem, original_extension) = split_extension(&input.old_name);
        let mut stem = original_stem.to_owned();
        let mut extension = original_extension.trim_start_matches('.').to_owned();

        if error.is_empty() {
            match &prepared {
                Err(reason) => error = reason.clone(),
                Ok(prepared_rules) => {
                    for rule in prepared_rules {
                        let include_extension = match rule {
                            PreparedRule::LiteralReplace {
                                include_extension, ..
                            }
                            | PreparedRule::RegexReplace {
                                include_extension, ..
                            } => *include_extension,
                            _ => false,
                        };
                        if include_extension {
                            let full_name = if extension.is_empty() {
                                stem.clone()
                            } else {
                                format!("{stem}.{extension}")
                            };
                            match apply_text_rule(&full_name, rule) {
                                Ok(result) => {
                                    let (new_stem, new_extension) = split_extension(&result);
                                    stem = new_stem.to_owned();
                                    extension = new_extension.trim_start_matches('.').to_owned();
                                }
                                Err(reason) => {
                                    error = reason;
                                    break;
                                }
                            }
                        } else {
                            match apply_text_rule(&stem, rule) {
                                Ok(result) => stem = result,
                                Err(reason) => {
                                    error = reason;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        if options.extension_mode == ExtensionMode::Change {
            let new_extension = options.extension.trim_start_matches('.');
            if new_extension.is_empty()
                || new_extension.contains('/')
                || new_extension.contains('\0')
            {
                if error.is_empty() {
                    error = "Enter a valid extension without a leading dot.".to_owned();
                }
            } else {
                extension = new_extension.to_owned();
            }
        } else if options.extension_mode == ExtensionMode::Remove {
            extension.clear();
        }

        let mut proposed = if options.template.trim().is_empty() {
            stem.clone()
        } else {
            let modified_date = input.modified.and_then(format_date);
            let parent_name = input
                .path
                .parent()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            match render_template(
                &options.template,
                original_stem,
                original_extension.trim_start_matches('.'),
                number,
                &today,
                modified_date.as_deref(),
                &parent_name,
            ) {
                Ok((rendered, includes_extension)) => {
                    if includes_extension {
                        let (rendered_stem, rendered_extension) = split_extension(&rendered);
                        extension = rendered_extension.trim_start_matches('.').to_owned();
                        rendered_stem.to_owned()
                    } else {
                        rendered
                    }
                }
                Err(reason) => {
                    if error.is_empty() {
                        error = reason;
                    }
                    stem.clone()
                }
            }
        };

        if options.numbering {
            let number = number_text(number, options.padding);
            match options.number_placement {
                NumberPlacement::Prefix => {
                    proposed = format!("{number}{}{proposed}", options.number_separator)
                }
                NumberPlacement::Suffix => {
                    proposed = format!("{proposed}{}{number}", options.number_separator)
                }
                NumberPlacement::Template => {
                    if !options.template.contains("{index}")
                        && !options.template.contains("{counter:")
                        && error.is_empty()
                    {
                        error = "The template must include {index} or {counter:03} for numbering."
                            .to_owned();
                    }
                }
            }
        }
        if !extension.is_empty() {
            proposed.push('.');
            proposed.push_str(&extension);
        }

        if error.is_empty() {
            if let Err(reason) = validate_proposed_name(&proposed) {
                error = reason;
            }
        }
        let unchanged = proposed == input.old_name;
        let (old_prefix, old_changed, old_suffix, new_prefix, new_changed, new_suffix) =
            changed_portion(&input.old_name, &proposed);
        rows.push(BatchRow {
            origin: input.path,
            old_name: input.old_name,
            new_name: proposed,
            error,
            conflict: false,
            included: input.included,
            unchanged,
            result: "Preview".to_owned(),
            old_prefix,
            old_changed,
            old_suffix,
            new_prefix,
            new_changed,
            new_suffix,
            snapshot: input.snapshot,
        });
    }

    let source_identities = rows
        .iter()
        .filter(|row| row.included && row.error.is_empty() && !row.unchanged)
        .filter_map(|row| row.snapshot.as_ref().map(SourceSnapshot::identity))
        .collect::<HashSet<_>>();
    let mut case_insensitive_directories = HashMap::new();
    let mut destination_counts: HashMap<(PathBuf, String), usize> = HashMap::new();
    for row in rows
        .iter()
        .filter(|row| row.included && row.error.is_empty() && !row.unchanged)
    {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err("The batch preview was superseded.".to_owned());
        }
        let parent = row.origin.parent().unwrap_or(Path::new("")).to_path_buf();
        let insensitive = *case_insensitive_directories
            .entry(parent.clone())
            .or_insert_with(|| {
                options.case_insensitive || detect_case_insensitive_directory(&parent)
            });
        let key_name = if insensitive {
            row.new_name.to_lowercase()
        } else {
            row.new_name.clone()
        };
        *destination_counts.entry((parent, key_name)).or_default() += 1;
    }

    for row in &mut rows {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err("The batch preview was superseded.".to_owned());
        }
        if !row.included || !row.error.is_empty() || row.unchanged {
            continue;
        }
        let parent = row.origin.parent().unwrap_or(Path::new(""));
        let destination = parent.join(&row.new_name);
        let insensitive = *case_insensitive_directories
            .entry(parent.to_path_buf())
            .or_insert_with(|| {
                options.case_insensitive || detect_case_insensitive_directory(parent)
            });
        let key_name = if insensitive {
            row.new_name.to_lowercase()
        } else {
            row.new_name.clone()
        };
        let key = (parent.to_path_buf(), key_name);
        if destination_counts.get(&key).copied().unwrap_or(0) > 1 {
            row.conflict = true;
            row.error = "Multiple selected items would have the same destination name.".to_owned();
            continue;
        }
        if let Ok(existing) = SourceSnapshot::read(&destination) {
            if !source_identities.contains(&existing.identity()) {
                row.conflict = true;
                row.error = "A destination with this name already exists.".to_owned();
            }
        }
    }
    Ok(rows)
}

fn compare_optional_time(
    left: Option<SystemTime>,
    right: Option<SystemTime>,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

pub fn has_errors(rows: &[BatchRow]) -> bool {
    rows.iter()
        .any(|row| row.included && (!row.error.is_empty() || row.conflict))
}

#[cfg(test)]
pub fn apply(rows: &[BatchRow]) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    apply_with_progress(rows, None, |_, _, _| {})
}

pub fn apply_with_progress(
    rows: &[BatchRow],
    cancel: Option<&AtomicBool>,
    mut progress: impl FnMut(usize, usize, &Path),
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    if has_errors(rows) {
        return Err("The batch contains invalid or conflicting names.".to_owned());
    }
    let actionable = rows
        .iter()
        .filter(|row| row.included && !row.unchanged)
        .collect::<Vec<_>>();
    for row in &actionable {
        let expected = row
            .snapshot
            .as_ref()
            .ok_or_else(|| format!("Could not verify {}.", row.origin.display()))?;
        let current = SourceSnapshot::read(&row.origin).map_err(|error| {
            format!(
                "{} is unavailable since preview: {error}",
                row.origin.display()
            )
        })?;
        if &current != expected {
            return Err(format!(
                "{} changed since the preview. Recalculate the preview before renaming.",
                row.origin.display()
            ));
        }
    }
    let pairs = actionable
        .iter()
        .map(|row| {
            (
                row.origin.clone(),
                row.origin
                    .parent()
                    .unwrap_or(Path::new(""))
                    .join(&row.new_name),
            )
        })
        .collect::<Vec<_>>();
    ops::rename_many_safely_with_progress(&pairs, cancel, |index, total, path| {
        progress(index, total, path)
    })?;
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| {
                let path = dir.join(name);
                fs::write(&path, name.as_bytes()).unwrap();
                path
            })
            .collect()
    }

    #[test]
    fn applies_ordered_rules_and_extension_policy() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["draft report.txt"]);
        let options = BatchOptions {
            rules: vec![
                RenameRule::Replace {
                    find: "draft".into(),
                    replace: "final".into(),
                    case_sensitive: true,
                    whole_word: false,
                    regex: false,
                    include_extension: false,
                },
                RenameRule::ReplaceSpaces("-".into()),
                RenameRule::ChangeCase(LetterCase::Title),
            ],
            ..Default::default()
        };
        let rows = plan(&files, &options, &HashSet::new());
        assert_eq!(rows[0].new_name, "Final-Report.txt");
    }

    #[test]
    fn supports_unicode_hidden_files_and_character_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &[".résumé"]);
        let options = BatchOptions {
            rules: vec![RenameRule::RemoveRange {
                start: 1,
                length: 1,
            }],
            ..Default::default()
        };
        let rows = plan(&files, &options, &HashSet::new());
        assert_eq!(rows[0].new_name, ".ésumé");
        assert!(!rows[0].unchanged);
    }

    #[test]
    fn numbering_template_padding_and_sorting_are_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["b.txt", "a.txt"]);
        let options = BatchOptions {
            template: "{counter:03}_{name}".into(),
            numbering: true,
            number_placement: NumberPlacement::Template,
            start: 7,
            step: 2,
            sort: SortMode::Name,
            ..Default::default()
        };
        let rows = plan(&files, &options, &HashSet::new());
        assert_eq!(rows[0].old_name, "a.txt");
        assert_eq!(rows[0].new_name, "007_a.txt");
        assert_eq!(rows[1].new_name, "009_b.txt");
    }

    #[test]
    fn reports_bad_regex_and_unsupported_template_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["photo.jpg"]);
        let bad_regex = BatchOptions {
            rules: vec![RenameRule::Replace {
                find: "(".into(),
                replace: "".into(),
                case_sensitive: true,
                whole_word: false,
                regex: true,
                include_extension: false,
            }],
            ..Default::default()
        };
        assert!(plan(&files, &bad_regex, &HashSet::new())[0]
            .error
            .contains("regular expression"));
        let bad_template = BatchOptions {
            template: "{unknown}".into(),
            ..Default::default()
        };
        assert!(plan(&files, &bad_template, &HashSet::new())[0]
            .error
            .contains("Unsupported"));
    }

    #[test]
    fn cancelled_preview_stops_without_touching_files() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["a.txt"]);
        let cancel = AtomicBool::new(true);
        let error = plan_cancellable(&files, &BatchOptions::default(), &HashSet::new(), &cancel)
            .unwrap_err();
        assert!(error.contains("superseded"));
        assert!(files[0].exists());
    }

    #[test]
    fn replaces_all_occurrences_with_case_and_word_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["Cat cat scatter.txt"]);
        let options = BatchOptions {
            rules: vec![RenameRule::Replace {
                find: "cat".into(),
                replace: "dog".into(),
                case_sensitive: false,
                whole_word: true,
                regex: false,
                include_extension: false,
            }],
            ..Default::default()
        };
        assert_eq!(
            plan(&files, &options, &HashSet::new())[0].new_name,
            "dog dog scatter.txt"
        );
    }

    #[test]
    fn literal_replacement_handles_matches_after_multibyte_characters() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["café-été.txt"]);
        let options = BatchOptions {
            rules: vec![RenameRule::Replace {
                find: "été".into(),
                replace: "soir".into(),
                case_sensitive: true,
                whole_word: false,
                regex: false,
                include_extension: false,
            }],
            ..Default::default()
        };
        assert_eq!(
            plan(&files, &options, &HashSet::new())[0].new_name,
            "café-soir.txt"
        );
    }

    #[test]
    fn template_name_and_extension_refer_to_the_original_filename() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["photo.jpg"]);
        let options = BatchOptions {
            rules: vec![RenameRule::Prefix("edited-".into())],
            template: "{name}_copy.{ext}".into(),
            ..Default::default()
        };
        assert_eq!(
            plan(&files, &options, &HashSet::new())[0].new_name,
            "photo_copy.jpg"
        );
    }

    #[test]
    fn supports_regex_and_extension_specific_changes() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["photo12.tar.gz"]);
        let options = BatchOptions {
            rules: vec![RenameRule::Replace {
                find: r"(\d+)".into(),
                replace: "-$1".into(),
                case_sensitive: true,
                whole_word: false,
                regex: true,
                include_extension: false,
            }],
            extension_mode: ExtensionMode::Change,
            extension: "zip".into(),
            ..Default::default()
        };
        assert_eq!(
            plan(&files, &options, &HashSet::new())[0].new_name,
            "photo-12.zip"
        );
    }

    #[test]
    fn detects_case_insensitive_duplicate_destinations() {
        let dir = tempfile::tempdir().unwrap();
        let options = BatchOptions {
            rules: vec![RenameRule::Prefix("item".into())],
            case_insensitive: true,
            ..Default::default()
        };
        let files = create(dir.path(), &["item", "ITEM"]);
        let rows = plan(&files, &options, &HashSet::new());
        assert!(rows.iter().all(|row| row.conflict));
    }

    #[test]
    fn distinguishes_conflicts_unchanged_and_excluded_files() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["a.txt", "b.txt", "zzz.txt"]);
        fs::write(dir.path().join("taken.txt"), b"occupied").unwrap();
        let options = BatchOptions {
            rules: vec![RenameRule::Replace {
                find: "a".into(),
                replace: "taken".into(),
                case_sensitive: true,
                whole_word: false,
                regex: false,
                include_extension: false,
            }],
            ..Default::default()
        };
        let rows = plan(&files, &options, &HashSet::from([files[1].clone()]));
        assert!(rows[0].conflict);
        assert!(!rows[1].included);
        assert!(has_errors(&rows));
        assert!(rows[2].unchanged);
    }

    #[test]
    fn detects_files_changed_or_removed_since_preview() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["a.txt"]);
        let options = BatchOptions {
            rules: vec![RenameRule::Prefix("new-".into())],
            ..Default::default()
        };
        let rows = plan(&files, &options, &HashSet::new());
        fs::write(&files[0], b"changed").unwrap();
        assert!(apply(&rows).unwrap_err().contains("changed since"));
    }

    #[test]
    fn detects_a_source_removed_after_preview() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["a.txt"]);
        let options = BatchOptions {
            rules: vec![RenameRule::Prefix("new-".into())],
            ..Default::default()
        };
        let rows = plan(&files, &options, &HashSet::new());
        fs::remove_file(&files[0]).unwrap();
        assert!(apply(&rows)
            .unwrap_err()
            .contains("unavailable since preview"));
    }

    #[test]
    fn applies_and_undoes_swaps_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let files = create(dir.path(), &["a.txt", "b.txt"]);
        let options = BatchOptions {
            rules: vec![RenameRule::Replace {
                find: "a".into(),
                replace: "b".into(),
                case_sensitive: true,
                whole_word: false,
                regex: false,
                include_extension: false,
            }],
            ..Default::default()
        };
        // A second operation creates the reverse mapping for a real swap.
        let mut rows = plan(&files, &options, &HashSet::new());
        rows[0].new_name = "b.txt".into();
        rows[1].new_name = "a.txt".into();
        rows[0].unchanged = false;
        rows[1].unchanged = false;
        rows[0].conflict = false;
        rows[0].error.clear();
        rows[1].conflict = false;
        rows[1].error.clear();
        rows[0].snapshot = SourceSnapshot::read(&files[0]).ok();
        rows[1].snapshot = SourceSnapshot::read(&files[1]).ok();
        let pairs = apply(&rows).unwrap();
        assert_eq!(fs::read(&files[0]).unwrap(), b"b.txt");
        assert_eq!(fs::read(&files[1]).unwrap(), b"a.txt");
        let redo = ops::UndoOp::RenameMany {
            pairs: pairs.clone(),
        }
        .undo()
        .unwrap()
        .unwrap();
        assert_eq!(fs::read(&files[0]).unwrap(), b"a.txt");
        assert_eq!(fs::read(&files[1]).unwrap(), b"b.txt");
        let undo = redo.apply().unwrap();
        assert_eq!(fs::read(&files[0]).unwrap(), b"b.txt");
        assert_eq!(fs::read(&files[1]).unwrap(), b"a.txt");
        assert!(matches!(undo, ops::UndoOp::RenameMany { .. }));
    }

    #[test]
    fn handles_large_preview_without_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let files = (0..2_000)
            .map(|index| {
                let path = dir.path().join(format!("file-{index}.txt"));
                fs::write(&path, b"x").unwrap();
                path
            })
            .collect::<Vec<_>>();
        let options = BatchOptions {
            rules: vec![RenameRule::Prefix("renamed-".into())],
            ..Default::default()
        };
        let rows = plan(&files, &options, &HashSet::new());
        assert_eq!(rows.len(), files.len());
        assert!(files.iter().all(|path| path.exists()));
    }
}
