//! Real filesystem operations with explicit safety rules.
//!
//! Design rules:
//! - Never follow a symlink while copying, moving, deleting or sizing a tree.
//! - Never silently overwrite: replacement first moves the existing entry to the
//!   trash, which also makes it recoverable by the user.
//! - Cross-filesystem moves copy first, then remove the source, and report a
//!   partial failure instead of pretending the move succeeded.
//! - Archive extraction rejects absolute paths, `..` traversal and escaping
//!   symlinks, and imposes entry-count and size limits.

use std::collections::HashSet;
use std::ffi::CString;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

use crate::paths::{data_dir, format_size, home_dir, path_to_file_url};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    Ask,
    Skip,
    Replace,
    KeepBoth,
}

impl ConflictPolicy {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ask" => Some(Self::Ask),
            "skip" => Some(Self::Skip),
            "replace" => Some(Self::Replace),
            "keep-both" | "keepboth" | "keep_both" => Some(Self::KeepBoth),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransferOutcome {
    pub created: Vec<PathBuf>,
    pub moves: Vec<(PathBuf, PathBuf)>,
    pub skipped: Vec<String>,
    pub failed: Vec<(PathBuf, String)>,
    pub cancelled: bool,
}

/// Validate a single path component entered by the user.
pub fn validate_file_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A name cannot be empty.".to_owned());
    }
    if name == "." || name == ".." {
        return Err("That name is reserved.".to_owned());
    }
    if name.contains('/') || name.contains('\0') {
        return Err("A name cannot contain a slash or a NUL byte.".to_owned());
    }
    if name.chars().count() > 255 {
        return Err("That name is too long.".to_owned());
    }
    Ok(name)
}

/// `photos/album (2)`, `report (3).pdf`, and so on.
pub fn unique_destination(dest_dir: &Path, name: &str) -> PathBuf {
    let candidate = dest_dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_owned());
    let extension = path
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    for counter in 2..10_000 {
        let candidate = dest_dir.join(format!("{stem} ({counter}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dest_dir.join(format!("{stem} ({}){extension}", std::process::id()))
}

fn source_name(path: &Path) -> Result<&std::ffi::OsStr, String> {
    path.file_name()
        .ok_or_else(|| format!("Cannot operate on {} as a file.", path.display()))
}

/// Why a copy or move stopped before finishing. Cancellation is not a failure:
/// the UI reports it separately so it cannot be mistaken for success.
#[derive(Debug, Clone)]
enum CopyFailure {
    Cancelled,
    Failed(String),
}

impl CopyFailure {
    fn into_message(self) -> String {
        match self {
            CopyFailure::Cancelled => "The operation was cancelled.".to_owned(),
            CopyFailure::Failed(message) => message,
        }
    }
}

fn temp_sibling(target: &Path) -> PathBuf {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_owned());
    target.with_file_name(format!(
        ".{name}.filem-part-{}-{counter}",
        std::process::id()
    ))
}

/// Stream one regular file to a sibling temporary file, then link it into place
/// without ever replacing an existing entry. Cancellation removes the partial
/// data, so a cancelled copy leaves no half-written file behind.
fn copy_file_streaming(
    source: &Path,
    target: &Path,
    mode: u32,
    cancel: &AtomicBool,
    bytes: &mut dyn FnMut(u64, u64),
) -> Result<(), CopyFailure> {
    let total = fs::metadata(source).map(|meta| meta.len()).unwrap_or(0);
    let temp = temp_sibling(target);
    let mut input = fs::File::open(source).map_err(|error| {
        CopyFailure::Failed(format!("Could not open {}: {error}", source.display()))
    })?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|error| {
            CopyFailure::Failed(format!("Could not create {}: {error}", temp.display()))
        })?;
    let mut buffer = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let result = loop {
        if cancel.load(Ordering::Relaxed) {
            break Err(CopyFailure::Cancelled);
        }
        match input.read(&mut buffer) {
            Ok(0) => break Ok(()),
            Ok(read) => {
                if let Err(error) = output.write_all(&buffer[..read]) {
                    break Err(CopyFailure::Failed(format!(
                        "Could not write {}: {error}",
                        temp.display()
                    )));
                }
                done = done.saturating_add(read as u64);
                bytes(done, total);
            }
            Err(error) => {
                break Err(CopyFailure::Failed(format!(
                    "Could not read {}: {error}",
                    source.display()
                )))
            }
        }
    };
    if let Err(failure) = result {
        drop(output);
        let _ = fs::remove_file(&temp);
        return Err(failure);
    }
    if let Err(error) = output.sync_all() {
        let _ = fs::remove_file(&temp);
        return Err(CopyFailure::Failed(format!(
            "Could not flush {}: {error}",
            temp.display()
        )));
    }
    drop(output);
    let _ = fs::set_permissions(&temp, fs::Permissions::from_mode(mode & 0o777));
    // A hard link fails when the target already exists, which is exactly the
    // no-silent-overwrite guarantee we want. Filesystems without hard links
    // fall back to a checked rename.
    match fs::hard_link(&temp, target) {
        Ok(()) => {
            let _ = fs::remove_file(&temp);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temp);
            Err(CopyFailure::Failed(format!(
                "{} already exists; nothing was overwritten.",
                target.display()
            )))
        }
        Err(_) => {
            if target.exists() {
                let _ = fs::remove_file(&temp);
                return Err(CopyFailure::Failed(format!(
                    "{} already exists; nothing was overwritten.",
                    target.display()
                )));
            }
            match rename_noreplace(&temp, target) {
                Ok(()) => Ok(()),
                Err(error) => {
                    let _ = fs::remove_file(&temp);
                    Err(CopyFailure::Failed(format!(
                        "Could not create {}: {error}",
                        target.display()
                    )))
                }
            }
        }
    }
}

fn copy_entry(
    source: &Path,
    target: &Path,
    cancel: &AtomicBool,
    bytes: &mut dyn FnMut(u64, u64),
) -> Result<(), CopyFailure> {
    let meta = fs::symlink_metadata(source).map_err(|error| {
        CopyFailure::Failed(format!("Could not inspect {}: {error}", source.display()))
    })?;
    let file_type = meta.file_type();

    if file_type.is_symlink() {
        let link = fs::read_link(source).map_err(|error| {
            CopyFailure::Failed(format!("Could not read link {}: {error}", source.display()))
        })?;
        std::os::unix::fs::symlink(link, target).map_err(|error| {
            CopyFailure::Failed(format!(
                "Could not create link {}: {error}",
                target.display()
            ))
        })?;
        return Ok(());
    }

    if file_type.is_dir() {
        fs::create_dir(target).map_err(|error| {
            CopyFailure::Failed(format!("Could not create {}: {error}", target.display()))
        })?;
        // Best effort: keep the mode, but never copy setuid/setgid bits.
        let mode = meta.permissions().mode() & 0o777;
        let _ = fs::set_permissions(target, fs::Permissions::from_mode(mode));
        let read = fs::read_dir(source).map_err(|error| {
            CopyFailure::Failed(format!("Could not read {}: {error}", source.display()))
        })?;
        for item in read {
            if cancel.load(Ordering::Relaxed) {
                let _ = fs::remove_dir_all(target);
                return Err(CopyFailure::Cancelled);
            }
            let item = item.map_err(|error| {
                CopyFailure::Failed(format!("Could not read {}: {error}", source.display()))
            })?;
            let name = item.file_name();
            // Everything removed here was created by this copy, never by the user.
            if let Err(failure) = copy_entry(&item.path(), &target.join(name), cancel, bytes) {
                let _ = fs::remove_dir_all(target);
                return Err(failure);
            }
        }
        return Ok(());
    }

    if file_type.is_file() {
        return copy_file_streaming(source, target, meta.permissions().mode(), cancel, bytes);
    }

    Err(CopyFailure::Failed(format!(
        "{} is a special file and was not copied.",
        source.display()
    )))
}

/// Copy one entry. Directories are copied recursively, symlinks are recreated
/// as links and never followed, and the destination must not already exist.
pub fn copy_one(source: &Path, target: &Path) -> Result<(), String> {
    let cancel = AtomicBool::new(false);
    copy_entry(source, target, &cancel, &mut |_, _| {}).map_err(CopyFailure::into_message)
}

fn is_cross_device(error: &io::Error) -> bool {
    error.raw_os_error() == Some(18) // EXDEV
}

fn move_entry(
    source: &Path,
    target: &Path,
    cancel: &AtomicBool,
    bytes: &mut dyn FnMut(u64, u64),
) -> Result<(), CopyFailure> {
    match rename_noreplace(source, target) {
        Ok(()) => Ok(()),
        Err(error) if is_cross_device(&error) => {
            copy_entry(source, target, cancel, bytes)?;
            remove_tree(source).map_err(|remove| {
                CopyFailure::Failed(format!(
                    "{source} was copied to {target}, but the original could not be removed: {remove}",
                    source = source.display(),
                    target = target.display()
                ))
            })?;
            Ok(())
        }
        Err(error) => Err(CopyFailure::Failed(format!(
            "Could not move {} to {}: {error}",
            source.display(),
            target.display()
        ))),
    }
}

/// Move one entry, falling back to copy-then-delete across filesystems.
pub fn move_one(source: &Path, target: &Path) -> Result<(), String> {
    let cancel = AtomicBool::new(false);
    move_entry(source, target, &cancel, &mut |_, _| {}).map_err(CopyFailure::into_message)
}

fn rename_noreplace(source: &Path, target: &Path) -> io::Result<()> {
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
    let target = CString::new(target.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "target path contains NUL"))?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn filesystem_identity(path: &Path) -> io::Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

/// Rename a set of paths without replacing existing entries. Sources are
/// staged beside themselves first, allowing swaps and longer rename cycles.
struct StagedRename {
    temporary: PathBuf,
    source: PathBuf,
    destination: PathBuf,
    finalized: bool,
}

pub fn rename_many_safely(pairs: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    rename_many_safely_with_progress(pairs, None, |_, _, _| {})
}

pub fn rename_many_safely_with_progress(
    pairs: &[(PathBuf, PathBuf)],
    cancel: Option<&AtomicBool>,
    mut progress: impl FnMut(usize, usize, &Path),
) -> Result<(), String> {
    let pairs = pairs
        .iter()
        .filter(|(source, destination)| source != destination)
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return Ok(());
    }

    let mut source_identities = HashSet::new();
    let mut destinations = HashSet::new();
    for (source, destination) in &pairs {
        let identity = filesystem_identity(source).map_err(|error| {
            format!(
                "Could not inspect rename source {}: {error}",
                source.display()
            )
        })?;
        source_identities.insert(identity);
        if !destinations.insert(destination.clone()) {
            return Err(format!(
                "The rename batch has multiple operations targeting {}.",
                destination.display()
            ));
        }
    }
    for (_, destination) in &pairs {
        if let Ok(existing_identity) = filesystem_identity(destination) {
            if !source_identities.contains(&existing_identity) {
                return Err(format!(
                    "Refusing to overwrite existing destination {}.",
                    destination.display()
                ));
            }
        }
    }

    static TEMP_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut staged = Vec::with_capacity(pairs.len());
    for (source, destination) in &pairs {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return rollback_staged(
                &mut staged,
                "The batch rename was cancelled before all files were staged.".to_owned(),
            );
        }
        let parent = source.parent().unwrap_or(Path::new("."));
        let mut temporary = None;
        for _ in 0..128 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let candidate = parent.join(format!(".filem-rename-{}-{sequence}", std::process::id()));
            match rename_noreplace(source, &candidate) {
                Ok(()) => {
                    temporary = Some(candidate);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return rollback_staged(
                        &mut staged,
                        format!("Could not stage {} for rename: {error}", source.display()),
                    );
                }
            }
        }
        let Some(temporary) = temporary else {
            return rollback_staged(
                &mut staged,
                format!(
                    "Could not allocate a temporary name for {}.",
                    source.display()
                ),
            );
        };
        staged.push(StagedRename {
            temporary,
            source: (*source).clone(),
            destination: (*destination).clone(),
            finalized: false,
        });
    }

    for index in 0..staged.len() {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return rollback_staged(
                &mut staged,
                "The batch rename was cancelled; restoring original names.".to_owned(),
            );
        }
        if let Err(error) = rename_noreplace(&staged[index].temporary, &staged[index].destination) {
            let reason = format!(
                "Could not rename to {}: {error}",
                staged[index].destination.display()
            );
            return rollback_staged(&mut staged, reason);
        }
        staged[index].finalized = true;
        progress(index + 1, staged.len(), &staged[index].destination);
    }
    Ok(())
}

fn rollback_staged(staged: &mut [StagedRename], reason: String) -> Result<(), String> {
    let mut recovery_failures = Vec::new();
    for entry in staged.iter_mut().rev().filter(|entry| entry.finalized) {
        if let Err(error) = rename_noreplace(&entry.destination, &entry.temporary) {
            recovery_failures.push(format!(
                "Could not move {} to recovery staging {}: {error}",
                entry.destination.display(),
                entry.temporary.display()
            ));
        } else {
            entry.finalized = false;
        }
    }
    for entry in staged.iter().rev() {
        let current = if entry.finalized {
            &entry.destination
        } else {
            &entry.temporary
        };
        if let Err(error) = rename_noreplace(current, &entry.source) {
            recovery_failures.push(format!(
                "Could not restore {} from {}: {error}",
                entry.source.display(),
                current.display()
            ));
        }
    }
    if recovery_failures.is_empty() {
        Err(reason)
    } else {
        Err(format!(
            "{reason} Recovery was incomplete: {}",
            recovery_failures.join("; ")
        ))
    }
}

/// Delete an entry permanently. Symlinks are removed, never traversed.
pub fn remove_tree(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    let result = if meta.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|error| format!("Could not delete {}: {error}", path.display()))
}

fn transfer_conflict(
    source: &Path,
    target: &Path,
    policy: ConflictPolicy,
) -> Result<Option<PathBuf>, String> {
    if target == source {
        return Ok(None);
    }
    if !target.exists() {
        return Ok(Some(target.to_path_buf()));
    }
    match policy {
        ConflictPolicy::Ask | ConflictPolicy::Skip => Ok(None),
        ConflictPolicy::KeepBoth => Ok(Some(unique_destination(
            target.parent().unwrap_or(Path::new("/")),
            &target
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".to_owned()),
        ))),
        ConflictPolicy::Replace => {
            trash(target)?;
            Ok(Some(target.to_path_buf()))
        }
    }
}

/// Transfer a selection into a destination directory.
///
/// `progress` is called with (item index, item count, current name, bytes done,
/// bytes in the current file). Byte progress is only meaningful while a single
/// large file is being copied; the UI combines both counters. The caller is
/// responsible for performing this on a worker thread.
pub fn transfer(
    sources: &[PathBuf],
    dest_dir: &Path,
    cut: bool,
    policy: ConflictPolicy,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize, &str, u64, u64),
) -> TransferOutcome {
    let mut outcome = TransferOutcome {
        created: Vec::new(),
        moves: Vec::new(),
        skipped: Vec::new(),
        failed: Vec::new(),
        cancelled: false,
    };
    let total = sources.len();

    if !dest_dir.is_dir() {
        outcome.failed.push((
            dest_dir.to_path_buf(),
            "The destination is not a folder.".to_owned(),
        ));
        return outcome;
    }

    for (index, source) in sources.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            break;
        }
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.display().to_string());
        progress(index, total, &name, 0, 0);

        let Ok(meta) = fs::symlink_metadata(source) else {
            outcome.failed.push((
                source.clone(),
                format!("{} no longer exists.", source.display()),
            ));
            continue;
        };

        // Refuse to copy or move a folder into itself or a descendant.
        if meta.is_dir() {
            let source_clean = crate::paths::clean_lexically(source);
            let dest_clean = crate::paths::clean_lexically(dest_dir);
            if dest_clean == source_clean || dest_clean.starts_with(&source_clean) {
                outcome.failed.push((
                    source.clone(),
                    "Cannot copy or move a folder inside itself.".to_owned(),
                ));
                continue;
            }
        }

        let name_os = match source_name(source) {
            Ok(name) => name.to_owned(),
            Err(error) => {
                outcome.failed.push((source.clone(), error));
                continue;
            }
        };
        let target = dest_dir.join(&name_os);

        if target == *source {
            outcome
                .skipped
                .push(format!("{name}: already in this folder"));
            continue;
        }

        let target = match transfer_conflict(source, &target, policy) {
            Ok(Some(target)) => target,
            Ok(None) => {
                if target.exists() {
                    outcome.skipped.push(name);
                }
                continue;
            }
            Err(error) => {
                outcome.failed.push((source.clone(), error));
                continue;
            }
        };

        let mut on_bytes = |done: u64, file_total: u64| {
            progress(index, total, &name, done, file_total);
        };
        let result = if cut {
            move_entry(source, &target, cancel, &mut on_bytes)
        } else {
            copy_entry(source, &target, cancel, &mut on_bytes)
        };

        match result {
            Ok(()) => {
                if cut {
                    outcome.moves.push((source.clone(), target));
                } else {
                    outcome.created.push(target);
                }
            }
            Err(CopyFailure::Cancelled) => {
                outcome.cancelled = true;
                break;
            }
            Err(CopyFailure::Failed(error)) => outcome.failed.push((source.clone(), error)),
        }
    }

    progress(total, total, "", 0, 0);
    outcome
}

#[derive(Debug, Clone)]
pub struct TrashItem {
    pub file: PathBuf,
    pub info: PathBuf,
    pub original: PathBuf,
}

pub fn home_trash() -> PathBuf {
    data_dir().join("Trash")
}

fn iso_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    // Civil date from days since the Unix epoch (Howard Hinnant's algorithm),
    // avoiding a date-time dependency for a single timestamp.
    let days = (seconds / 86_400) as i64;
    let rem = seconds % 86_400;
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}")
}

fn mount_point_for(path: &Path) -> PathBuf {
    let device = match fs::metadata(path) {
        Ok(meta) => meta.dev(),
        Err(_) => return PathBuf::from("/"),
    };
    let mut current = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new("/")).to_path_buf()
    };
    loop {
        let Some(parent) = current.parent() else {
            return current;
        };
        match fs::metadata(parent) {
            Ok(meta) if meta.dev() == device => current = parent.to_path_buf(),
            _ => return current,
        }
    }
}

fn trash_directory_for(path: &Path) -> Result<PathBuf, String> {
    let home = home_dir();
    let same_home = fs::metadata(path)
        .ok()
        .zip(fs::metadata(&home).ok())
        .map(|(a, b)| a.dev() == b.dev())
        .unwrap_or(false);
    if same_home {
        return Ok(home_trash());
    }

    let mount = mount_point_for(path);
    let uid = unsafe { libc::geteuid() };
    let shared = mount.join(".Trash");
    if let Ok(meta) = fs::symlink_metadata(&shared) {
        let sticky = meta.permissions().mode() & 0o1000 != 0;
        let owned_by_root_or_user = meta.uid() == 0 || meta.uid() == uid;
        if meta.is_dir() && sticky && owned_by_root_or_user {
            let dir = shared.join(uid.to_string());
            if dir.is_dir() {
                return Ok(dir);
            }
        }
    }
    Ok(mount.join(format!(".Trash-{uid}")))
}

fn trash_unique_name(files_dir: &Path, name: &str) -> (String, PathBuf) {
    let mut candidate = name.to_owned();
    if !files_dir.join(&candidate).exists()
        && !files_dir.join(format!("{candidate}.trashinfo")).exists()
    {
        return (candidate.clone(), files_dir.join(&candidate));
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_owned());
    let extension = path
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    for counter in 2..100_000 {
        candidate = format!("{stem}.{counter}{extension}");
        if !files_dir.join(&candidate).exists()
            && !files_dir.join(format!("{candidate}.trashinfo")).exists()
        {
            break;
        }
    }
    let file = files_dir.join(&candidate);
    (candidate, file)
}

/// Move an entry to the freedesktop trash. The trash directory is created on
/// demand. The `.trashinfo` record is written first so a crash leaves a
/// recoverable item rather than an orphan where possible.
pub fn trash(path: &Path) -> Result<TrashItem, String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    if meta.is_dir() && path.parent().is_none() {
        return Err("The filesystem root cannot be moved to the trash.".to_owned());
    }
    let original = crate::paths::clean_lexically(
        &fs::canonicalize(path.parent().unwrap_or(Path::new("/")))
            .map(|parent| parent.join(path.file_name().unwrap_or_default()))
            .unwrap_or_else(|_| path.to_path_buf()),
    );
    let base = trash_directory_for(path)?;
    let files_dir = base.join("files");
    let info_dir = base.join("info");
    fs::create_dir_all(&files_dir)
        .map_err(|error| format!("Could not create trash at {}: {error}", files_dir.display()))?;
    fs::create_dir_all(&info_dir).map_err(|error| {
        format!(
            "Could not create trash info at {}: {error}",
            info_dir.display()
        )
    })?;

    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| "That entry cannot be trashed.".to_owned())?;
    let (unique, file) = trash_unique_name(&files_dir, &name);
    let info = info_dir.join(format!("{unique}.trashinfo"));
    let original_lossy = original.to_string_lossy();
    let encoded = utf8_percent_encode(&original_lossy, URL_PATH_SET);
    let record = format!("[Trash Info]\nPath={encoded}\nDeletionDate={}\n", iso_now());
    fs::write(&info, record)
        .map_err(|error| format!("Could not write trash info {}: {error}", info.display()))?;

    if let Err(error) = move_one(path, &file) {
        let _ = fs::remove_file(&info);
        return Err(format!(
            "Could not move {} to the trash: {error}",
            path.display()
        ));
    }

    Ok(TrashItem {
        file,
        info,
        original,
    })
}

fn parse_trashinfo(text: &str) -> Option<PathBuf> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Path=") {
            let decoded = percent_decode(value)?;
            let path = PathBuf::from(decoded);
            if path.is_absolute() {
                return Some(crate::paths::clean_lexically(&path));
            }
        }
    }
    None
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            let byte = u8::from_str_radix(hex, 16).ok()?;
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Restore a trashed entry. Never overwrites: if the original name is taken,
/// a numbered name is used and reported through the returned path.
pub fn restore_trashed(item: &TrashItem) -> Result<PathBuf, String> {
    let parent = item
        .original
        .parent()
        .ok_or_else(|| "The original location is not valid any more.".to_owned())?;
    if !parent.is_dir() {
        return Err(format!(
            "The original folder {} no longer exists.",
            parent.display()
        ));
    }
    let target = if item.original.exists() {
        unique_destination(
            parent,
            &item
                .original
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "restored".to_owned()),
        )
    } else {
        item.original.clone()
    };
    move_one(&item.file, &target)?;
    let _ = fs::remove_file(&item.info);
    Ok(target)
}

pub fn trashed_items_in(dir: &Path) -> Vec<TrashItem> {
    let files = dir.join("files");
    let info = dir.join("info");
    let mut items = Vec::new();
    let Ok(read) = fs::read_dir(&files) else {
        return items;
    };
    for entry in read.flatten() {
        let name = entry.file_name();
        let info_path = info.join(format!("{}.trashinfo", name.to_string_lossy()));
        let original = fs::read_to_string(&info_path)
            .ok()
            .and_then(|text| parse_trashinfo(&text))
            .unwrap_or_else(|| PathBuf::from("/unknown"));
        items.push(TrashItem {
            file: entry.path(),
            info: info_path,
            original,
        });
    }
    items.sort_by(|a, b| a.file.cmp(&b.file));
    items
}

pub fn empty_trash(dir: &Path, cancel: &AtomicBool) -> (usize, Vec<(PathBuf, String)>) {
    let mut removed = 0usize;
    let mut failed = Vec::new();
    for item in trashed_items_in(dir) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match remove_tree(&item.file) {
            Ok(()) => {
                removed += 1;
                let _ = fs::remove_file(&item.info);
            }
            Err(error) => failed.push((item.file, error)),
        }
    }
    (removed, failed)
}

/// Restore every trashed item that can be restored.
pub fn restore_all(dir: &Path) -> (Vec<PathBuf>, Vec<(PathBuf, String)>) {
    let mut restored = Vec::new();
    let mut failed = Vec::new();
    for item in trashed_items_in(dir) {
        match restore_trashed(&item) {
            Ok(path) => restored.push(path),
            Err(error) => failed.push((item.file, error)),
        }
    }
    (restored, failed)
}

#[derive(Debug, Clone)]
pub enum UndoOp {
    Rename { from: PathBuf, to: PathBuf },
    Create { path: PathBuf, dir: bool },
    Copy { created: Vec<PathBuf> },
    Move { pairs: Vec<(PathBuf, PathBuf)> },
    Trash { items: Vec<TrashItem> },
    RenameMany { pairs: Vec<(PathBuf, PathBuf)> },
}

#[derive(Debug, Clone)]
pub enum RedoOp {
    Rename { from: PathBuf, to: PathBuf },
    Create { path: PathBuf, dir: bool },
    Move { pairs: Vec<(PathBuf, PathBuf)> },
    Trash { items: Vec<TrashItem> },
    RenameMany { pairs: Vec<(PathBuf, PathBuf)> },
}

impl UndoOp {
    pub fn description(&self) -> String {
        match self {
            UndoOp::Rename { from, .. } => format!(
                "Rename {}",
                from.file_name().unwrap_or_default().to_string_lossy()
            ),
            UndoOp::Create { path, dir } => format!(
                "{} {}",
                if *dir { "New folder" } else { "New file" },
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            UndoOp::Copy { created } => format!("Copy of {} item(s)", created.len()),
            UndoOp::Move { pairs } => format!("Move of {} item(s)", pairs.len()),
            UndoOp::Trash { items } => format!("Trash of {} item(s)", items.len()),
            UndoOp::RenameMany { pairs } => format!("Rename of {} item(s)", pairs.len()),
        }
    }

    /// Invert the operation. Returns what would be needed to redo it, when a
    /// faithful redo is possible. Copy has no redo because its sources may no
    /// longer exist.
    pub fn undo(self) -> Result<Option<RedoOp>, String> {
        match self {
            UndoOp::Rename { from, to } => {
                rename_noreplace(&to, &from).map_err(|error| {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        format!("Cannot undo the rename: {} already exists.", from.display())
                    } else {
                        format!("Could not rename {} back: {error}", to.display())
                    }
                })?;
                Ok(Some(RedoOp::Rename {
                    from: from.clone(),
                    to,
                }))
            }
            UndoOp::Create { path, dir } => {
                remove_tree(&path)?;
                Ok(Some(RedoOp::Create { path, dir }))
            }
            UndoOp::Copy { created } => {
                let mut failures = Vec::new();
                for path in created {
                    if let Err(error) = remove_tree(&path) {
                        failures.push(error);
                    }
                }
                if failures.is_empty() {
                    Ok(None)
                } else {
                    Err(failures.join("; "))
                }
            }
            UndoOp::Move { pairs } => {
                let mut failures = Vec::new();
                let mut redo = Vec::new();
                for (from, to) in pairs.into_iter().rev() {
                    if from.exists() {
                        failures.push(format!(
                            "{} already exists again, so the move was not undone",
                            from.display()
                        ));
                        continue;
                    }
                    match move_one(&to, &from) {
                        Ok(()) => redo.push((from, to)),
                        Err(error) => failures.push(error),
                    }
                }
                if failures.is_empty() {
                    Ok(Some(RedoOp::Move { pairs: redo }))
                } else {
                    Err(failures.join("; "))
                }
            }
            UndoOp::Trash { items } => {
                let mut failures = Vec::new();
                let mut restored = Vec::new();
                for item in &items {
                    match restore_trashed(item) {
                        Ok(path) => restored.push(path),
                        Err(error) => failures.push(error),
                    }
                }
                if failures.is_empty() {
                    Ok(Some(RedoOp::Trash { items }))
                } else {
                    Err(failures.join("; "))
                }
            }
            UndoOp::RenameMany { pairs } => {
                let inverse = pairs
                    .iter()
                    .rev()
                    .map(|(from, to)| (to.clone(), from.clone()))
                    .collect::<Vec<_>>();
                rename_many_safely(&inverse)?;
                Ok(Some(RedoOp::RenameMany { pairs }))
            }
        }
    }
}

impl RedoOp {
    pub fn description(&self) -> String {
        match self {
            RedoOp::Rename { from, .. } => format!(
                "Rename {}",
                from.file_name().unwrap_or_default().to_string_lossy()
            ),
            RedoOp::Create { path, dir } => format!(
                "{} {}",
                if *dir { "New folder" } else { "New file" },
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            RedoOp::Move { pairs } => format!("Move of {} item(s)", pairs.len()),
            RedoOp::Trash { items } => format!("Trash of {} item(s)", items.len()),
            RedoOp::RenameMany { pairs } => format!("Rename of {} item(s)", pairs.len()),
        }
    }

    pub fn apply(self) -> Result<UndoOp, String> {
        match self {
            RedoOp::Rename { from, to } => {
                rename_noreplace(&from, &to).map_err(|error| {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        format!("Cannot redo the rename: {} already exists.", to.display())
                    } else {
                        format!("Could not rename {} again: {error}", from.display())
                    }
                })?;
                Ok(UndoOp::Rename { from, to })
            }
            RedoOp::Create { path, dir } => {
                if path.exists() {
                    return Err(format!("{} already exists.", path.display()));
                }
                if dir {
                    fs::create_dir(&path)
                } else {
                    fs::File::create_new(&path).map(|_| ())
                }
                .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
                Ok(UndoOp::Create { path, dir })
            }
            RedoOp::Move { pairs } => {
                let mut moved = Vec::new();
                for (from, to) in pairs {
                    move_one(&from, &to)?;
                    moved.push((from, to));
                }
                Ok(UndoOp::Move { pairs: moved })
            }
            RedoOp::Trash { items } => {
                let mut redone = Vec::new();
                for item in items {
                    let current = if item.file.exists() {
                        item.file
                    } else {
                        item.original.clone()
                    };
                    match trash(&current) {
                        Ok(item) => redone.push(item),
                        Err(error) => return Err(error),
                    }
                }
                Ok(UndoOp::Trash { items: redone })
            }
            RedoOp::RenameMany { pairs } => {
                rename_many_safely(&pairs)?;
                Ok(UndoOp::RenameMany { pairs })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZstd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveCreateFormat {
    Zip,
    Tar,
    TarGz,
    SevenZip,
}

pub fn archive_format(path: &Path) -> Option<ArchiveFormat> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some(ArchiveFormat::TarGz)
    } else if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") || name.ends_with(".tbz") {
        Some(ArchiveFormat::TarBz2)
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        Some(ArchiveFormat::TarXz)
    } else if name.ends_with(".tar.zst") || name.ends_with(".tzst") {
        Some(ArchiveFormat::TarZstd)
    } else if name.ends_with(".tar") {
        Some(ArchiveFormat::Tar)
    } else if name.ends_with(".zip") {
        Some(ArchiveFormat::Zip)
    } else {
        None
    }
}

struct DecoderChild {
    child: Child,
    finished: bool,
}

impl Drop for DecoderChild {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn tar_reader(
    path: &Path,
    format: ArchiveFormat,
) -> Result<(Box<dyn Read>, Option<DecoderChild>), String> {
    match format {
        ArchiveFormat::Tar => fs::File::open(path)
            .map(|file| (Box::new(file) as Box<dyn Read>, None))
            .map_err(|error| format!("Could not open {}: {error}", path.display())),
        ArchiveFormat::TarGz => fs::File::open(path)
            .map(|file| {
                (
                    Box::new(flate2::read::GzDecoder::new(file)) as Box<dyn Read>,
                    None,
                )
            })
            .map_err(|error| format!("Could not open {}: {error}", path.display())),
        ArchiveFormat::TarBz2 | ArchiveFormat::TarXz | ArchiveFormat::TarZstd => {
            let (program, label) = match format {
                ArchiveFormat::TarBz2 => ("bzip2", "bzip2"),
                ArchiveFormat::TarXz => ("xz", "xz"),
                ArchiveFormat::TarZstd => ("zstd", "zstd"),
                _ => unreachable!(),
            };
            let mut child = Command::new(program)
                .args(["-dc", "--"])
                .arg(path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| {
                    format!(
                        "Could not start the {label} decoder required for {}: {error}",
                        path.display()
                    )
                })?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| format!("Could not read the {label} decoder output."))?;
            Ok((
                Box::new(stdout),
                Some(DecoderChild {
                    child,
                    finished: false,
                }),
            ))
        }
        ArchiveFormat::Zip => Err("ZIP is not a TAR format.".to_owned()),
    }
}

fn finish_decoder(child: Option<DecoderChild>) -> Result<(), String> {
    if let Some(mut child) = child {
        let status = child
            .child
            .wait()
            .map_err(|error| format!("Could not finish archive decoder: {error}"))?;
        child.finished = true;
        if !status.success() {
            return Err("Archive compression decoder reported invalid or corrupt data.".to_owned());
        }
    }
    Ok(())
}

pub fn archive_list(path: &Path, limit: usize) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    match archive_format(path) {
        Some(ArchiveFormat::Zip) => {
            let file = fs::File::open(path)
                .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
            let mut archive = zip::ZipArchive::new(io::BufReader::new(file))
                .map_err(|error| format!("Could not read ZIP: {error}"))?;
            for index in 0..archive.len().min(limit) {
                let entry = archive
                    .by_index(index)
                    .map_err(|error| format!("Could not read ZIP entry: {error}"))?;
                result.push(entry.name().to_owned());
            }
        }
        Some(
            format @ (ArchiveFormat::Tar
            | ArchiveFormat::TarGz
            | ArchiveFormat::TarBz2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarZstd),
        ) => {
            let (reader, child) = tar_reader(path, format)?;
            let mut archive = tar::Archive::new(reader);
            for entry in archive
                .entries()
                .map_err(|error| format!("Could not read TAR: {error}"))?
                .take(limit)
            {
                let entry = entry.map_err(|error| format!("Could not read TAR entry: {error}"))?;
                let path = entry
                    .path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "?".to_owned());
                result.push(path);
            }
            if result.len() < limit {
                let reader = archive.into_inner();
                drop(reader);
                finish_decoder(child)?;
            }
        }
        None => return Err("This format cannot be listed.".to_owned()),
    }
    Ok(result)
}

const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_LINK_BYTES: u64 = 4096;

/// Reject absolute paths and any `..` component before touching the filesystem.
pub fn safe_relative_path(value: &Path) -> Result<PathBuf, String> {
    if value.is_absolute() {
        return Err(format!("Archive entry {} is absolute.", value.display()));
    }
    let mut out = PathBuf::new();
    for component in value.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {
                return Err(format!("Archive entry {} contains '..'.", value.display()))
            }
            _ => {
                return Err(format!(
                    "Archive entry {} would escape the destination.",
                    value.display()
                ))
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err("Archive contains an empty entry name.".to_owned());
    }
    Ok(out)
}

fn link_stays_inside(dest: &Path, entry_dir: &Path, target: &Path) -> bool {
    if target.is_absolute() {
        return false;
    }
    let joined = crate::paths::clean_lexically(&entry_dir.join(target));
    joined.starts_with(dest)
}

fn ensure_safe_parent(dest: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut current = dest.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err("Invalid extraction path.".to_owned());
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(_) => {
                let resolved = fs::canonicalize(&current).map_err(|error| {
                    format!(
                        "Could not resolve extraction folder {}: {error}",
                        current.display()
                    )
                })?;
                if !resolved.starts_with(dest) || !resolved.is_dir() {
                    return Err(format!(
                        "Extraction path {} is not a safe folder inside the destination.",
                        current.display()
                    ));
                }
                current = resolved;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| {
                    format!(
                        "Could not create extraction folder {}: {error}",
                        current.display()
                    )
                })?;
            }
            Err(error) => {
                return Err(format!(
                    "Could not inspect extraction folder {}: {error}",
                    current.display()
                ))
            }
        }
    }
    Ok(current)
}

fn create_archive_directory(dest: &Path, relative: &Path) -> Result<(), String> {
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let parent = ensure_safe_parent(dest, parent)?;
    let target = parent.join(relative.file_name().unwrap_or_default());
    match fs::symlink_metadata(&target) {
        Ok(_) => {
            let resolved = fs::canonicalize(&target).map_err(|error| {
                format!(
                    "Could not resolve extraction folder {}: {error}",
                    target.display()
                )
            })?;
            if !resolved.starts_with(dest) || !resolved.is_dir() {
                return Err(format!(
                    "Extraction folder {} is not a safe directory.",
                    target.display()
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(&target)
            .map_err(|error| format!("Could not create {}: {error}", target.display())),
        Err(error) => Err(format!("Could not inspect {}: {error}", target.display())),
    }
}

fn write_file_checked(
    dest: &Path,
    relative: &Path,
    reader: &mut dyn Read,
    mode: Option<u32>,
    cancel: &AtomicBool,
    total_bytes: &mut u64,
    max_total_bytes: u64,
    max_file_bytes: u64,
) -> Result<u64, String> {
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let parent = ensure_safe_parent(dest, parent)?;
    let path = parent.join(relative.file_name().unwrap_or_default());
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                format!(
                    "{} already exists; nothing was overwritten.",
                    path.display()
                )
            } else {
                format!("Could not create {}: {error}", path.display())
            }
        })?;
    let remaining = max_total_bytes.saturating_sub(*total_bytes);
    let limit = max_file_bytes.min(remaining);
    let read_limit = limit.saturating_add(1);
    let mut buffer = vec![0u8; 256 * 1024];
    let mut copied = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(
                "Extraction cancelled; some files may already have been extracted.".to_owned(),
            );
        }
        let request = read_limit.saturating_sub(copied).min(buffer.len() as u64) as usize;
        let read = match reader.read(&mut buffer[..request]) {
            Ok(read) => read,
            Err(error) => {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(format!("Could not extract {}: {error}", path.display()));
            }
        };
        if read == 0 {
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(
                "Extraction cancelled; some files may already have been extracted.".to_owned(),
            );
        }
        copied = copied.saturating_add(read as u64);
        if copied > limit {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(if remaining < max_file_bytes {
                "Archive expands beyond the total extraction size limit.".to_owned()
            } else {
                "An archive file exceeds the per-file extraction size limit.".to_owned()
            });
        }
        if let Err(error) = file.write_all(&buffer[..read]) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(format!("Could not extract {}: {error}", path.display()));
        }
    }
    *total_bytes = (*total_bytes).saturating_add(copied);
    if let Some(mode) = mode {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777));
    }
    Ok(copied)
}

/// Extract an archive into an existing destination, rejecting traversal,
/// escaping symlinks, unsafe destination symlinks, and oversized payloads.
pub fn extract_archive(
    archive_path: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize, &str),
) -> Result<usize, String> {
    let format =
        archive_format(archive_path).ok_or_else(|| "Unsupported archive format.".to_owned())?;
    if !dest.is_dir() {
        return Err(format!(
            "Extraction destination is not an existing folder: {}",
            dest.display()
        ));
    }
    let dest = fs::canonicalize(dest)
        .map_err(|error| format!("Could not resolve {}: {error}", dest.display()))?;
    let mut extracted = 0usize;
    let mut total_bytes = 0u64;

    let cancelled = match format {
        ArchiveFormat::Zip => {
            let file = fs::File::open(archive_path)
                .map_err(|error| format!("Could not open archive: {error}"))?;
            let mut archive = zip::ZipArchive::new(io::BufReader::new(file))
                .map_err(|error| format!("Could not read ZIP: {error}"))?;
            let count = archive.len();
            if count > MAX_ARCHIVE_ENTRIES {
                return Err("Archive has too many entries.".to_owned());
            }
            let mut cancelled = false;
            for index in 0..count {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }
                let mut entry = archive
                    .by_index(index)
                    .map_err(|error| format!("Could not read ZIP entry: {error}"))?;
                let enclosed = entry
                    .enclosed_name()
                    .ok_or_else(|| format!("Unsafe archive entry: {}", entry.name()))?;
                let relative = safe_relative_path(&enclosed)?;
                progress(index, count, &relative.to_string_lossy());
                if entry.is_dir() {
                    create_archive_directory(&dest, &relative)?;
                    continue;
                }
                let mode = entry.unix_mode();
                let is_symlink = mode
                    .map(|mode| mode & 0o170000 == 0o120000)
                    .unwrap_or(false);
                if is_symlink {
                    let mut link = String::new();
                    entry
                        .take(MAX_ARCHIVE_LINK_BYTES + 1)
                        .read_to_string(&mut link)
                        .map_err(|error| format!("Could not read link entry: {error}"))?;
                    if link.len() as u64 > MAX_ARCHIVE_LINK_BYTES {
                        return Err("Archive link target exceeds the safety limit.".to_owned());
                    }
                    let link = PathBuf::from(link);
                    let parent = ensure_safe_parent(
                        &dest,
                        relative.parent().unwrap_or_else(|| Path::new("")),
                    )?;
                    let target = parent.join(relative.file_name().unwrap_or_default());
                    if !link_stays_inside(&dest, &parent, &link) {
                        return Err(format!(
                            "Archive link {} points outside the destination.",
                            target.display()
                        ));
                    }
                    if fs::symlink_metadata(&target).is_ok() {
                        return Err(format!("{} already exists.", target.display()));
                    }
                    std::os::unix::fs::symlink(&link, &target).map_err(|error| {
                        format!("Could not create link {}: {error}", target.display())
                    })?;
                    extracted += 1;
                    continue;
                }
                let declared = entry.size();
                if declared > MAX_ARCHIVE_FILE_BYTES
                    || declared > MAX_ARCHIVE_BYTES.saturating_sub(total_bytes)
                {
                    return Err("Archive expands beyond the extraction size limit.".to_owned());
                }
                write_file_checked(
                    &dest,
                    &relative,
                    &mut entry,
                    mode,
                    cancel,
                    &mut total_bytes,
                    MAX_ARCHIVE_BYTES,
                    MAX_ARCHIVE_FILE_BYTES,
                )?;
                extracted += 1;
            }
            cancelled
        }
        ArchiveFormat::Tar
        | ArchiveFormat::TarGz
        | ArchiveFormat::TarBz2
        | ArchiveFormat::TarXz
        | ArchiveFormat::TarZstd => {
            let (reader, child) = tar_reader(archive_path, format)?;
            let mut archive = tar::Archive::new(reader);
            let mut cancelled = false;
            {
                let entries = archive
                    .entries()
                    .map_err(|error| format!("Could not read TAR: {error}"))?;
                for (index, entry) in entries.enumerate() {
                    if index >= MAX_ARCHIVE_ENTRIES {
                        return Err("Archive has too many entries.".to_owned());
                    }
                    if cancel.load(Ordering::Relaxed) {
                        cancelled = true;
                        break;
                    }
                    let mut entry =
                        entry.map_err(|error| format!("Could not read TAR: {error}"))?;
                    let path = entry
                        .path()
                        .map_err(|error| format!("Invalid TAR path: {error}"))?
                        .into_owned();
                    let relative = safe_relative_path(&path)?;
                    progress(
                        index,
                        MAX_ARCHIVE_ENTRIES.min(10_000),
                        &relative.to_string_lossy(),
                    );
                    let entry_type = entry.header().entry_type();
                    if entry_type.is_dir() {
                        create_archive_directory(&dest, &relative)?;
                        continue;
                    }
                    if entry_type.is_symlink() || entry_type.is_hard_link() {
                        let link = entry
                            .link_name()
                            .map_err(|error| format!("Invalid link in archive: {error}"))?
                            .map(|path| path.into_owned())
                            .ok_or_else(|| "Link entry without a target.".to_owned())?;
                        let parent = ensure_safe_parent(
                            &dest,
                            relative.parent().unwrap_or_else(|| Path::new("")),
                        )?;
                        let target = parent.join(relative.file_name().unwrap_or_default());
                        if !link_stays_inside(&dest, &parent, &link) {
                            return Err(format!(
                                "Archive link {} points outside the destination.",
                                target.display()
                            ));
                        }
                        if fs::symlink_metadata(&target).is_ok() {
                            return Err(format!("{} already exists.", target.display()));
                        }
                        if entry_type.is_symlink() {
                            std::os::unix::fs::symlink(&link, &target).map_err(|error| {
                                format!("Could not create link {}: {error}", target.display())
                            })?;
                        } else {
                            let source = dest.join(safe_relative_path(&link)?);
                            let source_parent = source.parent().unwrap_or(&dest);
                            ensure_safe_parent(
                                &dest,
                                source_parent.strip_prefix(&dest).unwrap_or(source_parent),
                            )?;
                            if !source.starts_with(&dest) {
                                return Err("Hard link points outside the destination.".to_owned());
                            }
                            fs::hard_link(&source, &target).map_err(|error| {
                                format!("Could not create hard link {}: {error}", target.display())
                            })?;
                        }
                        extracted += 1;
                        continue;
                    }
                    if !entry_type.is_file() {
                        return Err("Archive contains an unsupported special file.".to_owned());
                    }
                    let size = entry.header().size().unwrap_or(0);
                    if size > MAX_ARCHIVE_FILE_BYTES
                        || size > MAX_ARCHIVE_BYTES.saturating_sub(total_bytes)
                    {
                        return Err("Archive expands beyond the extraction size limit.".to_owned());
                    }
                    let mode = entry.header().mode().ok();
                    write_file_checked(
                        &dest,
                        &relative,
                        &mut entry,
                        mode,
                        cancel,
                        &mut total_bytes,
                        MAX_ARCHIVE_BYTES,
                        MAX_ARCHIVE_FILE_BYTES,
                    )?;
                    extracted += 1;
                }
            }
            let reader = archive.into_inner();
            drop(reader);
            finish_decoder(child)?;
            cancelled
        }
    };

    if cancelled {
        return Err("Extraction cancelled; some files may already have been extracted.".to_owned());
    }
    Ok(extracted)
}

/// Create a supported archive next to the current selection.
pub fn compress_archive(
    sources: &[PathBuf],
    dest_dir: &Path,
    cancel: &AtomicBool,
    format: ArchiveCreateFormat,
    password: Option<&str>,
) -> Result<PathBuf, String> {
    if sources.is_empty() {
        return Err("Nothing was selected to compress.".to_owned());
    }
    let base = if sources.len() == 1 {
        sources[0]
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "archive".to_owned())
    } else {
        "selection".to_owned()
    };
    let extension = match format {
        ArchiveCreateFormat::Zip => "zip",
        ArchiveCreateFormat::Tar => "tar",
        ArchiveCreateFormat::TarGz => "tar.gz",
        ArchiveCreateFormat::SevenZip => "7z",
    };
    if password.is_some_and(str::is_empty) {
        return Err("Archive password cannot be empty.".to_owned());
    }
    if password.is_some()
        && !matches!(
            format,
            ArchiveCreateFormat::Zip | ArchiveCreateFormat::SevenZip
        )
    {
        return Err("Password encryption is supported for ZIP and 7z archives.".to_owned());
    }
    if format == ArchiveCreateFormat::SevenZip {
        return create_seven_zip(sources, dest_dir, cancel, password);
    }
    let target = unique_destination(dest_dir, &format!("{base}.{extension}"));
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|error| format!("Could not create {}: {error}", target.display()))?;
    let result = match format {
        ArchiveCreateFormat::Zip => create_zip(file, sources, cancel, password),
        ArchiveCreateFormat::Tar => create_tar(file, sources, cancel).map(|_| ()),
        ArchiveCreateFormat::TarGz => {
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            create_tar(encoder, sources, cancel).and_then(|encoder| {
                encoder
                    .finish()
                    .map(|_| ())
                    .map_err(|error| format!("Could not finish archive: {error}"))
            })
        }
        ArchiveCreateFormat::SevenZip => unreachable!(),
    };
    if let Err(error) = result {
        let _ = fs::remove_file(&target);
        return Err(error);
    }
    Ok(target)
}

fn create_tar<W: Write>(writer: W, sources: &[PathBuf], cancel: &AtomicBool) -> Result<W, String> {
    let mut builder = tar::Builder::new(writer);
    builder.follow_symlinks(false);
    for source in sources {
        if cancel.load(Ordering::Relaxed) {
            return Err("Compression cancelled.".to_owned());
        }
        let name = source
            .file_name()
            .map(|name| name.to_owned())
            .ok_or_else(|| format!("Cannot archive {}.", source.display()))?;
        let meta = fs::symlink_metadata(source)
            .map_err(|error| format!("Could not inspect {}: {error}", source.display()))?;
        if meta.is_dir() {
            builder
                .append_dir_all(Path::new(&name), source)
                .map_err(|error| format!("Could not archive {}: {error}", source.display()))?;
        } else {
            builder
                .append_path_with_name(source, Path::new(&name))
                .map_err(|error| format!("Could not archive {}: {error}", source.display()))?;
        }
    }
    builder
        .finish()
        .map_err(|error| format!("Could not finish archive: {error}"))?;
    builder
        .into_inner()
        .map_err(|error| format!("Could not finish archive: {error}"))
}

fn create_zip(
    file: fs::File,
    sources: &[PathBuf],
    cancel: &AtomicBool,
    password: Option<&str>,
) -> Result<(), String> {
    let mut archive = zip::ZipWriter::new(file);
    let mut options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    if let Some(password) = password {
        options = options.with_aes_encryption(zip::AesMode::Aes256, password);
    }
    for source in sources {
        if cancel.load(Ordering::Relaxed) {
            return Err("Compression cancelled.".to_owned());
        }
        let name = source
            .file_name()
            .ok_or_else(|| format!("Cannot archive {}.", source.display()))?
            .to_string_lossy()
            .into_owned();
        append_zip_entry(&mut archive, source, &name, options, cancel)?;
    }
    archive
        .finish()
        .map(|_| ())
        .map_err(|error| format!("Could not finish ZIP: {error}"))
}

fn create_seven_zip(
    sources: &[PathBuf],
    dest_dir: &Path,
    cancel: &AtomicBool,
    password: Option<&str>,
) -> Result<PathBuf, String> {
    if !seven_zip_available() {
        return Err("7-Zip is unavailable. Install p7zip to create 7z archives.".to_owned());
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Compression cancelled.".to_owned());
    }
    let base = if sources.len() == 1 {
        sources[0]
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "archive".to_owned())
    } else {
        "selection".to_owned()
    };
    let target = unique_destination(dest_dir, &format!("{base}.7z"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = dest_dir.join(format!(".filem-{}-{stamp}.7z", std::process::id()));
    let mut command = std::process::Command::new("7z");
    command.arg("a").arg("-t7z").arg("-y");
    if password.is_some() {
        command.arg("-mhe=on").arg("-p");
    }
    command
        .arg(&temporary)
        .args(sources)
        .current_dir(dest_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if password.is_some() {
        command.stdin(std::process::Stdio::piped());
    } else {
        command.stdin(std::process::Stdio::null());
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start 7-Zip: {error}"))?;
    if let Some(password) = password {
        use std::io::Write;
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(error) = stdin.write_all(format!("{password}\n{password}\n").as_bytes()) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&temporary);
                return Err(format!(
                    "Could not provide archive password to 7-Zip: {error}"
                ));
            }
        }
    }
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&temporary);
            return Err("Compression cancelled.".to_owned());
        }
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&temporary);
                return Err(format!("Could not wait for 7-Zip: {error}"));
            }
        };
        if let Some(status) = status {
            break status;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    };
    if !status.success() || !temporary.is_file() {
        let _ = fs::remove_file(&temporary);
        return Err("7-Zip could not create the archive.".to_owned());
    }
    if let Err(error) = fs::hard_link(&temporary, &target) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Could not save {}: {error}", target.display()));
    }
    fs::remove_file(&temporary)
        .map_err(|error| format!("Archive was created, but temporary cleanup failed: {error}"))?;
    Ok(target)
}

fn append_zip_entry(
    archive: &mut zip::ZipWriter<fs::File>,
    source: &Path,
    name: &str,
    options: zip::write::FileOptions<'_, ()>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Compression cancelled.".to_owned());
    }
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("Could not inspect {}: {error}", source.display()))?;
    if metadata.file_type().is_symlink() {
        archive
            .start_file(name, options.unix_permissions(0o120777))
            .map_err(|error| format!("Could not add {} to ZIP: {error}", source.display()))?;
        let target = fs::read_link(source)
            .map_err(|error| format!("Could not read link {}: {error}", source.display()))?;
        archive
            .write_all(target.to_string_lossy().as_bytes())
            .map_err(|error| {
                format!("Could not write link {} to ZIP: {error}", source.display())
            })?;
    } else if metadata.is_dir() {
        archive
            .add_directory(
                format!("{name}/"),
                options.unix_permissions(metadata.mode()),
            )
            .map_err(|error| format!("Could not add {} to ZIP: {error}", source.display()))?;
        for entry in fs::read_dir(source)
            .map_err(|error| format!("Could not read {}: {error}", source.display()))?
        {
            let entry =
                entry.map_err(|error| format!("Could not read {}: {error}", source.display()))?;
            let child_name = format!("{name}/{}", entry.file_name().to_string_lossy());
            append_zip_entry(archive, &entry.path(), &child_name, options, cancel)?;
        }
    } else if metadata.is_file() {
        archive
            .start_file(name, options.unix_permissions(metadata.mode()))
            .map_err(|error| format!("Could not add {} to ZIP: {error}", source.display()))?;
        let mut input = fs::File::open(source)
            .map_err(|error| format!("Could not open {}: {error}", source.display()))?;
        io::copy(&mut input, archive)
            .map_err(|error| format!("Could not write {} to ZIP: {error}", source.display()))?;
    }
    Ok(())
}
/// Convert an icon glyph-shaped mode into an `rwxr-xr-x` string.
pub fn mode_string(mode: u32) -> String {
    let mut result = String::with_capacity(10);
    result.push(if mode & 0o170000 == 0o040000 {
        'd'
    } else if mode & 0o170000 == 0o120000 {
        'l'
    } else {
        '-'
    });
    for bit in [
        0o400, 0o200, 0o100, 0o040, 0o020, 0o010, 0o004, 0o002, 0o001,
    ] {
        let letter = match bit {
            0o400 | 0o040 | 0o004 => 'r',
            0o200 | 0o020 | 0o002 => 'w',
            _ => 'x',
        };
        result.push(if mode & bit != 0 { letter } else { '-' });
    }
    result
}

fn user_name(uid: u32) -> Option<String> {
    let mut buffer = vec![0u8; 16 * 1024];
    let mut result = std::ptr::null_mut();
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            entry.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return None;
    }
    let entry = unsafe { entry.assume_init() };
    if entry.pw_name.is_null() {
        None
    } else {
        Some(
            unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn group_name(gid: u32) -> Option<String> {
    let mut buffer = vec![0u8; 16 * 1024];
    let mut result = std::ptr::null_mut();
    let mut entry = std::mem::MaybeUninit::<libc::group>::uninit();
    let status = unsafe {
        libc::getgrgid_r(
            gid,
            entry.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return None;
    }
    let entry = unsafe { entry.assume_init() };
    if entry.gr_name.is_null() {
        None
    } else {
        Some(
            unsafe { std::ffi::CStr::from_ptr(entry.gr_name) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn seven_zip_available() -> bool {
    std::process::Command::new("7z")
        .arg("-h")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok()
}

pub struct FolderSize {
    pub bytes: u64,
    pub allocated_bytes: u64,
    pub entries: u64,
    pub truncated: bool,
    pub errors: u64,
}

/// Walk a folder without following symlinks. Bounded so a huge tree cannot
/// freeze the application.
pub fn folder_size(path: &Path, cancel: &AtomicBool, max_entries: u64) -> FolderSize {
    let mut result = FolderSize {
        bytes: 0,
        allocated_bytes: 0,
        entries: 0,
        truncated: false,
        errors: 0,
    };
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        if cancel.load(Ordering::Relaxed) || result.entries >= max_entries {
            result.truncated = result.entries >= max_entries;
            break;
        }
        let Ok(read) = fs::read_dir(&current) else {
            result.errors += 1;
            continue;
        };
        for entry in read.flatten() {
            if cancel.load(Ordering::Relaxed) {
                result.truncated = true;
                break;
            }
            result.entries += 1;
            if result.entries >= max_entries {
                result.truncated = true;
                break;
            }
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                result.errors += 1;
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            result.allocated_bytes = result
                .allocated_bytes
                .saturating_add(meta.blocks().saturating_mul(512));
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                result.bytes = result.bytes.saturating_add(meta.len());
            }
        }
    }
    result
}

pub struct Properties {
    pub path: PathBuf,
    pub name: String,
    pub extension: String,
    pub kind: String,
    pub size_text: String,
    pub size_bytes: u64,
    pub allocated_bytes: u64,
    pub modified_ms: f64,
    pub accessed_ms: f64,
    pub created_ms: f64,
    pub mode: String,
    pub uid: u32,
    pub gid: u32,
    pub owner: String,
    pub group: String,
    pub device: u64,
    pub inode: u64,
    pub is_symlink: bool,
    pub is_directory: bool,
    pub immediate_files: Option<u64>,
    pub immediate_folders: Option<u64>,
    pub items: Option<u64>,
    pub link_target: Option<PathBuf>,
    pub truncated: bool,
}

pub struct ArchiveSummary {
    pub entries: u64,
    pub uncompressed_bytes: u64,
    pub truncated: bool,
}

pub fn archive_summary(path: &Path, max_entries: usize) -> Result<ArchiveSummary, String> {
    let Some(
        format @ (ArchiveFormat::Tar
        | ArchiveFormat::TarGz
        | ArchiveFormat::TarBz2
        | ArchiveFormat::TarXz
        | ArchiveFormat::TarZstd),
    ) = archive_format(path)
    else {
        return Err("This archive format has no TAR summary.".to_owned());
    };
    let (reader, child) = tar_reader(path, format)?;
    let mut archive = tar::Archive::new(reader);
    let mut entries = 0u64;
    let mut bytes = 0u64;
    let mut truncated = false;
    for entry in archive
        .entries()
        .map_err(|error| format!("Could not read TAR: {error}"))?
    {
        if entries as usize >= max_entries {
            truncated = true;
            break;
        }
        let entry = entry.map_err(|error| format!("Could not read TAR entry: {error}"))?;
        bytes = bytes.saturating_add(entry.size());
        entries += 1;
    }
    if !truncated {
        drop(archive);
        finish_decoder(child)?;
    }
    Ok(ArchiveSummary {
        entries,
        uncompressed_bytes: bytes,
        truncated,
    })
}

pub fn properties_with_cancel(path: &Path, cancel: &AtomicBool) -> Result<Properties, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Properties loading was cancelled.".to_owned());
    }
    let meta = fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    let is_symlink = meta.file_type().is_symlink();
    let follow = if is_symlink {
        fs::metadata(path).unwrap_or_else(|_| meta.clone())
    } else {
        meta.clone()
    };
    let is_dir = follow.is_dir();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    let (immediate_files, immediate_folders) = if is_dir {
        let mut files = 0u64;
        let mut folders = 0u64;
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Properties loading was cancelled.".to_owned());
                }
                if fs::symlink_metadata(entry.path())
                    .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
                {
                    folders += 1;
                } else {
                    files += 1;
                }
            }
        }
        (Some(files), Some(folders))
    } else {
        (None, None)
    };
    let (size_text, items, truncated, allocated_bytes) = if is_dir {
        let folder = folder_size(path, cancel, 250_000);
        (
            format_size(folder.bytes),
            Some(folder.entries),
            folder.truncated,
            folder.allocated_bytes,
        )
    } else {
        (
            format_size(follow.len()),
            None,
            false,
            meta.blocks().saturating_mul(512),
        )
    };

    Ok(Properties {
        path: path.to_path_buf(),
        name: name.clone(),
        extension: extension.clone(),
        kind: crate::icons::human_kind(&name, is_dir, &extension),
        size_text,
        size_bytes: if is_dir { 0 } else { follow.len() },
        allocated_bytes,
        modified_ms: follow
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as f64)
            .unwrap_or(0.0),
        accessed_ms: follow
            .accessed()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as f64)
            .unwrap_or(0.0),
        created_ms: follow
            .created()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as f64)
            .unwrap_or(0.0),
        mode: mode_string(meta.permissions().mode()),
        uid: meta.uid(),
        gid: meta.gid(),
        owner: user_name(meta.uid()).unwrap_or_else(|| meta.uid().to_string()),
        group: group_name(meta.gid()).unwrap_or_else(|| meta.gid().to_string()),
        device: meta.dev(),
        inode: meta.ino(),
        is_symlink,
        is_directory: is_dir,
        immediate_files,
        immediate_folders,
        items,
        link_target: if is_symlink {
            fs::read_link(path).ok()
        } else {
            None
        },
        truncated,
    })
}

/// Collect all descendants for folder drag/drop helpers and tests.
#[allow(dead_code)]
pub fn collect_paths(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        if out.len() >= limit {
            break;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        out.push(path.clone());
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if meta.is_dir() && !meta.file_type().is_symlink() {
                if let Ok(read) = fs::read_dir(&path) {
                    for entry in read.flatten() {
                        stack.push(entry.path());
                    }
                }
            }
        }
    }
    out
}

/// A small helper used by search results and the UI.
pub fn display_path(path: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(rest) = path.strip_prefix(&home) {
            if rest.as_os_str().is_empty() {
                return "~".to_owned();
            }
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

/// Exposed for the disk-image workflow later: this module intentionally does
/// not write block devices. Keeping the file-URL helper here avoids callers
/// building URLs by hand.
#[allow(dead_code)]
pub fn file_url(path: &Path) -> String {
    path_to_file_url(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CancellingReader<'a> {
        cancel: &'a AtomicBool,
        sent: bool,
    }

    impl Read for CancellingReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.sent {
                return Ok(0);
            }
            self.sent = true;
            self.cancel.store(true, Ordering::Relaxed);
            buffer[0] = b'x';
            Ok(1)
        }
    }

    fn write_file(path: &Path, contents: &str) {
        fs::write(path, contents).expect("write fixture");
    }

    #[test]
    fn copy_never_overwrites_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("report.txt");
        let target = dir.path().join("copy.txt");
        write_file(&source, "new");
        write_file(&target, "keep");
        assert!(copy_one(&source, &target).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
        assert_eq!(fs::read_to_string(&source).unwrap(), "new");
    }

    #[test]
    fn move_never_overwrites_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        let target = dir.path().join("target.txt");
        write_file(&source, "source");
        write_file(&target, "keep");

        assert!(move_one(&source, &target).is_err());
        assert_eq!(fs::read_to_string(source).unwrap(), "source");
        assert_eq!(fs::read_to_string(target).unwrap(), "keep");
    }

    #[test]
    fn undo_rename_does_not_replace_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("original.txt");
        let to = dir.path().join("renamed.txt");
        write_file(&from, "new occupant");
        write_file(&to, "renamed item");

        let result = UndoOp::Rename {
            from: from.clone(),
            to: to.clone(),
        }
        .undo();

        assert!(result.unwrap_err().contains("already exists"));
        assert_eq!(fs::read_to_string(from).unwrap(), "new occupant");
        assert_eq!(fs::read_to_string(to).unwrap(), "renamed item");
    }

    #[test]
    fn redo_rename_does_not_replace_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("original.txt");
        let to = dir.path().join("renamed.txt");
        write_file(&from, "original item");
        write_file(&to, "new occupant");

        let result = RedoOp::Rename {
            from: from.clone(),
            to: to.clone(),
        }
        .apply();

        assert!(result.unwrap_err().contains("already exists"));
        assert_eq!(fs::read_to_string(from).unwrap(), "original item");
        assert_eq!(fs::read_to_string(to).unwrap(), "new occupant");
    }

    #[test]
    fn staged_rename_never_overwrites_an_unselected_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        let target = dir.path().join("target.txt");
        write_file(&source, "source");
        write_file(&target, "preserve");
        let error = rename_many_safely(&[(source.clone(), target.clone())]).unwrap_err();
        assert!(error.contains("Refusing to overwrite"));
        assert_eq!(fs::read_to_string(source).unwrap(), "source");
        assert_eq!(fs::read_to_string(target).unwrap(), "preserve");
    }

    #[test]
    fn staged_rename_rolls_back_when_a_destination_appears_mid_batch() {
        let dir = tempfile::tempdir().unwrap();
        let first_source = dir.path().join("first.txt");
        let second_source = dir.path().join("second.txt");
        let first_target = dir.path().join("first-renamed.txt");
        let second_target = dir.path().join("second-renamed.txt");
        write_file(&first_source, "first");
        write_file(&second_source, "second");
        let pairs = [
            (first_source.clone(), first_target.clone()),
            (second_source.clone(), second_target.clone()),
        ];

        let error = rename_many_safely_with_progress(&pairs, None, |index, _, _| {
            if index == 1 {
                write_file(&second_target, "external");
            }
        })
        .unwrap_err();

        assert!(error.contains("Could not rename"));
        assert_eq!(fs::read_to_string(first_source).unwrap(), "first");
        assert_eq!(fs::read_to_string(second_source).unwrap(), "second");
        assert!(!first_target.exists());
        assert_eq!(fs::read_to_string(second_target).unwrap(), "external");
    }

    #[test]
    fn transfer_skip_leaves_the_existing_entry_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();
        write_file(&src.join("a.txt"), "incoming");
        write_file(&dst.join("a.txt"), "existing");
        let cancel = AtomicBool::new(false);
        let mut ticks = 0usize;
        let outcome = transfer(
            &[src.join("a.txt")],
            &dst,
            false,
            ConflictPolicy::Skip,
            &cancel,
            &mut |_, _, _, _, _| ticks += 1,
        );
        assert!(outcome.created.is_empty());
        assert_eq!(outcome.skipped, vec!["a.txt".to_owned()]);
        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "existing");
        assert!(ticks > 0, "progress must be reported");
    }

    #[test]
    fn transfer_keep_both_uses_a_numbered_name() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();
        write_file(&src.join("a.txt"), "incoming");
        write_file(&dst.join("a.txt"), "existing");
        let cancel = AtomicBool::new(false);
        let outcome = transfer(
            &[src.join("a.txt")],
            &dst,
            false,
            ConflictPolicy::KeepBoth,
            &cancel,
            &mut |_, _, _, _, _| {},
        );
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "existing");
        assert_eq!(
            fs::read_to_string(dst.join("a (2).txt")).unwrap(),
            "incoming"
        );
        assert_eq!(outcome.created, vec![dst.join("a (2).txt")]);
    }

    #[test]
    fn cancelled_transfer_reports_cancellation_and_copies_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();
        write_file(&src.join("a.txt"), "data");
        let cancel = AtomicBool::new(true);
        let outcome = transfer(
            &[src.join("a.txt")],
            &dst,
            false,
            ConflictPolicy::Ask,
            &cancel,
            &mut |_, _, _, _, _| {},
        );
        assert!(outcome.cancelled);
        assert!(outcome.created.is_empty());
        assert!(!dst.join("a.txt").exists());
    }

    #[test]
    fn symlinks_are_copied_as_links_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();
        std::os::unix::fs::symlink("/etc/hostname", src.join("link")).unwrap();
        copy_one(&src.join("link"), &dst.join("link")).unwrap();
        let meta = fs::symlink_metadata(dst.join("link")).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(
            fs::read_link(dst.join("link")).unwrap(),
            PathBuf::from("/etc/hostname")
        );
    }

    #[test]
    fn nested_folders_copy_with_contents_and_modes() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("tree");
        fs::create_dir_all(src.join("inner")).unwrap();
        write_file(&src.join("inner/note.md"), "hello");
        let dst = dir.path().join("copy");
        copy_one(&src, &dst).unwrap();
        assert_eq!(
            fs::read_to_string(dst.join("inner/note.md")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn folder_sizes_do_not_follow_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        write_file(&root.join("a.bin"), "12345");
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
        let cancel = AtomicBool::new(false);
        let size = folder_size(&root, &cancel, 10_000);
        assert_eq!(size.bytes, 5);
        assert!(size.entries >= 2);
        assert!(!size.truncated);
    }

    #[test]
    fn properties_count_large_folders_and_allocated_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("large");
        fs::create_dir_all(root.join("nested")).unwrap();
        for index in 0..2048 {
            fs::write(root.join(format!("file-{index}.txt")), "data").unwrap();
        }
        fs::write(root.join("nested/item.txt"), "nested").unwrap();

        let properties = properties_with_cancel(&root, &AtomicBool::new(false)).unwrap();
        assert_eq!(properties.immediate_files, Some(2048));
        assert_eq!(properties.immediate_folders, Some(1));
        assert_eq!(properties.items, Some(2050));
        assert!(properties.allocated_bytes > 0);
    }

    #[test]
    fn properties_report_file_metadata_and_link_target() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        fs::write(&file, "metadata").unwrap();

        let file_properties = properties_with_cancel(&file, &AtomicBool::new(false)).unwrap();
        assert_eq!(file_properties.name, "notes.txt");
        assert_eq!(file_properties.size_bytes, 8);
        assert!(!file_properties.mode.is_empty());
        assert_ne!(file_properties.inode, 0);

        let link = dir.path().join("notes-link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let link_properties = properties_with_cancel(&link, &AtomicBool::new(false)).unwrap();
        assert_eq!(link_properties.link_target.as_deref(), Some(file.as_path()));
    }

    #[test]
    fn trash_helpers_accept_the_trash_root_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Trash");
        let files = root.join("files");
        let info = root.join("info");
        let original_dir = dir.path().join("original");
        fs::create_dir_all(&files).unwrap();
        fs::create_dir_all(&info).unwrap();
        fs::create_dir(&original_dir).unwrap();

        let original = original_dir.join("note.txt");
        let trashed = files.join("note.txt");
        write_file(&trashed, "restore me");
        fs::write(
            info.join("note.txt.trashinfo"),
            format!("[Trash Info]\nPath={}\n", original.display()),
        )
        .unwrap();

        let items = trashed_items_in(&root);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].file, trashed);
        assert_eq!(items[0].original, original);
        assert_eq!(restore_trashed(&items[0]).unwrap(), original);
        assert_eq!(fs::read_to_string(&original).unwrap(), "restore me");

        let second = files.join("remove.txt");
        write_file(&second, "remove me");
        fs::write(
            info.join("remove.txt.trashinfo"),
            format!(
                "[Trash Info]\nPath={}\n",
                original_dir.join("remove.txt").display()
            ),
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        let (removed, failed) = empty_trash(&root, &cancel);
        assert_eq!(removed, 1);
        assert!(failed.is_empty());
        assert!(!second.exists());
        assert!(!info.join("remove.txt.trashinfo").exists());
    }

    #[test]
    fn archive_names_never_escape_the_destination() {
        assert!(safe_relative_path(Path::new("../etc/passwd")).is_err());
        assert!(safe_relative_path(Path::new("/etc/passwd")).is_err());
        assert_eq!(
            safe_relative_path(Path::new("notes/a.txt")).unwrap(),
            PathBuf::from("notes/a.txt")
        );
    }

    #[test]
    fn archive_format_recognizes_compressed_tar_variants() {
        assert_eq!(
            archive_format(Path::new("backup.tar.bz2")),
            Some(ArchiveFormat::TarBz2)
        );
        assert_eq!(
            archive_format(Path::new("backup.txz")),
            Some(ArchiveFormat::TarXz)
        );
        assert_eq!(
            archive_format(Path::new("backup.tar.zst")),
            Some(ArchiveFormat::TarZstd)
        );
    }

    #[test]
    fn extracts_optional_compressed_tar_formats_when_decoders_exist() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        fs::create_dir(&source).unwrap();
        write_file(&source.join("inside.txt"), "hello archive");
        let tar_path = dir.path().join("fixture.tar");
        let tar_file = fs::File::create(&tar_path).unwrap();
        let mut builder = tar::Builder::new(tar_file);
        builder.append_dir_all("source", &source).unwrap();
        builder.finish().unwrap();

        for (program, args, suffix) in [
            ("bzip2", vec!["-c", "--"], ".tar.bz2"),
            ("xz", vec!["-c", "--"], ".tar.xz"),
            ("zstd", vec!["-q", "-c", "--"], ".tar.zst"),
        ] {
            let Ok(output) = fs::File::create(dir.path().join(format!("fixture{suffix}"))) else {
                panic!("could not create compressed archive fixture");
            };
            let status = Command::new(program)
                .args(args)
                .arg(&tar_path)
                .stdout(Stdio::from(output))
                .stderr(Stdio::null())
                .status();
            let Ok(status) = status else {
                continue;
            };
            if !status.success() {
                continue;
            }
            let archive_path = dir.path().join(format!("fixture{suffix}"));
            let destination = dir.path().join(format!("out-{}", suffix.trim_matches('.')));
            fs::create_dir(&destination).unwrap();
            extract_archive(
                &archive_path,
                &destination,
                &AtomicBool::new(false),
                &mut |_, _, _| {},
            )
            .unwrap();
            assert_eq!(
                fs::read_to_string(destination.join("source/inside.txt")).unwrap(),
                "hello archive"
            );
        }
    }

    #[test]
    fn extraction_rejects_existing_symlink_escape_in_destination() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("destination");
        let outside = dir.path().join("outside");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, destination.join("link")).unwrap();

        let archive_path = dir.path().join("entry.tar");
        let archive_file = fs::File::create(&archive_path).unwrap();
        let mut builder = tar::Builder::new(archive_file);
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "link/pwned", &b"test"[..])
            .unwrap();
        builder.finish().unwrap();

        let result = extract_archive(
            &archive_path,
            &destination,
            &AtomicBool::new(false),
            &mut |_, _, _| {},
        );
        assert!(result.is_err());
        assert!(!outside.join("pwned").exists());
    }

    #[test]
    fn extraction_enforces_actual_bytes_written() {
        let dir = tempfile::tempdir().unwrap();
        let mut total = 0;
        let result = write_file_checked(
            dir.path(),
            Path::new("too-large"),
            &mut &b"0123456789"[..],
            None,
            &AtomicBool::new(false),
            &mut total,
            20,
            8,
        );
        assert!(result.is_err());
        assert_eq!(total, 0);
        assert!(!dir.path().join("too-large").exists());
    }

    #[test]
    fn extraction_cancellation_removes_the_current_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut reader = CancellingReader {
            cancel: &cancel,
            sent: false,
        };
        let mut total = 0;

        let result = write_file_checked(
            dir.path(),
            Path::new("cancelled"),
            &mut reader,
            None,
            &cancel,
            &mut total,
            20,
            8,
        );

        assert!(result.unwrap_err().contains("cancelled"));
        assert_eq!(total, 0);
        assert!(!dir.path().join("cancelled").exists());
    }

    #[test]
    fn compression_and_extraction_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("notes");
        fs::create_dir(&folder).unwrap();
        write_file(&folder.join("a.txt"), "archived");
        let cancel = AtomicBool::new(false);
        for (index, format) in [
            ArchiveCreateFormat::Zip,
            ArchiveCreateFormat::Tar,
            ArchiveCreateFormat::TarGz,
        ]
        .into_iter()
        .enumerate()
        {
            let archive = compress_archive(
                std::slice::from_ref(&folder),
                dir.path(),
                &cancel,
                format,
                None,
            )
            .unwrap();
            assert!(archive.exists());
            assert_eq!(
                archive_format(&archive),
                Some(match format {
                    ArchiveCreateFormat::Zip => ArchiveFormat::Zip,
                    ArchiveCreateFormat::Tar => ArchiveFormat::Tar,
                    ArchiveCreateFormat::TarGz => ArchiveFormat::TarGz,
                    ArchiveCreateFormat::SevenZip => unreachable!(),
                })
            );
            let out = dir.path().join(format!("out-{index}"));
            fs::create_dir(&out).unwrap();
            let mut ticks = 0usize;
            let count =
                extract_archive(&archive, &out, &cancel, &mut |_, _, _| ticks += 1).unwrap();
            assert!(count >= 1);
            assert_eq!(
                fs::read_to_string(out.join("notes/a.txt")).unwrap(),
                "archived"
            );
            assert!(ticks > 0);
        }
    }

    #[test]
    fn encrypted_zip_uses_aes_and_requires_the_password() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("secret.txt");
        write_file(&source, "private contents");
        let cancel = AtomicBool::new(false);
        let archive = compress_archive(
            std::slice::from_ref(&source),
            dir.path(),
            &cancel,
            ArchiveCreateFormat::Zip,
            Some("correct horse"),
        )
        .unwrap();
        let file = fs::File::open(&archive).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert!(archive.by_name("secret.txt").is_err());
        let mut entry = archive
            .by_name_decrypt("secret.txt", b"correct horse")
            .unwrap();
        let mut contents = String::new();
        entry.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "private contents");
        drop(entry);
        assert!(archive
            .by_name_decrypt("secret.txt", b"wrong password")
            .is_err());
    }

    #[test]
    fn seven_zip_creation_supports_encryption_without_password_argv() {
        if !seven_zip_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("secret.txt");
        write_file(&source, "private contents");
        let cancel = AtomicBool::new(false);
        let archive = compress_archive(
            std::slice::from_ref(&source),
            dir.path(),
            &cancel,
            ArchiveCreateFormat::SevenZip,
            Some("correct horse"),
        )
        .unwrap();
        assert_eq!(archive.extension().and_then(|ext| ext.to_str()), Some("7z"));
        let output = Command::new("7z")
            .args(["t", "-pcorrect horse"])
            .arg(&archive)
            .output()
            .unwrap();
        assert!(output.status.success());
        let output = Command::new("7z")
            .args(["t", "-pwrong password"])
            .arg(&archive)
            .output()
            .unwrap();
        assert!(!output.status.success());
    }

    #[test]
    fn names_are_validated_before_touching_the_filesystem() {
        assert!(validate_file_name("  ").is_err());
        assert!(validate_file_name("..").is_err());
        assert!(validate_file_name("a/b").is_err());
        assert_eq!(validate_file_name(" report.txt ").unwrap(), "report.txt");
    }
}
