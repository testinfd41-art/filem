//! The Rust-side application controller exposed to QML.
//!
//! One `Backend` owns every tab. Each tab keeps its own path, history, sorting,
//! entries and search state; only the active tab is exposed through the list
//! model, so switching tabs is a model reset rather than a new window.
//!
//! All filesystem work that can take measurable time (directory reads, search,
//! transfers, trash, undo, folder sizes) runs on a worker thread and returns
//! through a queued callback. The Qt thread never blocks on the disk.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use qmetaobject::prelude::*;
use qmetaobject::{QMetaType, QVariantList, QVariantMap, USER_ROLE};

use crate::batch;
use crate::config::Config;
use crate::fs_entry::{
    friendly_io_error, read_directory, read_recent_files, read_tag_collection, Entry, SortKey,
    SortSpec,
};
use crate::icons::{icon_names_for, IconResolver};
use crate::image_conversion;
use crate::metadata::{self, MetadataReport};
use crate::notifications;
use crate::ops::{self, ConflictPolicy, RedoOp, TrashItem, UndoOp};
use crate::paths::{self, breadcrumbs, format_size, home_dir, trash_dir, user_places, Place};
use crate::preview;
use crate::search;
use crate::store::{BookmarkStore, RecentItem, RecentStore, Session, SessionTab};
use crate::sysclip;
use crate::tags::TagStore;
use crate::transfer::{self, ItemInfo, ItemResult, JobContext, JobResult, Operation};
use crate::volumes;

const ROLE_NAME: i32 = USER_ROLE;
const ROLE_PATH: i32 = USER_ROLE + 1;
const ROLE_IS_DIR: i32 = USER_ROLE + 2;
const ROLE_SIZE: i32 = USER_ROLE + 3;
const ROLE_SIZE_TEXT: i32 = USER_ROLE + 4;
const ROLE_MODIFIED_MS: i32 = USER_ROLE + 5;
const ROLE_KIND: i32 = USER_ROLE + 6;
const ROLE_ICON_URL: i32 = USER_ROLE + 7;
const ROLE_IS_SYMLINK: i32 = USER_ROLE + 8;
const ROLE_IS_HIDDEN: i32 = USER_ROLE + 9;
const ROLE_EXTENSION: i32 = USER_ROLE + 10;
const ROLE_MODIFIED_TEXT: i32 = USER_ROLE + 11;
const ROLE_TAG: i32 = USER_ROLE + 12;
const ROLE_DISPLAY_PATH: i32 = USER_ROLE + 13;
const ROLE_TREE_DEPTH: i32 = USER_ROLE + 14;
const ROLE_TREE_EXPANDED: i32 = USER_ROLE + 15;
const ROLE_TREE_EXPANDABLE: i32 = USER_ROLE + 16;
const ROLE_TAGS: i32 = USER_ROLE + 17;

const MAX_SEARCH_RESULTS: usize = 400;
const MAX_UNDO_STEPS: usize = 40;
const RECENT_VIRTUAL_PATH: &str = "/.__filem_recent__";
const TAGS_VIRTUAL_PREFIX: &str = "/.__filem_tags__/";
static DEVICE_MONITOR_STARTED: AtomicBool = AtomicBool::new(false);
static USB_MONITOR_STARTED: AtomicBool = AtomicBool::new(false);
static RECENT_SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RECENT_SAVE_LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// Bounded thumbnail worker pool
// ---------------------------------------------------------------------------
//
// A bounded, priority-ordered queue keeps thumbnail work off the UI thread.
// Viewport requests outrank nearby cache-buffer requests, and a new directory
// naturally deprioritizes queued work from the previous one.
const THUMBNAIL_WORKERS: usize = 2;
const THUMBNAIL_QUEUE_CAPACITY: usize = 256;

struct ThumbnailJob {
    path: PathBuf,
    key: String,
    priority: u8,
    sequence: u64,
}

type ThumbnailCallback = Box<dyn FnOnce(ThumbnailResult) + Send + 'static>;

struct ThumbnailPoolState {
    queued: Vec<ThumbnailJob>,
    in_flight: HashSet<String>,
    callbacks: HashMap<String, Vec<ThumbnailCallback>>,
    next_sequence: u64,
}

struct ThumbnailPool {
    state: Mutex<ThumbnailPoolState>,
    available: Condvar,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ThumbnailSubmit {
    Queued,
    Deduplicated,
    Full,
}

static THUMBNAIL_POOL: OnceLock<Arc<ThumbnailPool>> = OnceLock::new();

fn thumbnail_pool() -> &'static Arc<ThumbnailPool> {
    THUMBNAIL_POOL.get_or_init(|| {
        let pool = Arc::new(ThumbnailPool {
            state: Mutex::new(ThumbnailPoolState {
                queued: Vec::new(),
                in_flight: HashSet::new(),
                callbacks: HashMap::new(),
                next_sequence: 0,
            }),
            available: Condvar::new(),
        });
        for worker_index in 0..THUMBNAIL_WORKERS {
            let pool = Arc::clone(&pool);
            std::thread::Builder::new()
                .name(format!("filem-thumbnail-{worker_index}"))
                .spawn(move || thumbnail_worker(pool))
                .expect("failed to start thumbnail worker");
        }
        pool
    })
}

fn submit_thumbnail_job(
    path: PathBuf,
    key: String,
    priority: u8,
    callback: ThumbnailCallback,
) -> ThumbnailSubmit {
    let pool = thumbnail_pool();
    let mut state = pool.state.lock().unwrap_or_else(|error| error.into_inner());
    if state.in_flight.contains(&key) {
        if let Some(queued) = state.queued.iter_mut().find(|job| job.key == key) {
            queued.priority = queued.priority.max(priority);
        }
        state.callbacks.entry(key).or_default().push(callback);
        return ThumbnailSubmit::Deduplicated;
    }
    if state.queued.len() >= THUMBNAIL_QUEUE_CAPACITY {
        return ThumbnailSubmit::Full;
    }

    if priority > 1 {
        for queued in &mut state.queued {
            if queued.path.parent() != path.parent() {
                queued.priority = queued.priority.saturating_sub(1);
            }
        }
    }
    let sequence = state.next_sequence;
    state.next_sequence = state.next_sequence.wrapping_add(1);
    state.in_flight.insert(key.clone());
    state.callbacks.insert(key.clone(), vec![callback]);
    state.queued.push(ThumbnailJob {
        path,
        key,
        priority,
        sequence,
    });
    drop(state);
    pool.available.notify_one();
    ThumbnailSubmit::Queued
}

fn promote_thumbnail_job(key: &str, priority: u8) {
    let pool = thumbnail_pool();
    let mut state = pool.state.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(job) = state.queued.iter_mut().find(|job| job.key == key) {
        job.priority = job.priority.max(priority);
    }
}

fn thumbnail_worker(pool: Arc<ThumbnailPool>) {
    loop {
        let job = {
            let mut state = pool.state.lock().unwrap_or_else(|error| error.into_inner());
            while state.queued.is_empty() {
                state = pool
                    .available
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
            let next = state
                .queued
                .iter()
                .enumerate()
                .max_by(|(_, left), (_, right)| {
                    left.priority
                        .cmp(&right.priority)
                        .then_with(|| right.sequence.cmp(&left.sequence))
                })
                .map(|(index, _)| index)
                .expect("non-empty thumbnail queue");
            state.queued.swap_remove(next)
        };
        let result = crate::preview::request_thumbnail(&job.path)
            .map(|path| crate::paths::path_to_file_url(&path));
        let callbacks = {
            let mut state = pool.state.lock().unwrap_or_else(|error| error.into_inner());
            state.in_flight.remove(&job.key);
            state.callbacks.remove(&job.key).unwrap_or_default()
        };
        for callback in callbacks {
            callback(ThumbnailResult {
                path: job.key.clone(),
                result: result.clone(),
            });
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum HistoryIntent {
    /// A fresh navigation that should append to the tab's history.
    New,
    Back(usize),
    Forward(usize),
    /// Reload the same location without touching history.
    Current,
}

fn default_application_id(output: &str) -> Option<&str> {
    output.lines().find_map(|line| {
        let line = line.trim();
        let (_, app_id) = line.split_once(": ")?;
        if !line.starts_with("Default application for ") {
            return None;
        }
        let app_id = app_id.trim();
        (app_id.ends_with(".desktop") && !app_id.contains('/')).then_some(app_id)
    })
}

fn xdg_application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        dirs.push(PathBuf::from(data_home).join("applications"));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }
    if let Some(data_dirs) = std::env::var_os("XDG_DATA_DIRS").filter(|value| !value.is_empty()) {
        dirs.extend(std::env::split_paths(&data_dirs).map(|path| path.join("applications")));
    } else {
        dirs.extend([
            PathBuf::from("/usr/local/share/applications"),
            PathBuf::from("/usr/share/applications"),
        ]);
    }
    dirs
}

fn desktop_file_id(relative_path: &Path) -> String {
    relative_path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("-")
}

fn find_desktop_file(app_id: &str) -> Result<PathBuf, String> {
    for applications_dir in xdg_application_dirs() {
        if let Ok(path) = find_desktop_file_in(&applications_dir, app_id) {
            return Ok(path);
        }
    }
    Err(format!(
        "Could not locate the selected application descriptor ({app_id})."
    ))
}

struct SearchState {
    query: String,
    filters: String,
    recursive: bool,
    truncated: bool,
    visited: usize,
    generation: u64,
    cancel: Arc<AtomicBool>,
}

impl SearchState {
    fn restart(&mut self) -> Arc<AtomicBool> {
        self.cancel.store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = cancel.clone();
        cancel
    }
}

struct Tab {
    id: u64,
    path: PathBuf,
    history: Vec<PathBuf>,
    history_index: usize,
    sort: SortSpec,
    /// Entries currently displayed (folder contents or search results).
    entries: Vec<Entry>,
    tree_rows: Vec<Entry>,
    tree_expanded: HashSet<PathBuf>,
    tree_children: HashMap<PathBuf, Vec<Entry>>,
    tree_loading: HashSet<PathBuf>,
    /// Folder contents kept aside while search results are displayed.
    folder_entries: Vec<Entry>,
    loading: bool,
    loaded_path: Option<PathBuf>,
    error: String,
    generation: u64,
    tree_generation: u64,
    search_generation: u64,
    search_running: bool,
    pending_intent: Option<HistoryIntent>,
    search: Option<SearchState>,
}

impl Tab {
    fn new(id: u64, path: PathBuf, sort: SortSpec) -> Self {
        Self {
            id,
            path: path.clone(),
            history: vec![path],
            history_index: 0,
            sort,
            entries: Vec::new(),
            tree_rows: Vec::new(),
            tree_expanded: HashSet::new(),
            tree_children: HashMap::new(),
            tree_loading: HashSet::new(),
            folder_entries: Vec::new(),
            loading: false,
            loaded_path: None,
            error: String::new(),
            generation: 0,
            tree_generation: 0,
            search_generation: 0,
            search_running: false,
            pending_intent: None,
            search: None,
        }
    }

    fn cancel_search(&mut self) {
        if let Some(state) = self.search.take() {
            state.cancel.store(true, Ordering::Relaxed);
        }
        self.search_generation = self.search_generation.wrapping_add(1);
        self.search_running = false;
    }

    fn install_folder_entries(&mut self, entries: Vec<Entry>) {
        if self.search.is_some() {
            self.folder_entries = entries;
        } else {
            self.entries = entries;
            self.folder_entries.clear();
        }
    }

    fn finish_search(&mut self) {
        let had_search = self.search.is_some();
        self.cancel_search();
        if had_search {
            self.entries = std::mem::take(&mut self.folder_entries);
        } else {
            self.folder_entries.clear();
        }
    }

    fn shows_loading_indicator(&self) -> bool {
        self.loading && self.loaded_path.as_ref() != Some(&self.path)
    }

    fn title(&self) -> String {
        tab_title(&self.path)
    }
}

fn tab_title(path: &Path) -> String {
    if is_recent_path(path) {
        return "Recent".to_owned();
    }
    if let Some((_, untagged)) = parse_tag_collection_path(path) {
        return if untagged { "Untagged" } else { "Tags" }.to_owned();
    }
    if path == home_dir() {
        return "Home".to_owned();
    }
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn is_recent_path(path: &Path) -> bool {
    path == Path::new(RECENT_VIRTUAL_PATH)
}

fn tag_collection_path(tags: &[usize], untagged: bool) -> PathBuf {
    let route = if untagged {
        "untagged".to_owned()
    } else if tags.is_empty() {
        "all".to_owned()
    } else {
        let mut tags = tags.to_vec();
        tags.sort_unstable();
        tags.dedup();
        tags.into_iter()
            .filter(|tag| *tag < crate::tags::TAG_COUNT)
            .map(|tag| tag.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    PathBuf::from(format!("{TAGS_VIRTUAL_PREFIX}{route}"))
}

fn is_tag_collection_path(path: &Path) -> bool {
    path.to_string_lossy().starts_with(TAGS_VIRTUAL_PREFIX)
}

fn is_virtual_collection(path: &Path) -> bool {
    is_recent_path(path) || is_tag_collection_path(path)
}

/// The shell a terminal should start for this user.
///
/// Terminal emulators launch `$SHELL`, and Filem passes its environment along.
/// Development shells (`nix develop`) export a non-interactive bash build there,
/// which lacks `progcomp` and does not treat `\[`/`\]` as non-printing prompt
/// markers, so "Open Terminal Here" would otherwise inherit a broken prompt.
/// Resolve the login shell from passwd instead.
fn login_shell() -> Option<String> {
    let uid = unsafe { libc::getuid() };
    let entry = unsafe { libc::getpwuid(uid) };
    if entry.is_null() {
        return None;
    }
    let shell = unsafe { (*entry).pw_shell };
    if shell.is_null() {
        return None;
    }
    let shell = unsafe { std::ffi::CStr::from_ptr(shell) }
        .to_string_lossy()
        .into_owned();
    let shell = shell.trim();
    let path = Path::new(shell);
    if shell.is_empty() || !path.is_absolute() || !path.exists() {
        None
    } else {
        Some(shell.to_owned())
    }
}

/// Build the command that launches the configured terminal in `directory`.
///
/// The terminal binary is a single configured name; the path is an argument,
/// never a shell string.
fn terminal_command(terminal: &str, directory: &Path) -> Command {
    let mut command = Command::new(terminal);
    command
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // A terminal emulator starts `$SHELL`. Point it at the login shell so a
    // development-shell environment cannot substitute a non-interactive bash;
    // keep the inherited value only when passwd cannot be resolved.
    if let Some(shell) = login_shell() {
        command.env("SHELL", shell);
    }
    command
}

fn same_filesystem_selection(sources: &[PathBuf], destination: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(destination_metadata) = fs::metadata(destination) else {
        return false;
    };
    sources.iter().all(|source| {
        fs::symlink_metadata(source)
            .is_ok_and(|metadata| metadata.dev() == destination_metadata.dev())
    })
}

fn parse_tag_collection_path(path: &Path) -> Option<(Vec<usize>, bool)> {
    let route = path
        .to_string_lossy()
        .strip_prefix(TAGS_VIRTUAL_PREFIX)?
        .to_owned();
    if route == "untagged" {
        return Some((Vec::new(), true));
    }
    if route == "all" {
        return Some((Vec::new(), false));
    }
    let mut selected = Vec::new();
    for item in route.split(',').filter(|item| !item.is_empty()) {
        let tag = item.parse::<usize>().ok()?;
        if tag >= crate::tags::TAG_COUNT {
            return None;
        }
        selected.push(tag);
    }
    (!selected.is_empty()).then_some((selected, false))
}

#[derive(Clone, PartialEq, Eq)]
struct Clipboard {
    paths: Vec<PathBuf>,
    cut: bool,
}

#[derive(Default)]
struct SharedClipboard {
    revision: u64,
    clipboard: Option<Clipboard>,
    last_external_clipboard: Option<sysclip::ClipboardSnapshot>,
}

static SHARED_CLIPBOARD: OnceLock<Mutex<SharedClipboard>> = OnceLock::new();

fn shared_clipboard() -> &'static Mutex<SharedClipboard> {
    SHARED_CLIPBOARD.get_or_init(|| Mutex::new(SharedClipboard::default()))
}

fn publish_shared_clipboard(clipboard: Option<Clipboard>) -> (Option<Clipboard>, u64) {
    let mut shared = shared_clipboard()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    shared.revision = shared.revision.wrapping_add(1);
    shared.clipboard = clipboard;
    (shared.clipboard.clone(), shared.revision)
}

struct PendingConflict {
    sources: Vec<PathBuf>,
    dest: PathBuf,
    cut: bool,
    from_clipboard: bool,
}

struct LoadResult {
    tab_id: u64,
    generation: u64,
    path: PathBuf,
    entries: Result<Vec<Entry>, String>,
    stale_tag_paths: Vec<PathBuf>,
}

struct TreeLoadResult {
    tab_id: u64,
    generation: u64,
    path: PathBuf,
    entries: Result<Vec<Entry>, String>,
}

struct SeenDirectoryResult {
    tab_id: u64,
    path: PathBuf,
    entries: Result<Vec<Entry>, String>,
}

struct PropertySnapshot {
    properties: ops::Properties,
    report: MetadataReport,
    tags: Vec<String>,
    icon_url: String,
}

struct SearchResult {
    tab_id: u64,
    generation: u64,
    update: search::SearchUpdate,
}

struct PreviewResult {
    generation: u64,
    data: preview::PreviewData,
}

struct ThumbnailResult {
    path: String,
    result: Result<String, String>,
}

struct PendingRecentFile {
    path: PathBuf,
    opened_at_ms: i64,
    generation: u64,
}

struct OpTick {
    generation: u64,
    detail: String,
    progress: f64,
}

type OperationProgress = Box<dyn FnMut(String, f64) + Send>;

struct TransferFinished {
    undo: Option<UndoOp>,
    completed_sources: Vec<PathBuf>,
    from_clipboard: bool,
    message: String,
    error: Option<String>,
}

fn transfer_job_variant(job: &transfer::JobSnapshot) -> QVariantMap {
    let mut map = QVariantMap::default();
    map.insert(
        QString::from("id"),
        QVariant::from(QString::from(job.id.to_string())),
    );
    map.insert(
        QString::from("operation"),
        QVariant::from(QString::from(job.operation.label())),
    );
    map.insert(
        QString::from("status"),
        QVariant::from(QString::from(job.status.as_str())),
    );
    map.insert(
        QString::from("sources"),
        QVariant::from(QString::from(job.source_summary())),
    );
    map.insert(
        QString::from("destination"),
        QVariant::from(QString::from(
            job.destination
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        )),
    );
    map.insert(
        QString::from("totalFiles"),
        QVariant::from(job.total_files as i32),
    );
    map.insert(
        QString::from("totalFolders"),
        QVariant::from(job.total_folders as i32),
    );
    map.insert(
        QString::from("completedFiles"),
        QVariant::from(job.completed_files as i32),
    );
    map.insert(
        QString::from("completedFolders"),
        QVariant::from(job.completed_folders as i32),
    );
    map.insert(
        QString::from("pendingItems"),
        QVariant::from(job.pending_items as i32),
    );
    map.insert(
        QString::from("skippedItems"),
        QVariant::from(job.skipped_items as i32),
    );
    map.insert(
        QString::from("failedItems"),
        QVariant::from(job.failed_items as i32),
    );
    map.insert(
        QString::from("currentItem"),
        QVariant::from(QString::from(
            job.current_item.to_string_lossy().into_owned(),
        )),
    );
    map.insert(QString::from("progress"), QVariant::from(job.progress));
    map.insert(
        QString::from("bytesPerSecond"),
        QVariant::from(job.bytes_per_second as f64),
    );
    map.insert(
        QString::from("etaSeconds"),
        QVariant::from(job.eta_seconds.map_or(-1, |value| value as i32)),
    );
    map.insert(
        QString::from("sizeKnown"),
        QVariant::from(job.inventory_ready),
    );
    map.insert(
        QString::from("totalSize"),
        QVariant::from(QString::from(format_size(job.total_bytes))),
    );
    map.insert(
        QString::from("completedSize"),
        QVariant::from(QString::from(format_size(job.bytes_completed))),
    );
    map.insert(
        QString::from("hasProgressBytes"),
        QVariant::from(job.bytes_completed > 0),
    );
    map.insert(
        QString::from("startedAtMs"),
        QVariant::from(job.started_at_ms.unwrap_or(0) as f64),
    );
    map.insert(
        QString::from("finishedAtMs"),
        QVariant::from(job.finished_at_ms.unwrap_or(0) as f64),
    );
    map.insert(
        QString::from("summary"),
        QVariant::from(QString::from(job.summary.clone())),
    );
    map.insert(
        QString::from("error"),
        QVariant::from(QString::from(job.error.clone())),
    );
    map.insert(
        QString::from("omittedItemDetails"),
        QVariant::from(job.omitted_item_details as i32),
    );
    map
}

fn set_transfer_inventory(context: &JobContext, inventory: ops::TransferInventory) {
    let items = inventory
        .items
        .iter()
        .map(|item| ItemInfo {
            path: item.path.clone(),
            is_directory: item.is_directory,
            size_bytes: item.size_bytes,
        })
        .collect::<Vec<_>>();
    context.set_inventory_counted(
        &items,
        inventory.total_files,
        inventory.total_folders,
        inventory.total_bytes,
    );
}

struct BatchPreviewResult {
    generation: u64,
    result: Result<Vec<batch::BatchRow>, String>,
}

struct BatchRenameDone {
    generation: u64,
    rows: Vec<batch::BatchRow>,
    pairs: Vec<(PathBuf, PathBuf)>,
    error: Option<String>,
}

struct UndoDone {
    generation: u64,
    description: String,
    original: UndoOp,
    redo: Option<RedoOp>,
    error: Option<String>,
}

struct RedoDone {
    generation: u64,
    description: String,
    original: RedoOp,
    undo: Option<UndoOp>,
    error: Option<String>,
}

fn list_paths(list: &QVariantList) -> Vec<PathBuf> {
    (0..list.len())
        .filter_map(|index| QString::from_qvariant(list[index].clone()))
        .map(|value| PathBuf::from(value.to_string()))
        .filter(|path| !path.as_os_str().is_empty())
        .collect()
}

fn batch_options_from_json(source: &str) -> Result<batch::BatchOptions, String> {
    use batch::{ExtensionMode, LetterCase, NumberPlacement, RenameRule, SortMode};
    use serde_json::Value;

    let value: Value = serde_json::from_str(source)
        .map_err(|error| format!("Could not read the batch rename settings: {error}"))?;
    let get_string = |object: &Value, key: &str, default: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(default)
            .to_owned()
    };
    let get_bool = |object: &Value, key: &str, default: bool| {
        object.get(key).and_then(Value::as_bool).unwrap_or(default)
    };
    let get_i64 = |object: &Value, key: &str, default: i64| {
        object.get(key).and_then(Value::as_i64).unwrap_or(default)
    };
    let get_usize = |object: &Value, key: &str, default: usize| {
        object
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(default)
    };

    let mut rules = Vec::new();
    if let Some(items) = value.get("rules").and_then(Value::as_array) {
        for item in items {
            let kind = item
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| "A batch rule is missing its operation type.".to_owned())?;
            let rule = match kind {
                "replace" => RenameRule::Replace {
                    find: get_string(item, "find", ""),
                    replace: get_string(item, "replace", ""),
                    case_sensitive: get_bool(item, "caseSensitive", true),
                    whole_word: get_bool(item, "wholeWord", false),
                    regex: get_bool(item, "regex", false),
                    include_extension: get_bool(item, "includeExtension", false),
                },
                "prefix" => RenameRule::Prefix(get_string(item, "text", "")),
                "suffix" => RenameRule::Suffix(get_string(item, "text", "")),
                "removeText" => RenameRule::RemoveText {
                    text: get_string(item, "text", ""),
                    case_sensitive: get_bool(item, "caseSensitive", true),
                    whole_word: get_bool(item, "wholeWord", false),
                },
                "removeRange" => RenameRule::RemoveRange {
                    start: get_usize(item, "start", 0),
                    length: get_usize(item, "length", 0),
                },
                "upper" => RenameRule::ChangeCase(LetterCase::Upper),
                "lower" => RenameRule::ChangeCase(LetterCase::Lower),
                "title" => RenameRule::ChangeCase(LetterCase::Title),
                "clean" => RenameRule::CleanCharacters,
                "normalizeWhitespace" => RenameRule::NormalizeWhitespace,
                "replaceSpaces" => RenameRule::ReplaceSpaces(get_string(item, "separator", "_")),
                _ => return Err(format!("Unsupported batch operation: {kind}.")),
            };
            rules.push(rule);
        }
    }

    let extension_mode = match get_string(&value, "extensionMode", "preserve").as_str() {
        "preserve" => ExtensionMode::Preserve,
        "change" => ExtensionMode::Change,
        "remove" => ExtensionMode::Remove,
        other => return Err(format!("Unsupported extension mode: {other}.")),
    };
    let number_placement = match get_string(&value, "numberPlacement", "suffix").as_str() {
        "prefix" => NumberPlacement::Prefix,
        "suffix" => NumberPlacement::Suffix,
        "template" => NumberPlacement::Template,
        other => return Err(format!("Unsupported number placement: {other}.")),
    };
    let sort = match get_string(&value, "sort", "selection").as_str() {
        "selection" => SortMode::Selection,
        "name" => SortMode::Name,
        "modified" => SortMode::Modified,
        "created" => SortMode::Created,
        other => return Err(format!("Unsupported batch sort order: {other}.")),
    };
    Ok(batch::BatchOptions {
        rules,
        template: get_string(&value, "template", ""),
        extension_mode,
        extension: get_string(&value, "extension", ""),
        numbering: get_bool(&value, "numbering", false),
        start: get_i64(&value, "start", 1),
        step: get_i64(&value, "step", 1),
        padding: get_usize(&value, "padding", 3),
        number_placement,
        number_separator: get_string(&value, "numberSeparator", "_"),
        sort,
        descending: get_bool(&value, "descending", false),
        case_insensitive: get_bool(&value, "caseInsensitive", false),
    })
}

fn batch_rows_to_variant(rows: &[batch::BatchRow]) -> QVariantList {
    let mut list = QVariantList::default();
    for row in rows {
        let mut item = QVariantMap::default();
        let status = if row.result != "Preview" {
            row.result.as_str()
        } else if !row.included {
            "Excluded"
        } else if row.conflict {
            "Conflict"
        } else if !row.error.is_empty() {
            "Invalid"
        } else if row.unchanged {
            "Unchanged"
        } else {
            "Ready"
        };
        item.insert(
            QString::from("path"),
            QVariant::from(QString::from(row.origin.to_string_lossy().to_string())),
        );
        item.insert(
            QString::from("original"),
            QVariant::from(QString::from(row.old_name.clone())),
        );
        item.insert(
            QString::from("proposed"),
            QVariant::from(QString::from(row.new_name.clone())),
        );
        item.insert(
            QString::from("status"),
            QVariant::from(QString::from(status)),
        );
        item.insert(
            QString::from("reason"),
            QVariant::from(QString::from(row.error.clone())),
        );
        item.insert(QString::from("included"), QVariant::from(row.included));
        item.insert(QString::from("unchanged"), QVariant::from(row.unchanged));
        item.insert(QString::from("conflict"), QVariant::from(row.conflict));
        for (key, value) in [
            ("oldPrefix", &row.old_prefix),
            ("oldChanged", &row.old_changed),
            ("oldSuffix", &row.old_suffix),
            ("newPrefix", &row.new_prefix),
            ("newChanged", &row.new_changed),
            ("newSuffix", &row.new_suffix),
        ] {
            item.insert(
                QString::from(key),
                QVariant::from(QString::from(value.clone())),
            );
        }
        list.push(QVariant::from(item));
    }
    list
}

fn summarize_failures(failed: &[(PathBuf, String)]) -> String {
    if failed.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = failed
        .iter()
        .take(3)
        .map(|(path, error)| {
            format!(
                "{}: {error}",
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string())
            )
        })
        .collect();
    if failed.len() > 3 {
        parts.push(format!("and {} more", failed.len() - 3));
    }
    parts.join("; ")
}

fn item_word(count: usize) -> &'static str {
    if count == 1 {
        "item"
    } else {
        "items"
    }
}

fn same_entries(left: &[Entry], right: &[Entry]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.name == right.name
                && left.path == right.path
                && left.is_dir == right.is_dir
                && left.is_symlink == right.is_symlink
                && left.is_hidden == right.is_hidden
                && left.size == right.size
                && left.modified_ms.to_bits() == right.modified_ms.to_bits()
                && left.kind == right.kind
                && left.icon_url == right.icon_url
                && left.extension == right.extension
                && left.dev == right.dev
                && left.ino == right.ino
                && left.tag == right.tag
                && left.display_path == right.display_path
                && left.tree_depth == right.tree_depth
                && left.tree_expanded == right.tree_expanded
        })
}

fn flatten_tree(
    roots: &[Entry],
    children: &HashMap<PathBuf, Vec<Entry>>,
    expanded: &HashSet<PathBuf>,
) -> Vec<Entry> {
    let mut rows = Vec::new();
    flatten_tree_children(roots, children, expanded, 0, &mut HashSet::new(), &mut rows);
    rows
}

fn flatten_tree_children(
    entries: &[Entry],
    children: &HashMap<PathBuf, Vec<Entry>>,
    expanded: &HashSet<PathBuf>,
    depth: usize,
    ancestors: &mut HashSet<PathBuf>,
    rows: &mut Vec<Entry>,
) {
    for entry in entries {
        let mut row = entry.clone();
        row.tree_depth = depth;
        row.tree_expanded = entry.is_dir && !entry.is_symlink && expanded.contains(&entry.path);
        rows.push(row);

        if depth >= 256
            || !entry.is_dir
            || entry.is_symlink
            || !expanded.contains(&entry.path)
            || !ancestors.insert(entry.path.clone())
        {
            continue;
        }
        if let Some(nested) = children.get(&entry.path) {
            flatten_tree_children(nested, children, expanded, depth + 1, ancestors, rows);
        }
        ancestors.remove(&entry.path);
    }
}

fn set_expanded_path(expanded_paths: &mut HashSet<PathBuf>, path: PathBuf, expanded: bool) -> bool {
    if expanded {
        expanded_paths.insert(path)
    } else {
        expanded_paths.remove(&path)
    }
}

#[derive(QObject)]
pub struct Backend {
    base: qt_base_class!(trait QAbstractListModel),

    current_path: qt_property!(QString; NOTIFY current_path_changed),
    current_path_changed: qt_signal!(),

    preview_data: qt_property!(QVariantMap; NOTIFY preview_changed),
    preview_changed: qt_signal!(),
    thumbnail_ready_path: qt_property!(QString; NOTIFY thumbnail_ready_changed),
    thumbnail_ready_changed: qt_signal!(),
    thumbnail_ready_cache_url: String,

    status: qt_property!(QString; NOTIFY status_changed),
    status_changed: qt_signal!(),

    error: qt_property!(QString; NOTIFY error_changed),
    error_changed: qt_signal!(),

    loading: qt_property!(bool; NOTIFY loading_changed),
    loading_changed: qt_signal!(),

    show_hidden: qt_property!(bool; NOTIFY view_changed),
    preview_visible: qt_property!(bool; NOTIFY view_changed),
    sort_key: qt_property!(QString; NOTIFY view_changed),
    sort_ascending: qt_property!(bool; NOTIFY view_changed),
    directories_first: qt_property!(bool; NOTIFY view_changed),
    show_size: qt_property!(bool; NOTIFY view_changed),
    show_modified: qt_property!(bool; NOTIFY view_changed),
    show_kind: qt_property!(bool; NOTIFY view_changed),
    view_mode: qt_property!(QString; NOTIFY view_changed),
    view_changed: qt_signal!(),

    single_click_open: qt_property!(bool; NOTIFY behavior_changed),
    sidebar_visible: qt_property!(bool; NOTIFY behavior_changed),
    sidebar_width: qt_property!(i32; NOTIFY behavior_changed),
    tab_bar_visible: qt_property!(bool; NOTIFY behavior_changed),
    drag_hover_delay_ms: qt_property!(i32; NOTIFY behavior_changed),
    terminal: qt_property!(QString; NOTIFY behavior_changed),
    behavior_changed: qt_signal!(),

    item_count: qt_property!(i32; NOTIFY item_count_changed),
    item_count_changed: qt_signal!(),

    can_go_back: qt_property!(bool; NOTIFY nav_changed),
    can_go_forward: qt_property!(bool; NOTIFY nav_changed),
    nav_changed: qt_signal!(),

    breadcrumbs: qt_property!(QVariantList; NOTIFY breadcrumbs_changed),
    breadcrumbs_changed: qt_signal!(),

    places: qt_property!(QVariantList; NOTIFY places_changed),
    places_changed: qt_signal!(),

    devices: qt_property!(QVariantList; NOTIFY devices_changed),
    devices_changed: qt_signal!(),
    devices_busy: qt_property!(bool; NOTIFY devices_busy_changed),
    device_action: qt_property!(QString; NOTIFY devices_busy_changed),
    devices_busy_changed: qt_signal!(),

    properties_open: qt_property!(bool; NOTIFY properties_changed),
    properties_data: qt_property!(QVariantMap; NOTIFY properties_changed),
    properties_changed: qt_signal!(),

    bookmarks: qt_property!(QVariantList; NOTIFY bookmarks_changed),
    bookmarks_changed: qt_signal!(),

    recent_view: qt_property!(bool; NOTIFY view_changed),
    tag_collection_view: qt_property!(bool; NOTIFY view_changed),
    virtual_collection: qt_property!(bool; NOTIFY view_changed),
    tag_collection_label: qt_property!(QString; NOTIFY view_changed),

    tabs: qt_property!(QVariantList; NOTIFY tabs_changed),
    active_tab: qt_property!(i32; NOTIFY tabs_changed),
    active_tab_id: qt_property!(i32; NOTIFY tabs_changed),
    tabs_changed: qt_signal!(),

    clipboard_count: qt_property!(i32; NOTIFY clipboard_changed),
    clipboard_cut: qt_property!(bool; NOTIFY clipboard_changed),
    clipboard_paths: qt_property!(QVariantList; NOTIFY clipboard_changed),
    clipboard_changed: qt_signal!(),

    can_undo: qt_property!(bool; NOTIFY undo_changed),
    undo_label: qt_property!(QString; NOTIFY undo_changed),
    can_redo: qt_property!(bool; NOTIFY undo_changed),
    redo_label: qt_property!(QString; NOTIFY undo_changed),
    undo_changed: qt_signal!(),

    op_active: qt_property!(bool; NOTIFY op_changed),
    op_title: qt_property!(QString; NOTIFY op_changed),
    op_detail: qt_property!(QString; NOTIFY op_changed),
    op_progress: qt_property!(f64; NOTIFY op_changed),
    op_cancelable: qt_property!(bool; NOTIFY op_changed),
    op_changed: qt_signal!(),

    transfer_jobs: qt_property!(QVariantList; NOTIFY transfer_jobs_changed),
    transfer_jobs_changed: qt_signal!(),

    search_mode: qt_property!(bool; NOTIFY search_changed),
    searching: qt_property!(bool; NOTIFY search_changed),
    search_query: qt_property!(QString; NOTIFY search_changed),
    search_recursive: qt_property!(bool; NOTIFY search_changed),
    search_summary: qt_property!(QString; NOTIFY search_changed),
    search_changed: qt_signal!(),

    batch_preview_rows: qt_property!(QVariantList; NOTIFY batch_preview_changed),
    batch_preview_id: qt_property!(QString; NOTIFY batch_preview_changed),
    batch_preview_busy: qt_property!(bool; NOTIFY batch_preview_changed),
    batch_preview_error: qt_property!(QString; NOTIFY batch_preview_changed),
    batch_preview_changed: qt_signal!(),

    renaming_path: qt_property!(QString; NOTIFY renaming_changed),
    renaming_changed: qt_signal!(),

    conflict_pending: qt_property!(bool; NOTIFY conflict_changed),
    conflict_names: qt_property!(QVariantList; NOTIFY conflict_changed),
    conflict_detail: qt_property!(QString; NOTIFY conflict_changed),
    conflict_changed: qt_signal!(),

    tags: qt_property!(QVariantList; NOTIFY tags_changed),
    untagged_count: qt_property!(i32; NOTIFY tags_changed),
    tags_changed: qt_signal!(),

    in_trash: qt_property!(bool; NOTIFY view_changed),

    window_width: qt_property!(i32; NOTIFY behavior_changed),
    window_height: qt_property!(i32; NOTIFY behavior_changed),

    start: qt_method!(fn(&mut self, initial: QString)),
    refresh_devices: qt_method!(fn(&mut self)),
    mount_device: qt_method!(fn(&mut self, device: QString)),
    unlock_device: qt_method!(fn(&mut self, device: QString, passphrase: QString)),
    unmount_device: qt_method!(fn(&mut self, path: QString)),
    eject_device: qt_method!(fn(&mut self, path: QString, device: QString)),
    connect_network_location: qt_method!(fn(&mut self, uri: QString)),
    show_properties: qt_method!(fn(&mut self, path: QString)),
    show_properties_for_paths: qt_method!(fn(&mut self, paths: QVariantList)),
    request_properties: qt_method!(fn(&mut self, path: QString)),
    close_properties: qt_method!(fn(&mut self)),
    open_with_apps: qt_method!(fn(&mut self, path: QString) -> QVariantList),
    open_with_app: qt_method!(fn(&mut self, app_id: QString, paths: QVariantList)),
    default_app_name: qt_method!(fn(&self, path: QString) -> QString),
    is_archive: qt_method!(fn(&self, path: QString) -> bool),
    is_image_file: qt_method!(fn(&self, path: QString) -> bool),
    image_output_formats: qt_method!(fn(&mut self) -> QVariantList),
    convert_images: qt_method!(fn(&mut self, paths: QVariantList, format: QString)),
    compress_paths:
        qt_method!(fn(&mut self, paths: QVariantList, format: QString, password: QString)),
    extract_path: qt_method!(fn(&mut self, path: QString, destination: QString)),
    set_tag: qt_method!(fn(&mut self, paths: QVariantList, index: i32)),
    toggle_tag: qt_method!(fn(&mut self, paths: QVariantList, index: i32)),
    tag_dropped_paths: qt_method!(fn(&mut self, uri_list: QString, index: i32)),
    navigate: qt_method!(fn(&mut self, path: QString)),
    go_up: qt_method!(fn(&mut self)),
    go_back: qt_method!(fn(&mut self)),
    go_forward: qt_method!(fn(&mut self)),
    refresh: qt_method!(fn(&mut self)),
    open_index: qt_method!(fn(&mut self, index: i32)),
    open_path: qt_method!(fn(&mut self, path: QString)),
    open_in_new_tab: qt_method!(fn(&mut self, index: i32)),
    toggle_hidden: qt_method!(fn(&mut self)),
    toggle_preview: qt_method!(fn(&mut self)),
    set_view_mode: qt_method!(fn(&mut self, mode: QString)),
    set_sort: qt_method!(fn(&mut self, key: QString, ascending: bool)),
    toggle_directories_first: qt_method!(fn(&mut self)),
    set_single_click_open: qt_method!(fn(&mut self, enabled: bool)),
    toggle_sidebar: qt_method!(fn(&mut self)),
    toggle_tab_bar: qt_method!(fn(&mut self)),
    set_sidebar_width: qt_method!(fn(&mut self, width: i32)),
    set_drag_hover_delay: qt_method!(fn(&mut self, delay_ms: i32)),
    toggle_column: qt_method!(fn(&mut self, column: QString)),
    home: qt_method!(fn(&mut self)),
    trash: qt_method!(fn(&mut self)),
    clear_error: qt_method!(fn(&mut self)),
    is_dir_at: qt_method!(fn(&self, index: i32) -> bool),
    path_at: qt_method!(fn(&self, index: i32) -> QString),
    set_tree_expanded: qt_method!(fn(&mut self, index: i32, expanded: bool)),
    set_tree_path_expanded: qt_method!(fn(&mut self, path: QString, expanded: bool)),
    tree_expanded_at: qt_method!(fn(&self, index: i32) -> bool),
    tree_expandable_at: qt_method!(fn(&self, index: i32) -> bool),
    tree_depth_at: qt_method!(fn(&self, index: i32) -> i32),
    tree_parent_index: qt_method!(fn(&self, index: i32) -> i32),
    paths_in_range: qt_method!(fn(&self, first: i32, last: i32) -> QVariantList),
    size_at: qt_method!(fn(&self, index: i32) -> f64),
    name_at: qt_method!(fn(&self, index: i32) -> QString),
    index_of_path: qt_method!(fn(&self, path: QString) -> i32),
    filter_present: qt_method!(fn(&self, paths: QVariantList) -> QVariantList),
    selection_info: qt_method!(fn(&self, paths: QVariantList) -> QVariantMap),

    new_tab: qt_method!(fn(&mut self, path: QString)),
    close_tab: qt_method!(fn(&mut self, index: i32)),
    select_tab: qt_method!(fn(&mut self, index: i32)),
    duplicate_tab: qt_method!(fn(&mut self, index: i32)),
    reopen_tab: qt_method!(fn(&mut self)),
    move_tab: qt_method!(fn(&mut self, from: i32, to: i32)),
    next_tab: qt_method!(fn(&mut self)),
    prev_tab: qt_method!(fn(&mut self)),
    save_session: qt_method!(fn(&mut self)),
    save_window: qt_method!(fn(&mut self, width: i32, height: i32)),

    copy_paths: qt_method!(fn(&mut self, paths: QVariantList)),
    cut_paths: qt_method!(fn(&mut self, paths: QVariantList)),
    drop_paths: qt_method!(
        fn(
            &mut self,
            dest: QString,
            paths: QVariantList,
            move_items: bool,
            auto_move: bool,
            policy: QString,
        )
    ),
    drop_external_paths:
        qt_method!(fn(&mut self, dest: QString, urls: QVariantList, move_items: bool)),
    paste: qt_method!(fn(&mut self, dest: QString, policy: QString)),
    refresh_clipboard_state: qt_method!(fn(&mut self)),
    confirm_conflicts: qt_method!(fn(&mut self, policy: QString)),
    cancel_conflicts: qt_method!(fn(&mut self)),
    rename: qt_method!(fn(&mut self, path: QString, name: QString) -> bool),
    begin_rename: qt_method!(fn(&mut self, path: QString)),
    cancel_rename: qt_method!(fn(&mut self)),
    create_folder: qt_method!(fn(&mut self, dest: QString, name: QString) -> QString),
    create_file: qt_method!(fn(&mut self, dest: QString, name: QString) -> QString),
    suggest_name: qt_method!(fn(&self, dest: QString, base: QString) -> QString),
    request_batch_preview: qt_method!(
        fn(&mut self, paths: QVariantList, configuration: QString, excluded: QVariantList)
    ),
    clear_batch_preview: qt_method!(fn(&mut self)),
    execute_batch_rename: qt_method!(fn(&mut self, plan_id: QString) -> bool),
    trash_paths: qt_method!(fn(&mut self, paths: QVariantList)),
    restore_paths: qt_method!(fn(&mut self, paths: QVariantList)),
    empty_trash: qt_method!(fn(&mut self)),
    delete_paths: qt_method!(fn(&mut self, paths: QVariantList, confirmed: bool)),
    undo: qt_method!(fn(&mut self)),
    redo: qt_method!(fn(&mut self)),
    cancel_operation: qt_method!(fn(&mut self)),
    refresh_transfer_jobs: qt_method!(fn(&mut self)),
    flush_pending_watch_events: qt_method!(fn(&mut self)),
    transfer_job_items: qt_method!(fn(&self, id: QString) -> QVariantList),
    cancel_transfer_job: qt_method!(fn(&mut self, id: QString)),
    cancel_pending_transfers: qt_method!(fn(&mut self)),
    clear_completed_transfers: qt_method!(fn(&mut self)),
    retry_transfer_job: qt_method!(fn(&mut self, id: QString)),

    add_bookmark: qt_method!(fn(&mut self, path: QString, label: QString) -> bool),
    toggle_bookmark: qt_method!(fn(&mut self, path: QString) -> bool),
    remove_bookmark: qt_method!(fn(&mut self, index: i32)),
    rename_bookmark: qt_method!(fn(&mut self, index: i32, label: QString)),
    move_bookmark: qt_method!(fn(&mut self, index: i32, delta: i32)),
    clear_recent_history: qt_method!(fn(&mut self)),
    flush_recent_history: qt_method!(fn(&mut self)),

    search: qt_method!(fn(&mut self, query: QString, recursive: bool)),
    search_advanced: qt_method!(fn(&mut self, query: QString, filters: QString, recursive: bool)),
    clear_search: qt_method!(fn(&mut self)),
    set_search_recursive: qt_method!(fn(&mut self, recursive: bool)),
    set_search_recursive_advanced:
        qt_method!(fn(&mut self, query: QString, filters: QString, recursive: bool)),
    copy_paths_text: qt_method!(fn(&mut self, paths: QVariantList) -> bool),
    paths_uri_list: qt_method!(fn(&self, paths: QVariantList) -> QString),
    open_terminal: qt_method!(fn(&mut self, path: QString)),
    request_preview: qt_method!(fn(&mut self, path: QString)),
    request_preview_page: qt_method!(fn(&mut self, path: QString, page: i32, max_dimension: i32)),
    file_url: qt_method!(fn(&self, path: QString) -> QString),
    thumbnail_url: qt_method!(fn(&self, path: QString) -> QString),
    request_thumbnail: qt_method!(fn(&mut self, path: QString, visible: bool)),
    pause_for_background: qt_method!(fn(&mut self)),
    resume_from_background: qt_method!(fn(&mut self)),

    tabs_data: Vec<Tab>,
    active: usize,
    next_tab_id: u64,
    closed_tabs: Vec<PathBuf>,
    config: Config,
    persisted_config: Config,
    icons: Arc<IconResolver>,
    tag_store: TagStore,
    bookmark_store: BookmarkStore,
    recent_store: RecentStore,
    recent_generation: u64,
    pending_recent: HashMap<u64, PendingRecentFile>,
    next_recent_id: u64,
    clipboard: Option<Clipboard>,
    clipboard_revision: u64,
    undo_stack: Vec<UndoOp>,
    redo_stack: Vec<RedoOp>,
    op_generation: u64,
    batch_preview_generation: u64,
    batch_plan: Option<(u64, Vec<batch::BatchRow>)>,
    batch_preview_cancel: Option<Arc<AtomicBool>>,
    preview_generation: u64,
    preview_path: String,
    thumbnail_pending: HashSet<String>,
    cancel_flag: Option<Arc<AtomicBool>>,
    pending_conflict: Option<PendingConflict>,
    started: bool,
    watch_cancel: Option<Arc<AtomicBool>>,
    watch_path: Option<PathBuf>,
    watch_tab: Option<u64>,
    watch_paths: HashSet<PathBuf>,
    pending_watch_events: HashSet<(u64, PathBuf)>,
    seen_refreshes: HashSet<PathBuf>,
    properties_generation: u64,
    properties_cancel: Option<Arc<AtomicBool>>,
    device_monitor_stop: Arc<AtomicBool>,
    device_refresh_pending: bool,
    known_device_paths: HashSet<PathBuf>,
}

impl Default for Backend {
    fn default() -> Self {
        let (config, _) = Config::load();
        let mut backend = Self {
            base: Default::default(),
            current_path: Default::default(),
            current_path_changed: Default::default(),
            preview_data: QVariantMap::default(),
            preview_changed: Default::default(),
            thumbnail_ready_path: Default::default(),
            thumbnail_ready_changed: Default::default(),
            thumbnail_ready_cache_url: String::new(),
            status: QString::from("Ready"),
            status_changed: Default::default(),
            error: Default::default(),
            error_changed: Default::default(),
            loading: false,
            loading_changed: Default::default(),
            show_hidden: config.ui.show_hidden,
            preview_visible: config.ui.preview_visible,
            sort_key: QString::from(config.ui.sort_key.clone()),
            sort_ascending: config.ui.sort_ascending,
            directories_first: config.ui.directories_first,
            show_size: config.ui.show_size,
            show_modified: config.ui.show_modified,
            show_kind: config.ui.show_kind,
            view_mode: QString::from(config.ui.view_mode.clone()),
            view_changed: Default::default(),
            single_click_open: config.ui.single_click_open,
            sidebar_visible: config.ui.sidebar_visible,
            sidebar_width: config.ui.sidebar_width,
            tab_bar_visible: config.ui.tab_bar_visible,
            drag_hover_delay_ms: config.ui.drag_hover_delay_ms.min(10_000) as i32,
            terminal: QString::from(config.ui.terminal.clone()),
            behavior_changed: Default::default(),
            item_count: 0,
            item_count_changed: Default::default(),
            can_go_back: false,
            can_go_forward: false,
            nav_changed: Default::default(),
            breadcrumbs: Default::default(),
            breadcrumbs_changed: Default::default(),
            places: Default::default(),
            places_changed: Default::default(),
            devices: Default::default(),
            devices_changed: Default::default(),
            devices_busy: false,
            device_action: Default::default(),
            devices_busy_changed: Default::default(),
            properties_open: false,
            properties_data: Default::default(),
            properties_changed: Default::default(),
            bookmarks: Default::default(),
            bookmarks_changed: Default::default(),
            recent_view: false,
            tag_collection_view: false,
            virtual_collection: false,
            tag_collection_label: QString::from(""),
            tabs: Default::default(),
            active_tab: 0,
            active_tab_id: 0,
            tabs_changed: Default::default(),
            clipboard_count: 0,
            clipboard_cut: false,
            clipboard_paths: Default::default(),
            clipboard_changed: Default::default(),
            can_undo: false,
            undo_label: Default::default(),
            can_redo: false,
            redo_label: Default::default(),
            undo_changed: Default::default(),
            op_active: false,
            op_title: Default::default(),
            op_detail: Default::default(),
            op_progress: 0.0,
            op_cancelable: false,
            op_changed: Default::default(),
            transfer_jobs: QVariantList::default(),
            transfer_jobs_changed: Default::default(),
            search_mode: false,
            searching: false,
            search_query: Default::default(),
            search_recursive: config.ui.search_recursive,
            search_summary: Default::default(),
            search_changed: Default::default(),
            batch_preview_rows: QVariantList::default(),
            batch_preview_id: Default::default(),
            batch_preview_busy: false,
            batch_preview_error: Default::default(),
            batch_preview_changed: Default::default(),
            renaming_path: Default::default(),
            renaming_changed: Default::default(),
            conflict_pending: false,
            conflict_names: Default::default(),
            conflict_detail: Default::default(),
            conflict_changed: Default::default(),
            tags: Default::default(),
            untagged_count: 0,
            tags_changed: Default::default(),
            in_trash: false,
            window_width: config.ui.window_width,
            window_height: config.ui.window_height,
            start: Default::default(),
            refresh_devices: Default::default(),
            mount_device: Default::default(),
            unlock_device: Default::default(),
            unmount_device: Default::default(),
            eject_device: Default::default(),
            connect_network_location: Default::default(),
            show_properties: Default::default(),
            show_properties_for_paths: Default::default(),
            request_properties: Default::default(),
            close_properties: Default::default(),
            open_with_apps: Default::default(),
            open_with_app: Default::default(),
            default_app_name: Default::default(),
            is_archive: Default::default(),
            is_image_file: Default::default(),
            image_output_formats: Default::default(),
            convert_images: Default::default(),
            compress_paths: Default::default(),
            extract_path: Default::default(),
            set_tag: Default::default(),
            toggle_tag: Default::default(),
            tag_dropped_paths: Default::default(),
            navigate: Default::default(),
            go_up: Default::default(),
            go_back: Default::default(),
            go_forward: Default::default(),
            refresh: Default::default(),
            open_index: Default::default(),
            open_path: Default::default(),
            open_in_new_tab: Default::default(),
            toggle_hidden: Default::default(),
            toggle_preview: Default::default(),
            set_view_mode: Default::default(),
            set_sort: Default::default(),
            toggle_directories_first: Default::default(),
            set_single_click_open: Default::default(),
            toggle_sidebar: Default::default(),
            toggle_tab_bar: Default::default(),
            set_sidebar_width: Default::default(),
            set_drag_hover_delay: Default::default(),
            toggle_column: Default::default(),
            home: Default::default(),
            trash: Default::default(),
            clear_error: Default::default(),
            is_dir_at: Default::default(),
            path_at: Default::default(),
            set_tree_expanded: Default::default(),
            set_tree_path_expanded: Default::default(),
            tree_expanded_at: Default::default(),
            tree_expandable_at: Default::default(),
            tree_depth_at: Default::default(),
            tree_parent_index: Default::default(),
            paths_in_range: Default::default(),
            size_at: Default::default(),
            name_at: Default::default(),
            index_of_path: Default::default(),
            filter_present: Default::default(),
            selection_info: Default::default(),
            new_tab: Default::default(),
            close_tab: Default::default(),
            select_tab: Default::default(),
            duplicate_tab: Default::default(),
            reopen_tab: Default::default(),
            move_tab: Default::default(),
            next_tab: Default::default(),
            prev_tab: Default::default(),
            save_session: Default::default(),
            save_window: Default::default(),
            copy_paths: Default::default(),
            cut_paths: Default::default(),
            drop_paths: Default::default(),
            drop_external_paths: Default::default(),
            paste: Default::default(),
            refresh_clipboard_state: Default::default(),
            confirm_conflicts: Default::default(),
            cancel_conflicts: Default::default(),
            rename: Default::default(),
            begin_rename: Default::default(),
            cancel_rename: Default::default(),
            create_folder: Default::default(),
            create_file: Default::default(),
            suggest_name: Default::default(),
            request_batch_preview: Default::default(),
            clear_batch_preview: Default::default(),
            execute_batch_rename: Default::default(),
            trash_paths: Default::default(),
            restore_paths: Default::default(),
            empty_trash: Default::default(),
            delete_paths: Default::default(),
            undo: Default::default(),
            redo: Default::default(),
            cancel_operation: Default::default(),
            refresh_transfer_jobs: Default::default(),
            flush_pending_watch_events: Default::default(),
            transfer_job_items: Default::default(),
            cancel_transfer_job: Default::default(),
            cancel_pending_transfers: Default::default(),
            clear_completed_transfers: Default::default(),
            retry_transfer_job: Default::default(),
            add_bookmark: Default::default(),
            toggle_bookmark: Default::default(),
            remove_bookmark: Default::default(),
            rename_bookmark: Default::default(),
            move_bookmark: Default::default(),
            clear_recent_history: Default::default(),
            flush_recent_history: Default::default(),
            search: Default::default(),
            search_advanced: Default::default(),
            clear_search: Default::default(),
            set_search_recursive: Default::default(),
            set_search_recursive_advanced: Default::default(),
            copy_paths_text: Default::default(),
            paths_uri_list: Default::default(),
            open_terminal: Default::default(),
            request_preview: Default::default(),
            request_preview_page: Default::default(),
            file_url: Default::default(),
            thumbnail_url: Default::default(),
            request_thumbnail: Default::default(),
            pause_for_background: Default::default(),
            resume_from_background: Default::default(),
            tabs_data: Vec::new(),
            active: 0,
            next_tab_id: 1,
            closed_tabs: Vec::new(),
            config: config.clone(),
            persisted_config: config.clone(),
            icons: IconResolver::shared(),
            tag_store: TagStore::default(),
            bookmark_store: BookmarkStore::default(),
            recent_store: RecentStore::default(),
            recent_generation: 0,
            pending_recent: HashMap::new(),
            next_recent_id: 1,
            clipboard: None,
            clipboard_revision: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            op_generation: 0,
            batch_preview_generation: 0,
            batch_plan: None,
            batch_preview_cancel: None,
            preview_generation: 0,
            preview_path: String::new(),
            thumbnail_pending: HashSet::new(),
            cancel_flag: None,
            pending_conflict: None,
            started: false,
            watch_cancel: None,
            watch_path: None,
            watch_tab: None,
            watch_paths: HashSet::new(),
            pending_watch_events: HashSet::new(),
            seen_refreshes: HashSet::new(),
            properties_generation: 0,
            properties_cancel: None,
            device_monitor_stop: Arc::new(AtomicBool::new(false)),
            device_refresh_pending: false,
            known_device_paths: HashSet::new(),
        };
        backend
            .tabs_data
            .push(Tab::new(1, home_dir(), config.sort_spec()));
        backend.next_tab_id = 2;
        backend
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.device_monitor_stop.store(true, Ordering::Release);
    }
}

impl QAbstractListModel for Backend {
    fn row_count(&self) -> i32 {
        self.active_entries().len().min(i32::MAX as usize) as i32
    }

    fn data(&self, index: QModelIndex, role: i32) -> QVariant {
        let row = index.row();
        let entries = self.active_entries();
        if row < 0 || row as usize >= entries.len() {
            return QVariant::default();
        }
        let entry = &entries[row as usize];
        match role {
            ROLE_NAME => QVariant::from(QString::from(entry.name.clone())),
            ROLE_PATH => QVariant::from(QString::from(entry.path.to_string_lossy().into_owned())),
            ROLE_IS_DIR => QVariant::from(entry.is_dir),
            ROLE_SIZE => QVariant::from(entry.size as f64),
            ROLE_SIZE_TEXT => QVariant::from(QString::from(entry.size_text())),
            ROLE_MODIFIED_MS => QVariant::from(entry.modified_ms),
            ROLE_MODIFIED_TEXT => QVariant::from(QString::from(entry.modified_text())),
            ROLE_KIND => QVariant::from(QString::from(entry.kind.clone())),
            ROLE_ICON_URL => QVariant::from(QString::from(entry.icon_url.clone())),
            ROLE_IS_SYMLINK => QVariant::from(entry.is_symlink),
            ROLE_IS_HIDDEN => QVariant::from(entry.is_hidden),
            ROLE_EXTENSION => QVariant::from(QString::from(entry.extension.clone())),
            ROLE_TAG => QVariant::from(entry.tag),
            ROLE_TAGS => {
                let mut tags = QVariantList::default();
                for tag in self.tag_store.tags_for(&entry.path, entry.dev, entry.ino) {
                    tags.push(QVariant::from(tag));
                }
                QVariant::from(tags)
            }
            ROLE_DISPLAY_PATH => QVariant::from(QString::from(entry.display_path.clone())),
            ROLE_TREE_DEPTH => QVariant::from(entry.tree_depth as i32),
            ROLE_TREE_EXPANDED => QVariant::from(entry.tree_expanded),
            ROLE_TREE_EXPANDABLE => QVariant::from(entry.is_dir && !entry.is_symlink),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> HashMap<i32, QByteArray> {
        HashMap::from([
            (ROLE_NAME, QByteArray::from("name")),
            (ROLE_PATH, QByteArray::from("path")),
            (ROLE_IS_DIR, QByteArray::from("isDir")),
            (ROLE_SIZE, QByteArray::from("size")),
            (ROLE_SIZE_TEXT, QByteArray::from("sizeText")),
            (ROLE_MODIFIED_MS, QByteArray::from("modifiedMs")),
            (ROLE_MODIFIED_TEXT, QByteArray::from("modifiedText")),
            (ROLE_KIND, QByteArray::from("kind")),
            (ROLE_ICON_URL, QByteArray::from("iconUrl")),
            (ROLE_IS_SYMLINK, QByteArray::from("isSymlink")),
            (ROLE_IS_HIDDEN, QByteArray::from("isHidden")),
            (ROLE_EXTENSION, QByteArray::from("extension")),
            (ROLE_TAG, QByteArray::from("tag")),
            (ROLE_TAGS, QByteArray::from("tags")),
            (ROLE_DISPLAY_PATH, QByteArray::from("displayPath")),
            (ROLE_TREE_DEPTH, QByteArray::from("treeDepth")),
            (ROLE_TREE_EXPANDED, QByteArray::from("treeExpanded")),
            (ROLE_TREE_EXPANDABLE, QByteArray::from("treeExpandable")),
        ])
    }
}

fn volume_maps(volumes: Vec<volumes::Volume>, icons: &IconResolver) -> QVariantList {
    volumes
        .into_iter()
        .map(|volume| {
            let capacity = volumes::capacity_text(&volume);
            let mounted = volume.path.is_some();
            let can_unmount = volume.can_unmount();
            let can_eject = volume.can_eject();
            let icon_names = if volume.system && volume.path.as_deref() == Some(Path::new("/")) {
                &[
                    "drive-harddisk-root",
                    "drive-harddisk-system",
                    "computer",
                    "drive-harddisk",
                ][..]
            } else if volume.system {
                &["drive-harddisk-system", "drive-harddisk", "computer"][..]
            } else if volume.network {
                &["network-server", "folder-remote"][..]
            } else if volume.filesystem.eq_ignore_ascii_case("phone") {
                &["phone", "phone-symbolic", "drive-removable-media"][..]
            } else if volume.filesystem.eq_ignore_ascii_case("camera") {
                &["camera-photo", "camera", "drive-removable-media"][..]
            } else if volume.encrypted {
                &["drive-harddisk-encrypted", "drive-harddisk"][..]
            } else if volume.filesystem.to_lowercase().contains("iso")
                || volume.filesystem.to_lowercase().contains("udf")
            {
                &["drive-optical", "drive-removable-media"][..]
            } else if volume.removable {
                &[
                    "drive-removable-media-usb-pendrive",
                    "drive-removable-media-usb",
                    "drive-removable-media",
                    "drive-harddisk",
                ][..]
            } else {
                &["drive-harddisk", "drive-harddisk-symbolic"][..]
            };
            let icon = icons.resolve_first(icon_names).unwrap_or_default();
            let capacity_ratio = if mounted && volume.total > 0 {
                (volume.total.saturating_sub(volume.free) as f64 / volume.total as f64)
                    .clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut map = QVariantMap::default();
            map.insert(
                QString::from("label"),
                QVariant::from(QString::from(volume.label)),
            );
            map.insert(
                QString::from("path"),
                QVariant::from(QString::from(
                    volume
                        .path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )),
            );
            map.insert(
                QString::from("device"),
                QVariant::from(QString::from(volume.device)),
            );
            map.insert(
                QString::from("ejectDevice"),
                QVariant::from(QString::from(volume.eject_device)),
            );
            map.insert(
                QString::from("filesystem"),
                QVariant::from(QString::from(volume.filesystem)),
            );
            map.insert(
                QString::from("capacity"),
                QVariant::from(QString::from(capacity)),
            );
            map.insert(
                QString::from("capacityRatio"),
                QVariant::from(capacity_ratio),
            );
            map.insert(
                QString::from("iconUrl"),
                QVariant::from(QString::from(icon)),
            );
            map.insert(QString::from("mounted"), QVariant::from(mounted));
            map.insert(QString::from("removable"), QVariant::from(volume.removable));
            map.insert(QString::from("usbFlash"), QVariant::from(volume.usb_flash));
            map.insert(QString::from("network"), QVariant::from(volume.network));
            map.insert(QString::from("encrypted"), QVariant::from(volume.encrypted));
            map.insert(QString::from("locked"), QVariant::from(volume.locked));
            map.insert(QString::from("system"), QVariant::from(volume.system));
            map.insert(QString::from("canUnmount"), QVariant::from(can_unmount));
            map.insert(QString::from("canEject"), QVariant::from(can_eject));
            QVariant::from(map)
        })
        .collect()
}

fn property_map(properties: ops::Properties, report: MetadataReport) -> QVariantMap {
    let rows = property_field_rows(&properties, &report);
    let mut map = QVariantMap::default();
    map.insert(
        QString::from("name"),
        QVariant::from(QString::from(properties.name)),
    );
    map.insert(
        QString::from("extension"),
        QVariant::from(QString::from(properties.extension)),
    );
    map.insert(
        QString::from("path"),
        QVariant::from(QString::from(
            properties.path.to_string_lossy().into_owned(),
        )),
    );
    map.insert(
        QString::from("kind"),
        QVariant::from(QString::from(properties.kind)),
    );
    map.insert(
        QString::from("size"),
        QVariant::from(QString::from(properties.size_text)),
    );
    map.insert(
        QString::from("sizeBytes"),
        QVariant::from(properties.size_bytes as f64),
    );
    map.insert(
        QString::from("allocatedBytes"),
        QVariant::from(QString::from(properties.allocated_bytes.to_string())),
    );
    map.insert(
        QString::from("mode"),
        QVariant::from(QString::from(properties.mode)),
    );
    map.insert(
        QString::from("owner"),
        QVariant::from(QString::from(format!(
            "{} ({}) / {} ({})",
            properties.owner, properties.uid, properties.group, properties.gid
        ))),
    );
    map.insert(
        QString::from("items"),
        QVariant::from(QString::from(
            properties
                .items
                .map(|count| {
                    format!(
                        "{count} items{}",
                        if properties.truncated {
                            " (scan capped)"
                        } else {
                            ""
                        }
                    )
                })
                .unwrap_or_default(),
        )),
    );
    map.insert(
        QString::from("linkTarget"),
        QVariant::from(QString::from(
            properties
                .link_target
                .map(|target| target.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )),
    );
    map.insert(
        QString::from("modified"),
        QVariant::from(QString::from(if properties.modified_ms > 0.0 {
            format!("{:.0}", properties.modified_ms)
        } else {
            "Unknown".to_owned()
        })),
    );
    map.insert(
        QString::from("accessed"),
        QVariant::from(QString::from(if properties.accessed_ms > 0.0 {
            format!("{:.0}", properties.accessed_ms)
        } else {
            "Unknown".to_owned()
        })),
    );
    map.insert(
        QString::from("device"),
        QVariant::from(QString::from(properties.device.to_string())),
    );
    map.insert(
        QString::from("inode"),
        QVariant::from(QString::from(properties.inode.to_string())),
    );
    map.insert(
        QString::from("created"),
        QVariant::from(QString::from(timestamp_text(properties.created_ms))),
    );
    map.insert(
        QString::from("isSymlink"),
        QVariant::from(properties.is_symlink),
    );
    map.insert(
        QString::from("isDirectory"),
        QVariant::from(properties.is_directory),
    );
    map.insert(
        QString::from("immediateFiles"),
        QVariant::from(QString::from(
            properties
                .immediate_files
                .map(|count| count.to_string())
                .unwrap_or_default(),
        )),
    );
    map.insert(
        QString::from("immediateFolders"),
        QVariant::from(QString::from(
            properties
                .immediate_folders
                .map(|count| count.to_string())
                .unwrap_or_default(),
        )),
    );
    map.insert(
        QString::from("mimeType"),
        QVariant::from(QString::from(report.mime_type)),
    );
    map.insert(
        QString::from("actualType"),
        QVariant::from(QString::from(report.actual_type)),
    );
    let mut fields = QVariantList::default();
    for field in rows {
        let mut item = QVariantMap::default();
        item.insert(
            QString::from("section"),
            QVariant::from(QString::from(field.section)),
        );
        item.insert(
            QString::from("label"),
            QVariant::from(QString::from(field.label)),
        );
        item.insert(
            QString::from("value"),
            QVariant::from(QString::from(field.value)),
        );
        fields.push(QVariant::from(item));
    }
    map.insert(QString::from("details"), QVariant::from(fields));
    let mut warnings = QVariantList::default();
    for warning in report.warnings {
        warnings.push(QVariant::from(QString::from(warning)));
    }
    map.insert(QString::from("warnings"), QVariant::from(warnings));
    map
}

fn property_map_many(snapshots: Vec<PropertySnapshot>) -> QVariantMap {
    if snapshots.len() == 1 {
        let snapshot = snapshots.into_iter().next().expect("single snapshot");
        let tag_name = snapshot.tags.join(", ");
        let icon_url = snapshot.icon_url.clone();
        let mut map = property_map(snapshot.properties, snapshot.report);
        map.insert(
            QString::from("iconUrl"),
            QVariant::from(QString::from(icon_url)),
        );
        map.insert(
            QString::from("tagName"),
            QVariant::from(QString::from(if tag_name.is_empty() {
                "None".to_owned()
            } else {
                tag_name
            })),
        );
        map.insert(QString::from("selectedCount"), QVariant::from(1));
        return map;
    }

    let count = snapshots.len();
    let icon_url = snapshots
        .iter()
        .map(|snapshot| snapshot.icon_url.as_str())
        .find(|url| !url.is_empty())
        .unwrap_or("")
        .to_owned();
    let mut values: BTreeMap<(String, String), Vec<Option<String>>> = BTreeMap::new();
    let mut warnings = HashSet::new();
    let mut kinds = HashSet::new();
    let mut locations = HashSet::new();
    for (index, snapshot) in snapshots.iter().enumerate() {
        kinds.insert(snapshot.properties.kind.clone());
        locations.insert(snapshot.properties.path.to_string_lossy().into_owned());
        for field in property_field_rows(&snapshot.properties, &snapshot.report) {
            values
                .entry((field.section, field.label))
                .or_insert_with(|| vec![None; count])[index] = Some(field.value);
        }
        warnings.extend(snapshot.report.warnings.iter().cloned());
    }

    let mut fields = QVariantList::default();
    for ((section, label), entries) in values {
        let unique = entries.iter().flatten().collect::<HashSet<_>>();
        let value = if unique.len() == 1 && entries.iter().all(Option::is_some) {
            unique.into_iter().next().cloned().unwrap_or_default()
        } else {
            "Multiple values".to_owned()
        };
        let mut field = QVariantMap::default();
        field.insert(
            QString::from("section"),
            QVariant::from(QString::from(section)),
        );
        field.insert(QString::from("label"), QVariant::from(QString::from(label)));
        field.insert(QString::from("value"), QVariant::from(QString::from(value)));
        fields.push(QVariant::from(field));
    }
    let mut warning_list = QVariantList::default();
    for warning in warnings {
        warning_list.push(QVariant::from(QString::from(warning)));
    }
    let mut map = QVariantMap::default();
    map.insert(
        QString::from("name"),
        QVariant::from(QString::from(format!("{count} selected items"))),
    );
    map.insert(
        QString::from("kind"),
        QVariant::from(QString::from(if kinds.len() == 1 {
            kinds.into_iter().next().unwrap_or_default()
        } else {
            "Multiple types".to_owned()
        })),
    );
    map.insert(
        QString::from("path"),
        QVariant::from(QString::from(if locations.len() == 1 {
            locations.into_iter().next().unwrap_or_default()
        } else {
            "Multiple locations".to_owned()
        })),
    );
    map.insert(
        QString::from("iconUrl"),
        QVariant::from(QString::from(icon_url)),
    );
    map.insert(QString::from("selectedCount"), QVariant::from(count as i32));
    map.insert(QString::from("details"), QVariant::from(fields));
    map.insert(QString::from("warnings"), QVariant::from(warning_list));
    map
}

fn property_field_rows(
    properties: &ops::Properties,
    report: &MetadataReport,
) -> Vec<metadata::MetadataField> {
    let mut rows = vec![
        ("General", "Name", properties.name.clone()),
        ("General", "Type", properties.kind.clone()),
        ("General", "Extension", properties.extension.clone()),
        ("General", "MIME type", report.mime_type.clone()),
        ("General", "Detected type", report.actual_type.clone()),
        ("General", "Size", properties.size_text.clone()),
        (
            "General",
            "Allocated size",
            format_size(properties.allocated_bytes),
        ),
        (
            "General",
            "Modified",
            timestamp_text(properties.modified_ms),
        ),
        (
            "General",
            "Accessed",
            timestamp_text(properties.accessed_ms),
        ),
        ("General", "Created", timestamp_text(properties.created_ms)),
        ("Permissions", "Permissions", properties.mode.clone()),
        (
            "Permissions",
            "Owner",
            format!("{} ({})", properties.owner, properties.uid),
        ),
        (
            "Permissions",
            "Group",
            format!("{} ({})", properties.group, properties.gid),
        ),
        (
            "Location",
            "Full path",
            properties.path.to_string_lossy().into_owned(),
        ),
        ("Location", "Device", properties.device.to_string()),
        ("Location", "Inode", properties.inode.to_string()),
    ]
    .into_iter()
    .map(|(section, label, value)| metadata::MetadataField {
        section: section.to_owned(),
        label: label.to_owned(),
        value,
    })
    .collect::<Vec<_>>();
    if properties.is_directory {
        rows.extend([
            metadata::MetadataField {
                section: "Type-specific Details".to_owned(),
                label: "Immediate files".to_owned(),
                value: properties
                    .immediate_files
                    .map(|count| count.to_string())
                    .unwrap_or_else(|| "Unavailable".to_owned()),
            },
            metadata::MetadataField {
                section: "Type-specific Details".to_owned(),
                label: "Immediate folders".to_owned(),
                value: properties
                    .immediate_folders
                    .map(|count| count.to_string())
                    .unwrap_or_else(|| "Unavailable".to_owned()),
            },
            metadata::MetadataField {
                section: "Type-specific Details".to_owned(),
                label: "Recursive items".to_owned(),
                value: properties
                    .items
                    .map(|count| {
                        format!(
                            "{count}{}",
                            if properties.truncated {
                                " (limit reached)"
                            } else {
                                ""
                            }
                        )
                    })
                    .unwrap_or_else(|| "Unavailable".to_owned()),
            },
            metadata::MetadataField {
                section: "Type-specific Details".to_owned(),
                label: "Recursive size".to_owned(),
                value: properties.size_text.clone(),
            },
        ]);
    }
    if properties.is_symlink {
        rows.push(metadata::MetadataField {
            section: "Location".to_owned(),
            label: "Symbolic link target".to_owned(),
            value: properties
                .link_target
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Unavailable".to_owned()),
        });
    }
    rows.extend(report.fields.iter().cloned());
    rows
}

fn timestamp_text(milliseconds: f64) -> String {
    if milliseconds > 0.0 {
        format!("{milliseconds:.0}")
    } else {
        String::new()
    }
}
fn desktop_app_name(app_id: &str) -> String {
    for dir in xdg_application_dirs() {
        let Ok(file) = find_desktop_file_in(&dir, app_id) else {
            continue;
        };
        let Ok(contents) = fs::read_to_string(file) else {
            continue;
        };
        let in_entry = contents
            .split("[Desktop Entry]")
            .nth(1)
            .and_then(|entry| entry.split("\n[").next())
            .unwrap_or("");
        if let Some(name) = in_entry
            .lines()
            .find_map(|line| line.strip_prefix("Name=").filter(|name| !name.is_empty()))
        {
            return name.to_owned();
        }
    }
    app_id
        .strip_suffix(".desktop")
        .unwrap_or(app_id)
        .replace(['-', '_'], " ")
}

fn find_desktop_file_in(applications_dir: &Path, app_id: &str) -> Result<PathBuf, String> {
    fn find(directory: &Path, root: &Path, app_id: &str) -> Option<PathBuf> {
        for entry in fs::read_dir(directory).ok()?.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if let Some(found) = find(&path, root, app_id) {
                    return Some(found);
                }
            } else if (file_type.is_file() || path.is_file())
                && path
                    .extension()
                    .is_some_and(|extension| extension == "desktop")
                && path
                    .strip_prefix(root)
                    .ok()
                    .is_some_and(|relative| desktop_file_id(relative) == app_id)
            {
                return Some(path);
            }
        }
        None
    }

    find(applications_dir, applications_dir, app_id)
        .ok_or_else(|| format!("Application descriptor not found: {app_id}"))
}

fn absolute_paths(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let current_dir = std::env::current_dir()
        .map_err(|error| format!("Could not resolve the selected file paths: {error}"))?;
    Ok(paths
        .into_iter()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                current_dir.join(path)
            }
        })
        .collect())
}

fn default_open_command(paths: &[PathBuf]) -> Command {
    let mut command = Command::new("gio");
    command.arg("open").args(paths);
    command
}

fn open_with_command(desktop_file: &Path, paths: &[PathBuf]) -> Command {
    let mut command = Command::new("gio");
    command.arg("launch").arg(desktop_file).args(paths);
    command
}

impl Backend {
    fn show_properties(&mut self, path: QString) {
        self.load_properties_for_paths(vec![PathBuf::from(path.to_string())], true);
    }

    fn show_properties_for_paths(&mut self, paths: QVariantList) {
        self.load_properties_for_paths(list_paths(&paths), true);
    }

    fn request_properties(&mut self, path: QString) {
        self.load_properties_for_paths(vec![PathBuf::from(path.to_string())], false);
    }

    fn load_properties_for_paths(&mut self, paths: Vec<PathBuf>, show_panel: bool) {
        if paths.is_empty() {
            self.set_error("Select at least one item to inspect.");
            return;
        }
        if let Some(cancel) = self.properties_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.properties_generation = self.properties_generation.wrapping_add(1);
        let generation = self.properties_generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.properties_cancel = Some(cancel.clone());
        let qptr = QPointer::from(&*self);
        let callback =
            qmetaobject::queued_callback(move |result: Result<Vec<PropertySnapshot>, String>| {
                if let Some(backend) = qptr.as_pinned() {
                    let mut backend = backend.borrow_mut();
                    if generation != backend.properties_generation {
                        return;
                    }
                    backend.properties_cancel = None;
                    match result {
                        Ok(snapshots) => {
                            backend.properties_data = property_map_many(snapshots);
                            if show_panel {
                                backend.properties_open = true;
                            }
                            backend.properties_changed();
                        }
                        Err(error) => backend.set_error(error),
                    }
                }
            });
        let tags = self.tag_store.clone();
        std::thread::spawn(move || {
            let result = paths
                .into_iter()
                .map(|path| {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("Metadata extraction was cancelled.".to_owned());
                    }
                    let job = metadata::begin_job(&cancel)?;
                    let properties = ops::properties_with_cancel(&path, &cancel)?;
                    let report = metadata::extract_with_job(&path, &cancel, &job)?;
                    use std::os::unix::fs::MetadataExt;
                    let metadata = fs::metadata(&path).or_else(|_| fs::symlink_metadata(&path));
                    let tags_for_path = metadata
                        .as_ref()
                        .map(|metadata| {
                            tags.tags_for(&path, metadata.dev(), metadata.ino())
                                .into_iter()
                                .map(|index| tags.name(index as usize).to_owned())
                                .collect()
                        })
                        .unwrap_or_default();
                    let icons = IconResolver::shared();
                    let icon_url = icons
                        .resolve_first(&icon_names_for(
                            &properties.name,
                            properties.is_directory,
                            &properties.extension,
                        ))
                        .unwrap_or_default();
                    Ok(PropertySnapshot {
                        properties,
                        report,
                        tags: tags_for_path,
                        icon_url,
                    })
                })
                .collect::<Result<Vec<_>, String>>();
            callback(result);
        });
    }

    fn close_properties(&mut self) {
        if let Some(cancel) = self.properties_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.properties_generation = self.properties_generation.wrapping_add(1);
        self.properties_open = false;
        self.properties_changed();
    }

    fn open_with_apps(&mut self, path: QString) -> QVariantList {
        let path = PathBuf::from(path.to_string());
        let output = match Command::new("gio")
            .args(["info", "--attributes=standard::content-type"])
            .arg(&path)
            .output()
        {
            Ok(output) => output,
            Err(error) => {
                self.set_error(format!("Could not find compatible applications: {error}"));
                return QVariantList::default();
            }
        };
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            self.set_error(if detail.is_empty() {
                format!("Could not determine the file type for {}.", path.display())
            } else {
                format!("Could not determine the file type: {detail}")
            });
            return QVariantList::default();
        }
        let info = String::from_utf8_lossy(&output.stdout);
        let Some(mime) = info
            .lines()
            .find_map(|line| line.trim().strip_prefix("standard::content-type: "))
        else {
            self.set_error(format!(
                "Could not determine the file type for {}.",
                path.display()
            ));
            return QVariantList::default();
        };
        let output = match Command::new("gio").args(["mime", mime]).output() {
            Ok(output) => output,
            Err(error) => {
                self.set_error(format!("Could not list compatible applications: {error}"));
                return QVariantList::default();
            }
        };
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            self.set_error(if detail.is_empty() {
                "Could not list compatible applications.".to_owned()
            } else {
                format!("Could not list compatible applications: {detail}")
            });
            return QVariantList::default();
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut in_apps = false;
        let mut ids = HashSet::new();
        let mut list = QVariantList::default();
        for line in text.lines() {
            let line = line.trim();
            if line == "Registered applications:" {
                in_apps = true;
                continue;
            }
            if line.ends_with("applications:") {
                in_apps = false;
            }
            if !in_apps || !line.ends_with(".desktop") || !ids.insert(line.to_owned()) {
                continue;
            }
            let mut map = QVariantMap::default();
            map.insert(
                QString::from("id"),
                QVariant::from(QString::from(line.to_owned())),
            );
            map.insert(
                QString::from("name"),
                QVariant::from(QString::from(desktop_app_name(line))),
            );
            list.push(QVariant::from(map));
        }
        list
    }

    fn default_app_name(&self, path: QString) -> QString {
        let path = PathBuf::from(path.to_string());
        let content_type = Command::new("gio")
            .args(["info", "--attributes=standard::content-type"])
            .arg(&path)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .find_map(|line| {
                        line.trim()
                            .strip_prefix("standard::content-type: ")
                            .map(str::to_owned)
                    })
            });
        let app_id = content_type.and_then(|mime| {
            let output = Command::new("gio").args(["mime", &mime]).output().ok()?;
            if !output.status.success() {
                return None;
            }
            default_application_id(&String::from_utf8_lossy(&output.stdout)).map(str::to_owned)
        });
        QString::from(app_id.as_deref().map(desktop_app_name).unwrap_or_default())
    }

    fn open_with_app(&mut self, app_id: QString, paths: QVariantList) {
        let app_id = app_id.to_string();
        if app_id.contains('/') || app_id.contains('\n') || !app_id.ends_with(".desktop") {
            self.set_error("Invalid application selection.");
            return;
        }
        let paths = match absolute_paths(list_paths(&paths)) {
            Ok(paths) => paths,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        if paths.is_empty() {
            self.set_error("No files were selected to open.");
            return;
        }
        let desktop_file = match find_desktop_file(&app_id) {
            Ok(path) => path,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        self.launch_files_async(
            open_with_command(&desktop_file, &paths),
            paths,
            "Could not launch the selected application".to_owned(),
        );
    }

    fn launch_files_async(
        &mut self,
        mut command: Command,
        paths: Vec<PathBuf>,
        error_context: String,
    ) {
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: Result<(), String>| {
            if let Some(backend) = qptr.as_pinned() {
                let mut backend = backend.borrow_mut();
                match result {
                    Ok(()) => {
                        for path in &paths {
                            backend.record_recent_file(path);
                        }
                    }
                    Err(error) => backend.set_error(format!("{error_context}: {error}")),
                }
            }
        });
        std::thread::spawn(move || {
            let result = command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .and_then(|mut child| child.wait())
                .map_err(|error| error.to_string())
                .and_then(|status| {
                    if status.success() {
                        Ok(())
                    } else {
                        Err(format!("launcher exited with status {status}"))
                    }
                });
            callback(result);
        });
    }

    fn is_archive(&self, path: QString) -> bool {
        ops::archive_format(Path::new(&path.to_string())).is_some()
    }

    fn is_image_file(&self, path: QString) -> bool {
        image_conversion::is_image_file(Path::new(&path.to_string()))
    }

    fn image_output_formats(&mut self) -> QVariantList {
        match image_conversion::available_formats() {
            Ok(formats) => {
                let mut list = QVariantList::default();
                for format in formats {
                    list.push(QVariant::from(QString::from(format)));
                }
                list
            }
            Err(error) => {
                self.set_error(error);
                QVariantList::default()
            }
        }
    }

    fn convert_images(&mut self, paths: QVariantList, format: QString) {
        let sources = list_paths(&paths);
        if sources.is_empty() {
            self.set_error("Select at least one image to convert.");
            return;
        }
        let format = format.to_string();
        self.run_archive_action(
            format!("Converting {} images", sources.len()),
            move |cancel, mut progress| {
                image_conversion::convert_images(&sources, &format, &cancel, &mut *progress)
            },
        );
    }

    fn run_archive_action(
        &mut self,
        title: String,
        action: impl FnOnce(Arc<AtomicBool>, OperationProgress) -> Result<String, String>
            + Send
            + 'static,
    ) {
        if self.non_transfer_operation_busy() {
            self.set_error("Wait for the current file operation to finish first.");
            return;
        }
        let generation = self.begin_operation(title, true);
        let cancel = self
            .cancel_flag
            .clone()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: Result<String, String>| {
            if let Some(backend) = qptr.as_pinned() {
                let mut backend = backend.borrow_mut();
                if generation != backend.op_generation {
                    return;
                }
                backend.finish_operation();
                match result {
                    Ok(message) => {
                        notifications::send("File operation complete", message.clone());
                        backend.status = QString::from(message);
                        backend.status_changed();
                        backend.refresh();
                    }
                    Err(error) => {
                        let title = if error.to_ascii_lowercase().contains("cancel") {
                            "File operation cancelled"
                        } else {
                            "File operation failed"
                        };
                        notifications::send(title, error.clone());
                        backend.set_error(error);
                    }
                }
            }
        });
        let qptr = QPointer::from(&*self);
        let tick_callback = qmetaobject::queued_callback(move |tick: OpTick| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_op_tick(tick);
            }
        });
        let mut last_update: Option<Instant> = None;
        let progress: OperationProgress = Box::new(move |detail, progress| {
            let now = Instant::now();
            if progress < 1.0
                && last_update
                    .is_some_and(|last| now.duration_since(last) < Duration::from_millis(80))
            {
                return;
            }
            last_update = Some(now);
            tick_callback(OpTick {
                generation,
                detail,
                progress: progress.clamp(0.0, 1.0),
            });
        });
        std::thread::spawn(move || callback(action(cancel, progress)));
    }

    fn compress_paths(&mut self, paths: QVariantList, format: QString, password: QString) {
        let sources = list_paths(&paths);
        if is_virtual_collection(&self.current_dir()) {
            self.set_error("Choose a destination folder before compressing collection items.");
            return;
        }
        let dest = self.current_dir();
        let password = password.to_string();
        let password = (!password.is_empty()).then_some(password);
        let format = match format.to_string().as_str() {
            "zip" => ops::ArchiveCreateFormat::Zip,
            "tar" => ops::ArchiveCreateFormat::Tar,
            "tar.gz" => ops::ArchiveCreateFormat::TarGz,
            "7z" => ops::ArchiveCreateFormat::SevenZip,
            _ => {
                self.set_error("Unsupported archive creation format.");
                return;
            }
        };
        if password.is_some()
            && !matches!(
                format,
                ops::ArchiveCreateFormat::Zip | ops::ArchiveCreateFormat::SevenZip
            )
        {
            self.set_error("Password encryption is available for ZIP and 7z archives.");
            return;
        }
        if sources.is_empty() {
            self.set_error("Select at least one item to compress.");
            return;
        }
        let title = format!("Compressing {} items", sources.len());
        self.run_archive_action(title, move |cancel, _progress| {
            let archive =
                ops::compress_archive(&sources, &dest, &cancel, format, password.as_deref())?;
            Ok(format!("Created {}", archive.display()))
        });
    }

    fn extract_path(&mut self, path: QString, destination: QString) {
        let archive = PathBuf::from(path.to_string());
        let Some(name) = archive
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            self.set_error("Invalid archive path.");
            return;
        };
        let destination = destination.to_string();
        let Some(dest) = paths::expand_path(&destination, &self.current_dir()) else {
            self.set_error("Choose a valid local extraction folder.");
            return;
        };
        if is_virtual_collection(&dest) {
            self.set_error("Choose a real folder as the extraction destination.");
            return;
        }
        if !dest.is_dir() {
            self.set_error(format!(
                "The extraction destination is not a folder: {}",
                dest.display()
            ));
            return;
        }
        self.run_archive_action(format!("Extracting {name}"), move |cancel, _progress| {
            let mut progress = |_: usize, _: usize, _: &str| {};
            let count = ops::extract_archive(&archive, &dest, &cancel, &mut progress)?;
            Ok(format!("Extracted {count} items to {}", dest.display()))
        });
    }

    fn start_usb_monitor(&mut self) {
        if USB_MONITOR_STARTED.swap(true, Ordering::AcqRel) {
            return;
        }
        let stop = self.device_monitor_stop.clone();
        let refresh_qptr = QPointer::from(&*self);
        let refresh = qmetaobject::queued_callback(move |()| {
            if let Some(backend) = refresh_qptr.as_pinned() {
                backend.borrow_mut().refresh_devices();
            }
        });
        std::thread::spawn(move || {
            let child = Command::new("udevadm")
                .args(["monitor", "--udev", "--subsystem-match=block"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();
            let Ok(mut child) = child else {
                USB_MONITOR_STARTED.store(false, Ordering::Release);
                return;
            };
            let Some(output) = child.stdout.take() else {
                let _ = child.kill();
                let _ = child.wait();
                USB_MONITOR_STARTED.store(false, Ordering::Release);
                return;
            };
            let fd = output.as_raw_fd();
            let mut buffer = [0_u8; 4096];
            loop {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let mut descriptor = libc::pollfd {
                    fd,
                    events: libc::POLLIN | libc::POLLHUP,
                    revents: 0,
                };
                let ready = unsafe { libc::poll(&mut descriptor, 1, 1000) };
                if ready < 0 {
                    break;
                }
                if ready == 0 {
                    continue;
                }
                let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
                if count <= 0 {
                    break;
                }
                refresh(());
            }
            let _ = output;
            let _ = child.kill();
            let _ = child.wait();
            USB_MONITOR_STARTED.store(false, Ordering::Release);
        });
    }

    fn start_device_monitor(&mut self) {
        if DEVICE_MONITOR_STARTED.swap(true, Ordering::AcqRel) {
            return;
        }
        let stop = self.device_monitor_stop.clone();
        let refresh_qptr = QPointer::from(&*self);
        let refresh = qmetaobject::queued_callback(move |()| {
            if let Some(backend) = refresh_qptr.as_pinned() {
                backend.borrow_mut().refresh_devices();
            }
        });
        std::thread::spawn(move || {
            let child = Command::new("gio")
                .args(["mount", "--monitor", "--detail"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();
            let Ok(mut child) = child else {
                DEVICE_MONITOR_STARTED.store(false, Ordering::Release);
                return;
            };
            let Some(output) = child.stdout.take() else {
                let _ = child.kill();
                let _ = child.wait();
                DEVICE_MONITOR_STARTED.store(false, Ordering::Release);
                return;
            };
            let fd = output.as_raw_fd();
            let mut buffer = [0_u8; 4096];
            loop {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let mut descriptor = libc::pollfd {
                    fd,
                    events: libc::POLLIN | libc::POLLHUP,
                    revents: 0,
                };
                let ready = unsafe { libc::poll(&mut descriptor, 1, 1000) };
                if ready < 0 {
                    break;
                }
                if ready == 0 {
                    continue;
                }
                let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
                if count <= 0 {
                    break;
                }
                refresh(());
            }
            let _ = output;
            let _ = child.kill();
            let _ = child.wait();
            DEVICE_MONITOR_STARTED.store(false, Ordering::Release);
        });
    }

    fn refresh_devices(&mut self) {
        self.start_device_monitor();
        self.start_usb_monitor();
        if self.devices_busy {
            self.device_refresh_pending = true;
            return;
        }
        self.devices_busy = true;
        self.devices_busy_changed();
        let qptr = QPointer::from(&*self);
        let callback =
            qmetaobject::queued_callback(move |result: Result<Vec<volumes::Volume>, String>| {
                if let Some(backend) = qptr.as_pinned() {
                    let mut backend = backend.borrow_mut();
                    backend.devices_busy = false;
                    backend.devices_busy_changed();
                    match result {
                        Ok(devices) => {
                            let next_paths: HashSet<PathBuf> = devices
                                .iter()
                                .filter_map(|device| device.path.clone())
                                .collect();
                            let removed_paths: Vec<PathBuf> = backend
                                .known_device_paths
                                .difference(&next_paths)
                                .filter(|path| path.as_path() != Path::new("/"))
                                .cloned()
                                .collect();
                            backend.known_device_paths = next_paths;
                            backend.devices = volume_maps(devices, &backend.icons);
                            backend.devices_changed();
                            if let Some(removed) = removed_paths
                                .iter()
                                .find(|path| backend.current_dir().starts_with(path))
                                .cloned()
                            {
                                let active = backend.active;
                                backend.load_tab(active, home_dir(), HistoryIntent::New);
                                backend.status = QString::from(format!(
                                    "{} was disconnected; returned to Home.",
                                    removed.display()
                                ));
                                backend.status_changed();
                            }
                        }
                        Err(error) => {
                            backend.status = QString::from(error);
                            backend.status_changed();
                        }
                    }
                    let refresh_again = backend.device_refresh_pending;
                    backend.device_refresh_pending = false;
                    if refresh_again {
                        backend.refresh_devices();
                    }
                }
            });
        std::thread::spawn(move || callback(volumes::discover()));
    }

    fn run_device_action(
        &mut self,
        action: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) {
        if self.devices_busy {
            return;
        }
        self.devices_busy = true;
        self.devices_busy_changed();
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: Result<String, String>| {
            if let Some(backend) = qptr.as_pinned() {
                let mut backend = backend.borrow_mut();
                backend.devices_busy = false;
                backend.devices_busy_changed();
                match result {
                    Ok(message) => {
                        backend.status = QString::from(message);
                        backend.status_changed();
                        backend.refresh_devices();
                    }
                    Err(error) => backend.set_error(error),
                }
            }
        });
        std::thread::spawn(move || callback(action()));
    }

    fn mount_device(&mut self, device: QString) {
        let device = device.to_string();
        if self.devices_busy {
            return;
        }
        self.devices_busy = true;
        self.device_action = QString::from(device.clone());
        self.devices_busy_changed();
        let qptr = QPointer::from(&*self);
        let callback =
            qmetaobject::queued_callback(move |result: Result<(PathBuf, String), String>| {
                if let Some(backend) = qptr.as_pinned() {
                    let mut backend = backend.borrow_mut();
                    backend.devices_busy = false;
                    backend.device_action = QString::default();
                    backend.devices_busy_changed();
                    match result {
                        Ok((path, message)) => {
                            backend.status = QString::from(message);
                            backend.status_changed();
                            let active = backend.active;
                            backend.load_tab(active, path, HistoryIntent::New);
                            backend.refresh_devices();
                        }
                        Err(error) => backend.set_error(error),
                    }
                }
            });
        std::thread::spawn(move || {
            let result = volumes::mount(&device).and_then(|message| {
                volumes::mounted_path(&device)
                    .map(|path| (path, message))
                    .ok_or_else(|| {
                        "The phone connected, but GVfs has not exposed its files yet. Wait a moment and click the device again.".to_owned()
                    })
            });
            callback(result);
        });
    }

    fn unlock_device(&mut self, device: QString, passphrase: QString) {
        let device = device.to_string();
        let passphrase = passphrase.to_string();
        self.run_device_action(move || volumes::unlock(&device, &passphrase));
    }

    fn unmount_device(&mut self, path: QString) {
        let path = PathBuf::from(path.to_string());
        self.run_device_action(move || volumes::unmount(&path));
    }

    fn eject_device(&mut self, path: QString, device: QString) {
        let path = PathBuf::from(path.to_string());
        let device = device.to_string();
        self.run_device_action(move || {
            if path.exists() {
                volumes::unmount(&path)?;
            }
            volumes::eject(&device)
        });
    }

    fn connect_network_location(&mut self, uri: QString) {
        let uri = uri.to_string();
        self.run_device_action(move || volumes::mount_network(&uri));
    }
    fn active_entries(&self) -> &[Entry] {
        let Some(tab) = self.tabs_data.get(self.active) else {
            return &[];
        };
        if self.view_mode.to_string() == "list"
            && tab.search.is_none()
            && !is_virtual_collection(&tab.path)
        {
            &tab.tree_rows
        } else {
            &tab.entries
        }
    }

    fn replace_tree_rows(&mut self, index: usize, rows: Vec<Entry>) {
        let Some(tab) = self.tabs_data.get(index) else {
            return;
        };
        let old_rows = tab.tree_rows.clone();
        let visible = self.active == index && self.view_mode.to_string() == "list";
        if !visible {
            self.tabs_data[index].tree_rows = rows;
            return;
        }

        let same_identity = |left: &Entry, right: &Entry| {
            left.path == right.path && left.tree_depth == right.tree_depth
        };
        let mut prefix = 0;
        while prefix < old_rows.len()
            && prefix < rows.len()
            && same_identity(&old_rows[prefix], &rows[prefix])
        {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix < old_rows.len().saturating_sub(prefix)
            && suffix < rows.len().saturating_sub(prefix)
            && same_identity(
                &old_rows[old_rows.len() - 1 - suffix],
                &rows[rows.len() - 1 - suffix],
            )
        {
            suffix += 1;
        }

        let old_middle = old_rows.len() - prefix - suffix;
        let new_middle = rows.len() - prefix - suffix;
        if old_middle > 0 {
            (self as &mut dyn QAbstractListModel)
                .begin_remove_rows(prefix as i32, (prefix + old_middle - 1) as i32);
            self.tabs_data[index]
                .tree_rows
                .drain(prefix..prefix + old_middle);
            (self as &mut dyn QAbstractListModel).end_remove_rows();
        }
        if new_middle > 0 {
            (self as &mut dyn QAbstractListModel)
                .begin_insert_rows(prefix as i32, (prefix + new_middle - 1) as i32);
            self.tabs_data[index].tree_rows.splice(
                prefix..prefix,
                rows[prefix..prefix + new_middle].iter().cloned(),
            );
            (self as &mut dyn QAbstractListModel).end_insert_rows();
        }
        self.tabs_data[index].tree_rows = rows;

        let old_by_path = old_rows
            .iter()
            .map(|entry| (entry.path.as_path(), entry))
            .collect::<HashMap<_, _>>();
        let changed_rows = self.tabs_data[index]
            .tree_rows
            .iter()
            .enumerate()
            .filter_map(|(row, entry)| {
                old_by_path
                    .get(entry.path.as_path())
                    .filter(|old| {
                        old.tree_depth == entry.tree_depth
                            && !same_entries(
                                std::slice::from_ref(*old),
                                std::slice::from_ref(entry),
                            )
                    })
                    .map(|_| row)
            })
            .collect::<Vec<_>>();
        let mut range_start = None;
        let mut previous = 0;
        for row in changed_rows.into_iter().chain(std::iter::once(usize::MAX)) {
            if let Some(start) = range_start {
                if row != previous + 1 {
                    let first = self.row_index(start as i32);
                    let last = self.row_index(previous as i32);
                    (self as &mut dyn QAbstractListModel).data_changed(first, last);
                    range_start = None;
                }
            }
            if row != usize::MAX {
                range_start.get_or_insert(row);
                previous = row;
            }
        }

        self.item_count = self.tabs_data[index].tree_rows.len().min(i32::MAX as usize) as i32;
        self.item_count_changed();
    }

    fn notify_tree_row_changed(&mut self, path: &Path) {
        if let Some(row) = self.tabs_data[self.active]
            .tree_rows
            .iter()
            .position(|entry| entry.path == path)
        {
            let index = self.row_index(row as i32);
            (self as &mut dyn QAbstractListModel).data_changed(index, index);
        }
    }

    fn replace_tree_descendants(
        &mut self,
        index: usize,
        parent_path: &Path,
        descendants: Vec<Entry>,
    ) {
        let Some(tab) = self.tabs_data.get(index) else {
            return;
        };
        let Some(parent_row) = tab
            .tree_rows
            .iter()
            .position(|entry| entry.path == parent_path)
        else {
            return;
        };
        let parent_depth = tab.tree_rows[parent_row].tree_depth;
        let first = parent_row + 1;
        let mut end = first;
        while end < tab.tree_rows.len() && tab.tree_rows[end].tree_depth > parent_depth {
            end += 1;
        }
        let remove_count = end - first;
        let insert_count = descendants.len();
        let visible = self.active == index && self.view_mode.to_string() == "list";

        if visible && remove_count > 0 {
            (self as &mut dyn QAbstractListModel).begin_remove_rows(first as i32, (end - 1) as i32);
        }
        self.tabs_data[index].tree_rows.drain(first..end);
        if visible && remove_count > 0 {
            (self as &mut dyn QAbstractListModel).end_remove_rows();
        }
        if visible && insert_count > 0 {
            (self as &mut dyn QAbstractListModel)
                .begin_insert_rows(first as i32, (first + insert_count - 1) as i32);
            self.tabs_data[index]
                .tree_rows
                .splice(first..first, descendants);
            (self as &mut dyn QAbstractListModel).end_insert_rows();
        } else if insert_count > 0 {
            self.tabs_data[index]
                .tree_rows
                .splice(first..first, descendants);
        }
        if visible {
            self.item_count = self.tabs_data[index].tree_rows.len().min(i32::MAX as usize) as i32;
            self.item_count_changed();
        }
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs_data.iter().position(|tab| tab.id == id)
    }

    fn apply_startup_state(
        &mut self,
        bookmark_store: BookmarkStore,
        recent_store: RecentStore,
        tag_store: TagStore,
        warning: Option<String>,
    ) {
        self.bookmark_store = bookmark_store;
        self.recent_store = recent_store;
        self.tag_store = tag_store;
        if let Some(warning) = warning {
            self.set_error(warning);
        }
        self.push_bookmarks_property();
        self.push_tags_property();
    }

    fn schedule_startup_state_load(&mut self) {
        // Keep the first render moving: the directory listing starts on its own
        // worker thread, and the bookmark/tag metadata is filled in afterward.
        let (bookmark_store, bookmark_warning) = BookmarkStore::load();
        let (recent_store, recent_warning) = RecentStore::load();
        let (tag_store, tag_warning) = TagStore::load();
        self.apply_startup_state(
            bookmark_store,
            recent_store,
            tag_store,
            bookmark_warning.or(recent_warning).or(tag_warning),
        );
    }

    fn start(&mut self, initial: QString) {
        self.load_places();

        if self.started {
            self.refresh();
            return;
        }

        self.started = true;

        let initial = initial.trimmed().to_string();
        if !initial.is_empty() {
            if let Some(target) = paths::expand_path(&initial, &home_dir()) {
                self.load_tab(0, target, HistoryIntent::New);
            } else {
                self.set_error(format!("Not a valid path: {initial}"));
            }
            self.schedule_startup_state_load();
            return;
        }

        if self.config.ui.restore_session {
            if let Some(restored) = self.restore_session() {
                self.rebuild_tabs_property();
                self.sync_active(true);
                self.load_tab(self.active, restored, HistoryIntent::Current);
                self.schedule_startup_state_load();
                return;
            }
        }

        let target = home_dir();
        self.load_tab(0, target, HistoryIntent::New);
        self.schedule_startup_state_load();
    }

    fn pause_for_background(&mut self) {
        self.stop_watch();
        if crate::lifecycle::performance_config().startup_behavior == "fast_reopen" {
            return;
        }
        for tab in &mut self.tabs_data {
            tab.generation = tab.generation.wrapping_add(1);
            tab.tree_generation = tab.tree_generation.wrapping_add(1);
            tab.search_generation = tab.search_generation.wrapping_add(1);
            tab.cancel_search();
            tab.loading = false;
            tab.search_running = false;
            tab.entries.clear();
            tab.tree_rows.clear();
            tab.tree_children.clear();
            tab.tree_loading.clear();
            tab.folder_entries.clear();
        }
        self.sync_active(true);
    }

    fn resume_from_background(&mut self) {
        if self.tabs_data.is_empty() {
            self.start(QString::default());
            return;
        }
        let path = self.tabs_data[self.active].path.clone();
        self.load_tab(self.active, path, HistoryIntent::Current);
    }

    fn restore_session(&mut self) -> Option<PathBuf> {
        let session = Session::load();
        if session.is_empty() {
            return None;
        }
        let sort = self.config.sort_spec();
        let mut tabs = Vec::new();
        for entry in &session.tabs {
            let path = PathBuf::from(&entry.path);
            if path.is_dir() || is_virtual_collection(&path) {
                let mut sort_spec = sort;
                if let Ok(key) = entry.sort_key.parse::<SortKey>() {
                    sort_spec.key = key;
                    sort_spec.ascending = entry.sort_ascending;
                    sort_spec.directories_first = entry.directories_first;
                }
                let id = self.next_tab_id;
                self.next_tab_id += 1;
                tabs.push(Tab::new(id, paths::clean_lexically(&path), sort_spec));
            }
        }
        if tabs.is_empty() {
            return None;
        }
        self.tabs_data = tabs;
        self.active = session.active.min(self.tabs_data.len() - 1);
        Some(self.tabs_data[self.active].path.clone())
    }

    fn load_places(&mut self) {
        let mut list = QVariantList::default();
        let mut places = user_places();
        places.insert(
            0,
            Place {
                id: "recent".to_owned(),
                label: "Recent".to_owned(),
                path: PathBuf::from(RECENT_VIRTUAL_PATH),
            },
        );
        if !places.iter().any(|place| place.id == "trash") {
            places.push(Place {
                id: "trash".to_owned(),
                label: "Trash".to_owned(),
                path: trash_dir(),
            });
        }
        for place in places {
            let mut map = QVariantMap::default();
            map.insert(
                QString::from("id"),
                QVariant::from(QString::from(place.id.as_str())),
            );
            map.insert(
                QString::from("label"),
                QVariant::from(QString::from(place.label)),
            );
            map.insert(
                QString::from("path"),
                QVariant::from(QString::from(place.path.to_string_lossy().into_owned())),
            );
            let icon_names = match place.id.as_str() {
                "recent" => vec!["document-open-recent", "view-history", "appointment-soon"],
                "home" => vec!["user-home", "go-home", "folder-home"],
                "trash" => vec!["user-trash", "user-trash-full", "folder-trash"],
                _ => icon_names_for(
                    place
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(""),
                    true,
                    "",
                ),
            };
            let icon = self.icons.resolve_first(&icon_names).unwrap_or_default();
            map.insert(
                QString::from("iconUrl"),
                QVariant::from(QString::from(icon)),
            );
            list.push(QVariant::from(map));
        }
        self.places = list;
        self.places_changed();
    }

    fn navigate(&mut self, path: QString) {
        self.navigate_input(&path.to_string());
    }

    fn navigate_input(&mut self, input: &str) {
        if input.trim().eq_ignore_ascii_case("recent") {
            self.load_tab(
                self.active,
                PathBuf::from(RECENT_VIRTUAL_PATH),
                HistoryIntent::New,
            );
            return;
        }
        let current = self.current_dir();
        let cwd = if is_virtual_collection(&current) {
            home_dir()
        } else {
            current
        };
        let Some(target) = paths::expand_path(input, &cwd) else {
            self.set_error(format!("Not a valid path: {input}"));
            return;
        };
        self.load_tab(self.active, target, HistoryIntent::New);
    }

    fn current_dir(&self) -> PathBuf {
        self.tabs_data
            .get(self.active)
            .map(|tab| tab.path.clone())
            .unwrap_or_else(home_dir)
    }

    fn load_tab(&mut self, index: usize, target: PathBuf, intent: HistoryIntent) {
        if index >= self.tabs_data.len() {
            return;
        }
        let target = paths::clean_lexically(&target);
        let tag_collection = is_tag_collection_path(&target);
        if !is_recent_path(&target) && !tag_collection {
            match fs::metadata(&target) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    self.fail_tab(index, format!("{} is not a folder.", target.display()));
                    return;
                }
                Err(error) => {
                    self.fail_tab(index, friendly_io_error(&target, &error));
                    return;
                }
            }
        }

        let show_hidden = self.show_hidden;
        let icons = self.icons.clone();
        let tags = self.tag_store.clone();
        let recent_items = self.recent_store.items.clone();
        let recent_view = is_recent_path(&target);
        let tag_collection = parse_tag_collection_path(&target);
        if is_tag_collection_path(&target) && tag_collection.is_none() {
            self.fail_tab(index, "The tag collection is invalid.".to_owned());
            return;
        }
        let (tab_id, generation, spec) = {
            let tab = &mut self.tabs_data[index];
            let preserve_tree_loads =
                matches!(intent, HistoryIntent::Current) && tab.path == target;
            if recent_view && tab.path != target {
                tab.sort = SortSpec {
                    key: SortKey::Modified,
                    ascending: false,
                    directories_first: false,
                };
            }
            // Reflect the destination immediately so breadcrumbs and the window
            // title never lag behind a navigation while the listing loads.
            tab.path = target.clone();
            tab.generation += 1;
            tab.loading = true;
            tab.error.clear();
            tab.pending_intent = Some(intent);
            if !preserve_tree_loads {
                tab.tree_generation += 1;
                tab.tree_loading.clear();
                tab.tree_children
                    .retain(|folder, _| !folder.starts_with(&target));
            }
            if !matches!(intent, HistoryIntent::Current) {
                // A new location ends any search in this tab.
                tab.cancel_search();
                tab.folder_entries.clear();
            }
            (tab.id, tab.generation, tab.sort)
        };

        // Keep the existing delegates alive while the next listing loads. The
        // completed result below performs the single necessary model reset.
        self.sync_active(false);
        self.sync_nav();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: LoadResult| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_load_result(result);
            }
        });

        std::thread::spawn(move || {
            let (mut entries, stale_tag_paths) = if recent_view {
                (
                    Ok(read_recent_files(&recent_items, spec, &icons, &tags)),
                    Vec::new(),
                )
            } else if let Some((selected, untagged)) = tag_collection {
                let (entries, missing) = read_tag_collection(
                    &tags.collection_paths(&selected, untagged),
                    &selected,
                    untagged,
                    spec,
                    &icons,
                    &tags,
                );
                (Ok(entries), missing)
            } else {
                (
                        read_directory(&target, spec, show_hidden, &icons),
                        Vec::new(),
                )
            };
            if target == trash_dir() {
                if let Ok(entries) = &mut entries {
                    let trash_root = ops::home_trash();
                    for entry in entries {
                        entry.display_path = ops::trash_original_for(&trash_root, &entry.name)
                            .map(|original| ops::display_path(&original))
                            .unwrap_or_else(|| "/unknown".to_owned());
                    }
                }
            }
            callback(LoadResult {
                tab_id,
                generation,
                path: target,
                entries,
                stale_tag_paths,
            });
        });
    }

    fn load_expanded_tree_folders(&mut self, index: usize) {
        if self.view_mode.to_string() != "list" {
            return;
        }
        let paths = self
            .tabs_data
            .get(index)
            .map(|tab| {
                tab.entries
                    .iter()
                    .filter(|entry| {
                        entry.is_dir && !entry.is_symlink && tab.tree_expanded.contains(&entry.path)
                    })
                    .map(|entry| entry.path.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for path in paths {
            self.start_tree_load(index, path);
        }
    }

    fn start_tree_load(&mut self, index: usize, path: PathBuf) {
        if self.view_mode.to_string() != "list" {
            return;
        }
        let show_hidden = self.show_hidden;
        let icons = self.icons.clone();
        let spec;
        let (tab_id, generation) = {
            let tab = &mut self.tabs_data[index];
            if tab.search.is_some()
                || !tab.tree_expanded.contains(&path)
                || tab.tree_children.contains_key(&path)
                || !tab.tree_loading.insert(path.clone())
            {
                return;
            }
            spec = tab.sort;
            (tab.id, tab.tree_generation)
        };
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: TreeLoadResult| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_tree_load_result(result);
            }
        });
        std::thread::spawn(move || {
            let entries = match fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => Err(format!(
                    "Symbolic-link folders cannot be expanded: {}",
                    path.display()
                )),
                Ok(meta) if !meta.is_dir() => {
                    Err(format!("{} is no longer a folder.", path.display()))
                }
                Ok(_) => read_directory(&path, spec, show_hidden, &icons),
                Err(error) => Err(friendly_io_error(&path, &error)),
            };
            callback(TreeLoadResult {
                tab_id,
                generation,
                path,
                entries,
            });
        });
    }

    fn apply_tree_load_result(&mut self, result: TreeLoadResult) {
        let Some(index) = self.tab_index(result.tab_id) else {
            return;
        };
        let tags = self.tag_store.clone();
        let visible = self.active == index
            && self.view_mode.to_string() == "list"
            && self.tabs_data[index].search.is_none();
        if self.tabs_data[index].tree_generation != result.generation {
            return;
        }
        let mut seen_entries = Vec::new();
        let mut loaded_directory = false;
        let load_error = {
            let tab = &mut self.tabs_data[index];
            tab.tree_loading.remove(&result.path);
            match result.entries {
                Ok(mut entries) => {
                    loaded_directory = true;
                    for entry in &mut entries {
                        entry.tag = tags.tag_for(&entry.path, entry.dev, entry.ino);
                    }
                    seen_entries = entries
                        .iter()
                        .map(|entry| (entry.path.clone(), entry.dev, entry.ino))
                        .collect();
                    tab.tree_children.insert(result.path.clone(), entries);
                    None
                }
                Err(message) => {
                    let vanished = matches!(
                        fs::symlink_metadata(&result.path),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound
                    );
                    if vanished || message.starts_with("Symbolic-link folders cannot be expanded:")
                    {
                        tab.tree_expanded.remove(&result.path);
                        tab.tree_children.remove(&result.path);
                        None
                    } else {
                        Some(message)
                    }
                }
            }
        };
        for (path, dev, ino) in seen_entries {
            self.tag_store.record_seen(&path, dev, ino);
        }
        if loaded_directory {
            self.tag_store.record_directory(&result.path);
        }
        if !self.tag_store.known_paths.is_empty() {
            self.push_tags_property();
        }
        if visible {
            let should_be_expanded = self.tabs_data[index].tree_expanded.contains(&result.path);
            let row_changed = self.tabs_data[index]
                .tree_rows
                .iter_mut()
                .find(|entry| entry.path == result.path)
                .is_some_and(|entry| {
                    if entry.tree_expanded == should_be_expanded {
                        false
                    } else {
                        entry.tree_expanded = should_be_expanded;
                        true
                    }
                });
            if row_changed {
                self.notify_tree_row_changed(&result.path);
            }
            let parent = self.tabs_data[index]
                .tree_rows
                .iter()
                .find(|entry| entry.path == result.path)
                .filter(|entry| entry.tree_expanded)
                .map(|entry| (entry.tree_depth, entry.path.clone()));
            if let Some((depth, path)) = parent {
                let mut descendants = Vec::new();
                if let Some(children) = self.tabs_data[index].tree_children.get(&path) {
                    flatten_tree_children(
                        children,
                        &self.tabs_data[index].tree_children,
                        &self.tabs_data[index].tree_expanded,
                        depth + 1,
                        &mut HashSet::new(),
                        &mut descendants,
                    );
                }
                let load_paths = descendants
                    .iter()
                    .filter(|entry| {
                        entry.tree_expanded
                            && !self.tabs_data[index]
                                .tree_children
                                .contains_key(&entry.path)
                    })
                    .map(|entry| entry.path.clone())
                    .collect::<Vec<_>>();
                let unchanged = self.tabs_data[index]
                    .tree_rows
                    .iter()
                    .position(|entry| entry.path == path)
                    .is_some_and(|parent_row| {
                        let depth = self.tabs_data[index].tree_rows[parent_row].tree_depth;
                        let first = parent_row + 1;
                        let mut end = first;
                        while end < self.tabs_data[index].tree_rows.len()
                            && self.tabs_data[index].tree_rows[end].tree_depth > depth
                        {
                            end += 1;
                        }
                        same_entries(&self.tabs_data[index].tree_rows[first..end], &descendants)
                    });
                if !unchanged {
                    self.replace_tree_descendants(index, &path, descendants);
                }
                for path in load_paths {
                    self.start_tree_load(index, path);
                }
            } else if self.tabs_data[index]
                .tree_rows
                .iter()
                .any(|entry| entry.path == result.path)
            {
                self.replace_tree_descendants(index, &result.path, Vec::new());
            }
        }
        if index == self.active {
            self.ensure_watch();
        }
        if let Some(message) = load_error {
            self.set_error(message);
        }
    }

    fn fail_tab(&mut self, index: usize, message: String) {
        if let Some(tab) = self.tabs_data.get_mut(index) {
            tab.loading = false;
            tab.pending_intent = None;
            tab.error = message.clone();
        }
        if index == self.active {
            self.error = QString::from(message);
            self.error_changed();
            self.loading = false;
            self.loading_changed();
        }
    }

    fn apply_load_result(&mut self, result: LoadResult) {
        let Some(index) = self.tab_index(result.tab_id) else {
            return;
        };
        if self.tabs_data[index].generation != result.generation {
            return;
        }
        self.tabs_data[index].loading = false;
        if self.prune_missing_tag_paths(&result.stale_tag_paths) {
            self.push_tags_property();
        }

        match result.entries {
            Ok(mut entries) => {
                let in_trash = result.path == trash_dir();
                if !is_virtual_collection(&result.path) && !in_trash {
                    let listed = entries
                        .iter()
                        .map(|entry| entry.path.clone())
                        .collect::<HashSet<_>>();
                    let mut known_prefix = result.path.to_string_lossy().into_owned();
                    if !known_prefix.ends_with(std::path::MAIN_SEPARATOR) {
                        known_prefix.push(std::path::MAIN_SEPARATOR);
                    }
                    let missing = self
                        .tag_store
                        .known_paths
                        .range(known_prefix.clone()..)
                        .take_while(|(path, _)| path.starts_with(&known_prefix))
                        .map(|(path, _)| path)
                        .map(PathBuf::from)
                        .filter(|path| {
                            path.parent() == Some(result.path.as_path())
                                && !listed.contains(path)
                                && !path
                                    .file_name()
                                    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
                                && matches!(
                                    fs::symlink_metadata(path),
                                    Err(error) if error.kind() == std::io::ErrorKind::NotFound
                                )
                        })
                        .collect::<Vec<_>>();
                    if self.prune_missing_tag_paths(&missing) {
                        self.push_tags_property();
                    }
                }
                for entry in &mut entries {
                    entry.tag = self.tag_store.tag_for(&entry.path, entry.dev, entry.ino);
                    if !is_tag_collection_path(&result.path) && !in_trash {
                        self.tag_store
                            .record_seen(&entry.path, entry.dev, entry.ino);
                    }
                }
                let mut recent_store_changed = false;
                if is_recent_path(&result.path) {
                    let previous_len = self.recent_store.items.len();
                    let retained = entries
                        .iter()
                        .map(|entry| entry.path.to_string_lossy().into_owned())
                        .collect();
                    self.recent_store.retain_paths(&retained);
                    recent_store_changed = self.recent_store.items.len() != previous_len;
                }
                let entries_changed = !same_entries(&self.tabs_data[index].entries, &entries);
                let tree_rows = {
                    let tab = &self.tabs_data[index];
                    flatten_tree(&entries, &tab.tree_children, &tab.tree_expanded)
                };
                let tree_changed = !same_entries(&self.tabs_data[index].tree_rows, &tree_rows);
                let tree_model_active = self.active == index
                    && self.view_mode.to_string() == "list"
                    && self.tabs_data[index].search.is_none()
                    && !is_virtual_collection(&result.path);
                let reset_model =
                    self.active == index && (entries_changed || tree_changed) && !tree_model_active;
                if reset_model {
                    (self as &mut dyn QAbstractListModel).begin_reset_model();
                }
                {
                    let tab = &mut self.tabs_data[index];
                    tab.path = result.path.clone();
                    tab.loaded_path = Some(result.path.clone());
                    tab.install_folder_entries(entries);
                    tab.error.clear();
                    if !tree_model_active {
                        tab.tree_rows = tree_rows.clone();
                    }
                }
                if tree_model_active && tree_changed {
                    self.replace_tree_rows(index, tree_rows);
                }
                if reset_model {
                    (self as &mut dyn QAbstractListModel).end_reset_model();
                }
                self.apply_history(index, &result.path);
                if self.active == index {
                    self.error = QString::from("");
                    self.error_changed();
                    self.sync_active(false);
                    self.ensure_watch();
                }
                self.load_expanded_tree_folders(index);
                // A refresh while searching repeats the same query against the
                // reloaded folder, so results never go stale silently.
                if self.tabs_data[index].search.is_some() {
                    self.run_search(index);
                }
                if recent_store_changed {
                    self.persist_recent_store();
                }
            }
            Err(message) => {
                self.fail_tab(index, message);
            }
        }
        self.rebuild_tabs_property();
    }

    fn apply_history(&mut self, index: usize, path: &Path) {
        let tab = &mut self.tabs_data[index];
        match tab.pending_intent.take() {
            Some(HistoryIntent::New) => {
                tab.history.truncate(tab.history_index + 1);
                if tab.history.last().map(|entry| entry.as_path()) != Some(path) {
                    tab.history.push(path.to_path_buf());
                }
                tab.history_index = tab.history.len().saturating_sub(1);
            }
            Some(HistoryIntent::Back(step)) | Some(HistoryIntent::Forward(step)) => {
                if tab.history.get(step).map(|entry| entry.as_path()) == Some(path) {
                    tab.history_index = step;
                }
            }
            Some(HistoryIntent::Current) | None => {}
        }
        if index == self.active {
            self.sync_nav();
        }
    }

    fn sync_nav(&mut self) {
        let Some(tab) = self.tabs_data.get(self.active) else {
            return;
        };
        let back = tab.history_index > 0 && !tab.history.is_empty();
        let forward = tab.history_index + 1 < tab.history.len();
        if self.can_go_back != back {
            self.can_go_back = back;
            self.nav_changed();
        }
        if self.can_go_forward != forward {
            self.can_go_forward = forward;
            self.nav_changed();
        }
    }

    /// Refresh every property that mirrors the active tab.
    fn sync_active(&mut self, reset_model: bool) {
        let count = self.active_entries().len().min(i32::MAX as usize) as i32;
        let Some((
            tab_path,
            crumbs,
            loading,
            error,
            sort,
            search_query,
            search_running,
            summary,
            in_trash,
        )) = self.tabs_data.get(self.active).map(|tab| {
            (
                tab.path.clone(),
                self.build_breadcrumbs(&tab.path),
                tab.loading,
                tab.error.clone(),
                tab.sort,
                tab.search.as_ref().map(|state| state.query.clone()),
                tab.search_running,
                tab.search
                    .as_ref()
                    .map(|state| search_summary(state, &tab.entries))
                    .unwrap_or_default(),
                tab.path == trash_dir(),
            )
        })
        else {
            return;
        };
        let path = tab_path.to_string_lossy().into_owned();
        let recent_view = is_recent_path(&tab_path);
        let tag_collection_view = is_tag_collection_path(&tab_path);
        let tag_collection_label = parse_tag_collection_path(&tab_path)
            .map(|(selected, untagged)| {
                if untagged {
                    "Untagged".to_owned()
                } else if selected.is_empty() {
                    "All Tagged".to_owned()
                } else {
                    selected
                        .iter()
                        .map(|index| self.tag_store.name(*index))
                        .collect::<Vec<_>>()
                        .join(" + ")
                }
            })
            .unwrap_or_default();

        if reset_model {
            (self as &mut dyn QAbstractListModel).begin_reset_model();
            (self as &mut dyn QAbstractListModel).end_reset_model();
        }

        if self.current_path.to_string() != path {
            self.current_path = QString::from(path);
            self.current_path_changed();
            self.breadcrumbs = crumbs;
            self.breadcrumbs_changed();
        }
        self.item_count = count;
        self.item_count_changed();
        if self.loading != loading {
            self.loading = loading;
            self.loading_changed();
        }
        if self.error.to_string() != error {
            self.error = QString::from(error);
            self.error_changed();
        }
        let key = sort.key.as_str();
        if self.sort_key.to_string() != key {
            self.sort_key = QString::from(key);
            self.view_changed();
        }
        if self.sort_ascending != sort.ascending {
            self.sort_ascending = sort.ascending;
            self.view_changed();
        }
        if self.directories_first != sort.directories_first {
            self.directories_first = sort.directories_first;
            self.view_changed();
        }
        let mode = search_query.is_some();
        if self.search_mode != mode {
            self.search_mode = mode;
            self.search_changed();
        }
        if self.searching != search_running {
            self.searching = search_running;
            self.search_changed();
        }
        self.search_query = QString::from(search_query.unwrap_or_default());
        self.search_changed();
        self.search_summary = QString::from(summary);
        self.in_trash = in_trash;
        if self.recent_view != recent_view {
            self.recent_view = recent_view;
            self.view_changed();
        }
        if self.tag_collection_view != tag_collection_view {
            self.tag_collection_view = tag_collection_view;
            self.view_changed();
        }
        let virtual_collection = is_virtual_collection(&tab_path);
        if self.virtual_collection != virtual_collection {
            self.virtual_collection = virtual_collection;
            self.view_changed();
        }
        if self.tag_collection_label.to_string() != tag_collection_label {
            self.tag_collection_label = QString::from(tag_collection_label);
            self.view_changed();
        }
        self.view_changed();
        self.update_status();
        self.sync_nav();
        self.rebuild_tabs_property();
    }

    fn build_breadcrumbs(&self, path: &Path) -> QVariantList {
        let mut list = QVariantList::default();
        if is_recent_path(path) {
            let mut map = QVariantMap::default();
            map.insert(
                QString::from("label"),
                QVariant::from(QString::from("Recent")),
            );
            map.insert(
                QString::from("path"),
                QVariant::from(QString::from(RECENT_VIRTUAL_PATH)),
            );
            list.push(QVariant::from(map));
            return list;
        }
        if is_tag_collection_path(path) {
            let mut map = QVariantMap::default();
            map.insert(
                QString::from("label"),
                QVariant::from(QString::from("Tags")),
            );
            map.insert(
                QString::from("path"),
                QVariant::from(QString::from(
                    tag_collection_path(&[], false)
                        .to_string_lossy()
                        .into_owned(),
                )),
            );
            list.push(QVariant::from(map));
            let label = parse_tag_collection_path(path)
                .map(|(selected, untagged)| {
                    if untagged {
                        "Untagged".to_owned()
                    } else if selected.is_empty() {
                        "All Tagged".to_owned()
                    } else {
                        selected
                            .iter()
                            .map(|index| self.tag_store.name(*index))
                            .collect::<Vec<_>>()
                            .join(" + ")
                    }
                })
                .unwrap_or_else(|| "Tags".to_owned());
            map = QVariantMap::default();
            map.insert(QString::from("label"), QVariant::from(QString::from(label)));
            map.insert(
                QString::from("path"),
                QVariant::from(QString::from(path.to_string_lossy().into_owned())),
            );
            list.push(QVariant::from(map));
            return list;
        }
        for crumb in breadcrumbs(path) {
            let mut map = QVariantMap::default();
            map.insert(
                QString::from("label"),
                QVariant::from(QString::from(crumb.label)),
            );
            map.insert(
                QString::from("path"),
                QVariant::from(QString::from(crumb.path.to_string_lossy().into_owned())),
            );
            list.push(QVariant::from(map));
        }
        list
    }

    fn update_status(&mut self) {
        let visible_count = self.active_entries().len();
        let Some(tab) = self.tabs_data.get(self.active) else {
            return;
        };
        let text = if let Some(state) = &tab.search {
            let count = tab.entries.len();
            let mut text = format!("{count} {} for “{}”", item_word(count), state.query);
            if state.truncated {
                text.push_str(" · refine to narrow");
            }
            text
        } else {
            let count = visible_count;
            format!("{count} {}", item_word(count))
        };
        self.status = QString::from(text);
        self.status_changed();
    }

    fn refresh(&mut self) {
        if is_virtual_collection(&self.current_dir()) {
            return;
        }
        let path = self.current_dir();
        if let Some(tab) = self.tabs_data.get_mut(self.active) {
            tab.tree_generation += 1;
            tab.tree_loading.clear();
            tab.tree_children.clear();
        }
        self.load_tab(self.active, path, HistoryIntent::Current);
    }

    fn request_preview(&mut self, path: QString) {
        self.request_preview_internal(path, 1, 1200, true);
    }

    fn request_preview_page(&mut self, path: QString, page: i32, max_dimension: i32) {
        self.request_preview_internal(
            path,
            page.max(1) as usize,
            max_dimension.max(512) as u32,
            false,
        );
    }

    fn request_preview_internal(
        &mut self,
        path: QString,
        page: usize,
        max_dimension: u32,
        clear: bool,
    ) {
        let path = PathBuf::from(path.to_string());
        self.preview_generation = self.preview_generation.wrapping_add(1);
        let generation = self.preview_generation;
        self.preview_path = path.to_string_lossy().into_owned();
        if clear {
            self.preview_data = QVariantMap::default();
            self.preview_changed();
        }
        if path.as_os_str().is_empty() {
            return;
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if clear
            && (preview::is_audio_extension(&extension) || preview::is_video_extension(&extension))
        {
            self.request_thumbnail(QString::from(path.to_string_lossy().into_owned()), true);
        }

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: PreviewResult| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_preview_result(result);
            }
        });
        std::thread::spawn(move || {
            let is_pdf = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
            let data = preview::build_pdf_page(&path, page, max_dimension);
            if is_pdf && data.page_count > 1 {
                let mut adjacent = Vec::with_capacity(2);
                if page > 1 {
                    adjacent.push(page - 1);
                }
                if page < data.page_count {
                    adjacent.push(page + 1);
                }
                preview::prefetch_pdf_pages(&path, &adjacent, max_dimension);
            }
            callback(PreviewResult { generation, data });
        });
    }

    fn apply_preview_result(&mut self, result: PreviewResult) {
        if result.generation != self.preview_generation {
            return;
        }
        self.preview_data = Self::preview_to_map(result.data);
        self.preview_changed();
    }

    fn file_url(&self, path: QString) -> QString {
        QString::from(paths::path_to_file_url(Path::new(&path.to_string())))
    }

    fn thumbnail_url(&self, path: QString) -> QString {
        let path = PathBuf::from(path.to_string());
        preview::cached_thumbnail_path(&path)
            .map(|thumbnail| QString::from(paths::path_to_file_url(&thumbnail)))
            .unwrap_or_default()
    }

    fn notify_thumbnail_ready(&mut self, path: String, cache_url: String) -> bool {
        if self.thumbnail_ready_path.to_string() == path
            && self.thumbnail_ready_cache_url == cache_url
        {
            return false;
        }
        self.thumbnail_ready_path = QString::from(path);
        self.thumbnail_ready_cache_url = cache_url;
        self.thumbnail_ready_changed();
        true
    }

    fn prepare_thumbnail_request(&mut self, path: &Path) -> bool {
        let key = path.to_string_lossy().into_owned();
        if let Some(thumbnail) = preview::cached_thumbnail_path(path) {
            if !self.thumbnail_pending.contains(&key) {
                self.notify_thumbnail_ready(key, paths::path_to_file_url(&thumbnail));
            }
            return false;
        }
        self.thumbnail_pending.insert(key)
    }

    fn request_thumbnail(&mut self, path: QString, visible: bool) {
        let path = PathBuf::from(path.to_string());
        let key = path.to_string_lossy().into_owned();
        if !self.prepare_thumbnail_request(&path) {
            if self.thumbnail_pending.contains(&key) {
                promote_thumbnail_job(&key, if visible { 2 } else { 1 });
            }
            return;
        }

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: ThumbnailResult| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_thumbnail_result(result);
            }
        });
        let callback = Box::new(callback) as ThumbnailCallback;
        match submit_thumbnail_job(
            path.clone(),
            key.clone(),
            if visible { 2 } else { 1 },
            callback,
        ) {
            ThumbnailSubmit::Queued | ThumbnailSubmit::Deduplicated => {}
            ThumbnailSubmit::Full => {
                self.thumbnail_pending.remove(&key);
                eprintln!(
                    "Filem: thumbnail queue is full; deferred request for {}.",
                    path.display()
                );
            }
        }
    }

    fn apply_thumbnail_result(&mut self, result: ThumbnailResult) {
        self.thumbnail_pending.remove(&result.path);
        let refresh_preview = result.result.is_ok() && self.preview_path == result.path;
        let preview_path = self.preview_path.clone();
        match result.result {
            Ok(url) => {
                self.notify_thumbnail_ready(result.path, url);
            }
            Err(error) => {
                eprintln!("Filem: {error}");
            }
        }
        if refresh_preview {
            self.request_preview_internal(QString::from(preview_path), 1, 1200, false);
        }
    }

    fn go_up(&mut self) {
        let current = self.current_dir();
        if is_virtual_collection(&current) {
            return;
        }
        let Some(parent) = current.parent() else {
            return;
        };
        if parent == current {
            return;
        }
        self.load_tab(self.active, parent.to_path_buf(), HistoryIntent::New);
    }

    fn go_back(&mut self) {
        let Some(tab) = self.tabs_data.get(self.active) else {
            return;
        };
        if tab.history_index == 0 || tab.history.is_empty() {
            return;
        }
        let index = tab.history_index - 1;
        let target = tab.history[index].clone();
        self.load_tab(self.active, target, HistoryIntent::Back(index));
    }

    fn preview_to_map(preview: preview::PreviewData) -> QVariantMap {
        let mut map = QVariantMap::default();
        map.insert(
            QString::from("title"),
            QVariant::from(QString::from(preview.title)),
        );
        map.insert(
            QString::from("kind"),
            QVariant::from(QString::from(preview.kind)),
        );
        map.insert(
            QString::from("subtitle"),
            QVariant::from(QString::from(preview.subtitle)),
        );
        map.insert(
            QString::from("text"),
            QVariant::from(QString::from(preview.text)),
        );
        map.insert(
            QString::from("image_url"),
            QVariant::from(QString::from(preview.image_url)),
        );
        map.insert(
            QString::from("page_count"),
            QVariant::from(preview.page_count as i32),
        );
        map.insert(
            QString::from("page_number"),
            QVariant::from(preview.page_number as i32),
        );
        map.insert(
            QString::from("note"),
            QVariant::from(QString::from(preview.note)),
        );
        map.insert(
            QString::from("size_text"),
            QVariant::from(QString::from(preview.size_text)),
        );
        map.insert(
            QString::from("modified_ms"),
            QVariant::from(preview.modified_ms),
        );
        map.insert(QString::from("is_dir"), QVariant::from(preview.is_dir));
        map.insert(
            QString::from("path"),
            QVariant::from(QString::from(preview.path)),
        );
        map.insert(
            QString::from("key"),
            QVariant::from(QString::from(preview.key)),
        );

        let mut archive = QVariantList::default();
        for entry in preview.archive {
            archive.push(QVariant::from(QString::from(entry)));
        }
        map.insert(QString::from("archive"), QVariant::from(archive));
        map
    }

    fn go_forward(&mut self) {
        let Some(tab) = self.tabs_data.get(self.active) else {
            return;
        };
        if tab.history_index + 1 >= tab.history.len() {
            return;
        }
        let index = tab.history_index + 1;
        let target = tab.history[index].clone();
        self.load_tab(self.active, target, HistoryIntent::Forward(index));
    }

    fn open_index(&mut self, index: i32) {
        let Some(entry) = self.active_entries().get(index as usize) else {
            return;
        };
        let path = entry.path.clone();
        let is_dir = entry.is_dir;
        if is_dir {
            self.load_tab(self.active, path, HistoryIntent::New);
        } else {
            self.launch(&path);
        }
    }

    fn open_path(&mut self, path: QString) {
        let path = PathBuf::from(path.to_string());
        if path.is_dir() {
            self.load_tab(self.active, path, HistoryIntent::New);
        } else {
            self.launch(&path);
        }
    }

    fn open_in_new_tab(&mut self, index: i32) {
        let Some(entry) = self.active_entries().get(index as usize) else {
            return;
        };
        let path = entry.path.clone();
        if entry.is_dir {
            self.new_tab(QString::from(path.to_string_lossy().into_owned()));
        } else {
            self.launch(&path);
        }
    }

    fn launch(&mut self, path: &Path) {
        let paths = match absolute_paths(vec![path.to_path_buf()]) {
            Ok(paths) => paths,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        self.launch_files_async(
            default_open_command(&paths),
            paths,
            "Could not open with the default application".to_owned(),
        );
    }

    fn record_recent_file(&mut self, path: &Path) {
        let opened_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
            .unwrap_or(0);
        if opened_at_ms == 0 {
            return;
        }
        let generation = self.recent_generation;
        let id = self.next_recent_id;
        self.next_recent_id = self.next_recent_id.wrapping_add(1).max(1);
        let path = path.to_path_buf();
        self.pending_recent.insert(
            id,
            PendingRecentFile {
                path: path.clone(),
                opened_at_ms,
                generation,
            },
        );
        let qptr = QPointer::from(&*self);
        let callback =
            qmetaobject::queued_callback(move |(id, item): (u64, Option<RecentItem>)| {
                if let Some(backend) = qptr.as_pinned() {
                    backend.borrow_mut().finish_recent_file(id, item);
                }
            });
        std::thread::spawn(move || {
            let item = fs::canonicalize(&path)
                .ok()
                .filter(|canonical| {
                    fs::metadata(canonical).is_ok_and(|metadata| metadata.is_file())
                })
                .map(|canonical| RecentItem {
                    path: canonical.to_string_lossy().into_owned(),
                    opened_at_ms,
                });
            callback((id, item));
        });
    }

    fn finish_recent_file(&mut self, id: u64, item: Option<RecentItem>) {
        let Some(pending) = self.pending_recent.remove(&id) else {
            return;
        };
        if pending.generation != self.recent_generation {
            return;
        }
        if let Some(item) = item {
            self.apply_recent_item(item);
        }
    }

    fn apply_recent_item(&mut self, item: RecentItem) {
        self.recent_store.add_item(item);
        self.persist_recent_store();
        if is_recent_path(&self.current_dir()) {
            if self.search_mode {
                self.clear_search();
            }
            self.refresh();
        }
    }

    fn persist_recent_store(&mut self) {
        let store = self.recent_store.clone();
        let sequence = RECENT_SAVE_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1;
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: Result<(), String>| {
            if let (Err(error), Some(backend)) = (result, qptr.as_pinned()) {
                backend.borrow_mut().set_error(error);
            }
        });
        std::thread::spawn(move || {
            let _guard = RECENT_SAVE_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if sequence != RECENT_SAVE_SEQUENCE.load(Ordering::SeqCst) {
                return;
            }
            callback(store.save());
        });
    }

    fn flush_recent_history(&mut self) {
        let pending = std::mem::take(&mut self.pending_recent);
        let flush_generation = self.recent_generation;
        self.recent_generation = self.recent_generation.wrapping_add(1);
        for item in pending.into_values() {
            if item.generation != flush_generation {
                continue;
            }
            let Ok(path) = fs::canonicalize(&item.path) else {
                continue;
            };
            if !fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
                continue;
            }
            self.recent_store.add_item(RecentItem {
                path: path.to_string_lossy().into_owned(),
                opened_at_ms: item.opened_at_ms,
            });
        }
        RECENT_SAVE_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        let _guard = RECENT_SAVE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Err(error) = self.recent_store.save() {
            self.set_error(error);
        }
    }

    fn open_terminal(&mut self, path: QString) {
        let path = path.to_string();
        if is_virtual_collection(Path::new(&path)) {
            self.set_error("A terminal cannot be opened in a virtual collection.");
            return;
        }
        let terminal = self.terminal.to_string();
        let directory = PathBuf::from(path);
        if terminal.trim().is_empty() {
            self.set_error("No terminal is configured.");
            return;
        }
        // The terminal binary is a single configured name; the path is an
        // argument, never a shell string.
        let result = terminal_command(&terminal, &directory).spawn();
        if let Err(error) = result {
            self.set_error(format!("Could not start {terminal}: {error}"));
        }
    }

    fn toggle_hidden(&mut self) {
        self.config.ui.show_hidden = !self.config.ui.show_hidden;
        self.show_hidden = self.config.ui.show_hidden;
        self.view_changed();
        self.persist_config();
        self.refresh();
        // Hidden entries are a view preference shared by every tab, but only
        // the active tab reloads immediately; other tabs reload when shown.
        let active = self.active;
        for index in 0..self.tabs_data.len() {
            if index != active {
                let path = self.tabs_data[index].path.clone();
                self.load_tab(index, path, HistoryIntent::Current);
            }
        }
    }

    fn toggle_preview(&mut self) {
        self.config.ui.preview_visible = !self.config.ui.preview_visible;
        self.preview_visible = self.config.ui.preview_visible;
        self.view_changed();
        self.persist_config();
    }

    fn set_view_mode(&mut self, mode: QString) {
        let mode = mode.to_string();
        let normalized = match mode.as_str() {
            "list" | "grid" | "column" => mode,
            _ => return,
        };
        if self.config.ui.view_mode == normalized && self.view_mode.to_string() == normalized {
            return;
        }
        self.config.set_view_mode(&normalized);
        self.view_mode = QString::from(normalized.clone());
        self.view_changed();
        (self as &mut dyn QAbstractListModel).begin_reset_model();
        (self as &mut dyn QAbstractListModel).end_reset_model();
        self.sync_active(false);
        self.load_expanded_tree_folders(self.active);
        self.persist_config();
        self.save_session();
    }

    fn set_sort(&mut self, key: QString, ascending: bool) {
        let Ok(key) = key.to_string().parse::<SortKey>() else {
            return;
        };
        if let Some(tab) = self.tabs_data.get_mut(self.active) {
            tab.sort.key = key;
            tab.sort.ascending = ascending;
        }
        self.sort_key = QString::from(key.as_str());
        self.sort_ascending = ascending;
        self.view_changed();
        self.config.set_sort(key, ascending);
        self.persist_config();
        self.refresh();
    }

    fn toggle_directories_first(&mut self) {
        let value = !self.directories_first;
        if let Some(tab) = self.tabs_data.get_mut(self.active) {
            tab.sort.directories_first = value;
        }
        self.directories_first = value;
        self.config.ui.directories_first = value;
        self.view_changed();
        self.persist_config();
        self.refresh();
    }

    fn set_single_click_open(&mut self, enabled: bool) {
        self.config.ui.single_click_open = enabled;
        self.single_click_open = enabled;
        self.behavior_changed();
        self.persist_config();
    }

    fn toggle_sidebar(&mut self) {
        self.config.ui.sidebar_visible = !self.config.ui.sidebar_visible;
        self.sidebar_visible = self.config.ui.sidebar_visible;
        self.behavior_changed();
        self.persist_config();
    }

    fn toggle_tab_bar(&mut self) {
        self.config.ui.tab_bar_visible = !self.config.ui.tab_bar_visible;
        self.tab_bar_visible = self.config.ui.tab_bar_visible;
        self.behavior_changed();
        self.persist_config();
    }

    fn set_sidebar_width(&mut self, width: i32) {
        let width = width.clamp(160, 420);
        if self.sidebar_width == width {
            return;
        }
        self.sidebar_width = width;
        self.config.ui.sidebar_width = width;
        self.behavior_changed();
        self.persist_config();
    }

    fn set_drag_hover_delay(&mut self, delay_ms: i32) {
        let delay_ms = delay_ms.clamp(200, 5000) as u32;
        if self.config.ui.drag_hover_delay_ms == delay_ms {
            return;
        }
        self.config.ui.drag_hover_delay_ms = delay_ms;
        self.drag_hover_delay_ms = delay_ms as i32;
        self.behavior_changed();
        self.persist_config();
    }

    fn toggle_column(&mut self, column: QString) {
        match column.to_string().as_str() {
            "size" => self.config.ui.show_size = !self.config.ui.show_size,
            "modified" => self.config.ui.show_modified = !self.config.ui.show_modified,
            "kind" => self.config.ui.show_kind = !self.config.ui.show_kind,
            _ => return,
        }
        self.show_size = self.config.ui.show_size;
        self.show_modified = self.config.ui.show_modified;
        self.show_kind = self.config.ui.show_kind;
        self.view_changed();
        self.persist_config();
    }

    fn home(&mut self) {
        self.load_tab(self.active, home_dir(), HistoryIntent::New);
    }

    fn trash(&mut self) {
        // `trash_dir()` is the `files/` directory itself; its sibling `info/`
        // holds the restore metadata written by ops::trash().
        let files = trash_dir();
        if !files.is_dir() && fs::create_dir_all(&files).is_err() {
            self.set_error(format!(
                "Could not create the trash at {}.",
                files.display()
            ));
            return;
        }
        if let Some(root) = files.parent() {
            let _ = fs::create_dir_all(root.join("info"));
        }
        self.load_tab(self.active, files, HistoryIntent::New);
    }

    fn clear_error(&mut self) {
        if let Some(tab) = self.tabs_data.get_mut(self.active) {
            tab.error.clear();
        }
        self.error = QString::from("");
        self.error_changed();
    }

    fn is_dir_at(&self, index: i32) -> bool {
        self.active_entries()
            .get(index as usize)
            .map(|entry| entry.is_dir)
            .unwrap_or(false)
    }

    fn set_tree_path_expanded(&mut self, path: QString, expanded: bool) {
        let path = PathBuf::from(path.to_string());
        let Some(index) = self
            .active_entries()
            .iter()
            .position(|entry| entry.path == path)
        else {
            return;
        };
        self.set_tree_expanded(index as i32, expanded);
    }

    fn set_tree_expanded(&mut self, index: i32, expanded: bool) {
        if self.view_mode.to_string() != "list" {
            return;
        }
        let Some(entry) = self.active_entries().get(index as usize) else {
            return;
        };
        if !entry.is_dir || entry.is_symlink {
            return;
        }
        let path = entry.path.clone();
        let Some(tab) = self.tabs_data.get_mut(self.active) else {
            return;
        };
        if tab.search.is_some() {
            return;
        }
        if !set_expanded_path(&mut tab.tree_expanded, path.clone(), expanded) {
            return;
        }
        let active = self.active;
        let parent_depth = self.tabs_data[active]
            .tree_rows
            .iter()
            .find(|row| row.path == path)
            .map_or(0, |row| row.tree_depth);
        let needs_load = expanded && !self.tabs_data[active].tree_children.contains_key(&path);
        if let Some(row) = self.tabs_data[active]
            .tree_rows
            .iter_mut()
            .find(|row| row.path == path)
        {
            row.tree_expanded = expanded;
        }

        if expanded {
            let descendants = self.tabs_data[active]
                .tree_children
                .get(&path)
                .map(|children| {
                    let mut rows = Vec::new();
                    flatten_tree_children(
                        children,
                        &self.tabs_data[active].tree_children,
                        &self.tabs_data[active].tree_expanded,
                        parent_depth + 1,
                        &mut HashSet::new(),
                        &mut rows,
                    );
                    rows
                })
                .unwrap_or_default();
            self.replace_tree_descendants(active, &path, descendants);
        } else {
            self.replace_tree_descendants(active, &path, Vec::new());
        }
        self.notify_tree_row_changed(&path);
        if needs_load {
            self.start_tree_load(active, path);
        }
        self.ensure_watch();
    }

    fn tree_expanded_at(&self, index: i32) -> bool {
        self.view_mode.to_string() == "list"
            && self
                .tabs_data
                .get(self.active)
                .is_some_and(|tab| tab.search.is_none() && !is_virtual_collection(&tab.path))
            && self
                .active_entries()
                .get(index as usize)
                .is_some_and(|entry| entry.tree_expanded)
    }

    fn tree_expandable_at(&self, index: i32) -> bool {
        self.view_mode.to_string() == "list"
            && self
                .tabs_data
                .get(self.active)
                .is_some_and(|tab| tab.search.is_none() && !is_virtual_collection(&tab.path))
            && self
                .active_entries()
                .get(index as usize)
                .is_some_and(|entry| entry.is_dir && !entry.is_symlink)
    }

    fn tree_depth_at(&self, index: i32) -> i32 {
        self.active_entries()
            .get(index as usize)
            .map(|entry| entry.tree_depth.min(i32::MAX as usize) as i32)
            .unwrap_or(-1)
    }

    fn tree_parent_index(&self, index: i32) -> i32 {
        if self.view_mode.to_string() != "list"
            || self
                .tabs_data
                .get(self.active)
                .is_none_or(|tab| tab.search.is_some() || is_virtual_collection(&tab.path))
        {
            return -1;
        }
        let entries = self.active_entries();
        let Some(entry) = entries.get(index as usize) else {
            return -1;
        };
        if entry.tree_depth == 0 {
            return -1;
        }
        entries[..index as usize]
            .iter()
            .rposition(|candidate| candidate.tree_depth + 1 == entry.tree_depth)
            .map(|parent| parent as i32)
            .unwrap_or(-1)
    }

    fn path_at(&self, index: i32) -> QString {
        self.active_entries()
            .get(index as usize)
            .map(|entry| QString::from(entry.path.to_string_lossy().into_owned()))
            .unwrap_or_default()
    }

    fn paths_in_range(&self, first: i32, last: i32) -> QVariantList {
        let entries = self.active_entries();
        let start = first.max(0) as usize;
        if last < first || start >= entries.len() {
            return QVariantList::default();
        }

        entries
            .iter()
            .skip(start)
            .take(last as usize - start + 1)
            .map(|entry| QVariant::from(QString::from(entry.path.to_string_lossy().into_owned())))
            .collect()
    }

    fn size_at(&self, index: i32) -> f64 {
        self.active_entries()
            .get(index as usize)
            .map(|entry| entry.size as f64)
            .unwrap_or(0.0)
    }

    fn name_at(&self, index: i32) -> QString {
        self.active_entries()
            .get(index as usize)
            .map(|entry| QString::from(entry.name.clone()))
            .unwrap_or_default()
    }

    fn index_of_path(&self, path: QString) -> i32 {
        let path = path.to_string();
        self.active_entries()
            .iter()
            .position(|entry| entry.path.to_string_lossy() == path)
            .map(|index| index as i32)
            .unwrap_or(-1)
    }

    /// Drop selected paths that are no longer in the current listing, which is
    /// how a tab switch or refresh keeps its selection without keeping ghosts.
    fn filter_present(&self, paths: QVariantList) -> QVariantList {
        let wanted: std::collections::HashSet<String> = list_paths(&paths)
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        let mut present = QVariantList::default();
        for entry in self.active_entries() {
            let path = entry.path.to_string_lossy();
            if wanted.contains(path.as_ref()) {
                present.push(QVariant::from(QString::from(path.into_owned())));
            }
        }
        present
    }

    /// Selected count and byte size, computed once in Rust rather than by
    /// walking the model from QML for every selection change.
    fn selection_info(&self, paths: QVariantList) -> QVariantMap {
        let wanted: std::collections::HashSet<String> = list_paths(&paths)
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        let mut count = 0usize;
        let mut files = 0usize;
        let mut folders = 0usize;
        let mut bytes = 0u64;
        for entry in self.active_entries() {
            let path = entry.path.to_string_lossy();
            if !wanted.contains(path.as_ref()) {
                continue;
            }
            count += 1;
            if entry.is_dir {
                folders += 1;
            } else {
                files += 1;
                bytes = bytes.saturating_add(entry.size);
            }
        }
        let text = if count == 0 {
            String::new()
        } else if files == 0 {
            format!("{count} {} selected", item_word(count))
        } else if folders == 0 {
            format!("{count} selected · {}", format_size(bytes))
        } else {
            format!(
                "{count} selected · {} + {folders} {}",
                format_size(bytes),
                if folders == 1 { "folder" } else { "folders" }
            )
        };
        let mut map = QVariantMap::default();
        map.insert(QString::from("count"), QVariant::from(count as i32));
        map.insert(QString::from("files"), QVariant::from(files as i32));
        map.insert(QString::from("folders"), QVariant::from(folders as i32));
        map.insert(QString::from("bytes"), QVariant::from(bytes as f64));
        map.insert(QString::from("text"), QVariant::from(QString::from(text)));
        map
    }

    // ---- Tabs -----------------------------------------------------------

    fn new_tab(&mut self, path: QString) {
        let path = path.to_string();
        let target = if path.trim().is_empty() {
            self.current_dir()
        } else {
            match paths::expand_path(&path, &self.current_dir()) {
                Some(target) => target,
                None => {
                    self.set_error(format!("Not a valid path: {path}"));
                    return;
                }
            }
        };
        let sort = self
            .tabs_data
            .get(self.active)
            .map(|tab| tab.sort)
            .unwrap_or_else(|| self.config.sort_spec());
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let index = self.active + 1;
        self.tabs_data
            .insert(index, Tab::new(id, target.clone(), sort));
        self.active = index;
        self.rebuild_tabs_property();
        self.sync_active(true);
        self.load_tab(index, target, HistoryIntent::New);
        self.save_session();
    }

    fn close_tab(&mut self, index: i32) {
        let index = index as usize;
        if index >= self.tabs_data.len() {
            return;
        }
        if self.tabs_data.len() == 1 {
            self.status = QString::from("The last tab stays open. Use Ctrl+Q to quit.");
            self.status_changed();
            return;
        }
        let removed = self.tabs_data.remove(index);
        self.closed_tabs.push(removed.path.clone());
        if self.closed_tabs.len() > 12 {
            self.closed_tabs.remove(0);
        }
        if self.active >= index && self.active > 0 {
            self.active -= 1;
        }
        if self.active >= self.tabs_data.len() {
            self.active = self.tabs_data.len() - 1;
        }
        self.rebuild_tabs_property();
        self.sync_active(true);
        self.ensure_watch();
        self.save_session();
    }

    fn select_tab(&mut self, index: i32) {
        let index = index as usize;
        if index >= self.tabs_data.len() || index == self.active {
            return;
        }
        self.active = index;
        self.sync_active(true);
        self.ensure_watch();
        // A tab whose content is stale (never loaded, or reloaded while hidden)
        // is loaded now.
        if self.tabs_data[index].entries.is_empty() && !self.tabs_data[index].loading {
            let path = self.tabs_data[index].path.clone();
            self.load_tab(index, path, HistoryIntent::Current);
        }
        self.save_session();
    }

    fn duplicate_tab(&mut self, index: i32) {
        let index = index as usize;
        let Some(tab) = self.tabs_data.get(index) else {
            return;
        };
        let path = tab.path.clone();
        let sort = tab.sort;
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let insert = index + 1;
        self.tabs_data
            .insert(insert, Tab::new(id, path.clone(), sort));
        self.active = insert;
        self.rebuild_tabs_property();
        self.sync_active(true);
        self.load_tab(insert, path, HistoryIntent::New);
        self.save_session();
    }

    fn reopen_tab(&mut self) {
        let Some(path) = self.closed_tabs.pop() else {
            self.status = QString::from("There is no closed tab to reopen.");
            self.status_changed();
            return;
        };
        self.new_tab(QString::from(path.to_string_lossy().into_owned()));
    }

    fn move_tab(&mut self, from: i32, to: i32) {
        let from = from as usize;
        let to = to as usize;
        if from >= self.tabs_data.len() || to >= self.tabs_data.len() || from == to {
            return;
        }
        let tab = self.tabs_data.remove(from);
        let active_id = self.tabs_data.get(self.active).map(|tab| tab.id);
        self.tabs_data.insert(to, tab);
        if let Some(id) = active_id {
            if let Some(index) = self.tab_index(id) {
                self.active = index;
            }
        }
        self.rebuild_tabs_property();
        self.save_session();
    }

    fn next_tab(&mut self) {
        if self.tabs_data.len() < 2 {
            return;
        }
        let next = (self.active + 1) % self.tabs_data.len();
        self.select_tab(next as i32);
    }

    fn prev_tab(&mut self) {
        if self.tabs_data.len() < 2 {
            return;
        }
        let prev = if self.active == 0 {
            self.tabs_data.len() - 1
        } else {
            self.active - 1
        };
        self.select_tab(prev as i32);
    }

    fn rebuild_tabs_property(&mut self) {
        let mut list = QVariantList::default();
        for (index, tab) in self.tabs_data.iter().enumerate() {
            let mut map = QVariantMap::default();
            map.insert(QString::from("index"), QVariant::from(index as i32));
            map.insert(QString::from("id"), QVariant::from(tab.id as f64));
            map.insert(
                QString::from("title"),
                QVariant::from(QString::from(tab.title())),
            );
            map.insert(
                QString::from("path"),
                QVariant::from(QString::from(tab.path.to_string_lossy().into_owned())),
            );
            map.insert(
                QString::from("active"),
                QVariant::from(index == self.active),
            );
            map.insert(
                QString::from("loading"),
                QVariant::from(tab.shows_loading_indicator()),
            );
            map.insert(
                QString::from("searching"),
                QVariant::from(tab.search.is_some()),
            );
            list.push(QVariant::from(map));
        }
        self.tabs = list;
        self.active_tab = self.active as i32;
        self.active_tab_id = self
            .tabs_data
            .get(self.active)
            .map(|tab| tab.id as i32)
            .unwrap_or(0);
        self.tabs_changed();
    }

    fn save_session(&mut self) {
        if !self.config.ui.restore_session {
            return;
        }
        let tabs: Vec<SessionTab> = self
            .tabs_data
            .iter()
            .map(|tab| SessionTab {
                path: tab.path.to_string_lossy().into_owned(),
                view: self.config.ui.view_mode.clone(),
                sort_key: tab.sort.key.as_str().to_owned(),
                sort_ascending: tab.sort.ascending,
                directories_first: tab.sort.directories_first,
            })
            .collect();
        let session = Session {
            tabs,
            active: self.active,
            dual_pane: false,
            pane2_tabs: Vec::new(),
            pane2_active: 0,
        };
        if let Err(error) = session.save() {
            self.set_error(error);
        }
    }

    fn save_window(&mut self, width: i32, height: i32) {
        self.config.ui.window_width = width.clamp(720, 8000);
        self.config.ui.window_height = height.clamp(480, 8000);
        self.persist_config();
    }

    // ---- Clipboard and operations ---------------------------------------

    fn copy_paths(&mut self, paths: QVariantList) {
        self.set_clipboard(list_paths(&paths), false);
    }

    fn cut_paths(&mut self, paths: QVariantList) {
        self.set_clipboard(list_paths(&paths), true);
    }

    fn apply_clipboard_snapshot(&mut self, clipboard: Option<Clipboard>, revision: u64) {
        let mut paths = QVariantList::default();
        if let Some(clipboard) = &clipboard {
            for path in &clipboard.paths {
                paths.push(QVariant::from(QString::from(
                    path.to_string_lossy().into_owned(),
                )));
            }
        }
        self.clipboard_count = paths.len().min(i32::MAX as usize) as i32;
        self.clipboard_cut = clipboard.as_ref().is_some_and(|clipboard| clipboard.cut);
        self.clipboard_paths = paths;
        self.clipboard = clipboard;
        self.clipboard_revision = revision;
        self.clipboard_changed();
    }

    fn refresh_clipboard_state(&mut self) {
        if let Some(mut external) = sysclip::read_clipboard() {
            external.paths.retain(|path| path.exists());
            external.paths.sort();
            external.paths = ops::normalize_transfer_sources(&external.paths);
            let current = {
                let mut shared = shared_clipboard()
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if shared.last_external_clipboard.as_ref() == Some(&external) {
                    None
                } else {
                    shared.last_external_clipboard = Some(external.clone());
                    Some(shared.clipboard.clone())
                }
            };
            if let Some(current) = current {
                let matches = current.as_ref().is_some_and(|clipboard| {
                    clipboard.paths == external.paths
                        && external.cut.map_or(true, |cut| clipboard.cut == cut)
                }) || (current.is_none() && external.paths.is_empty());
                if !matches {
                    let clipboard = (!external.paths.is_empty()).then_some(Clipboard {
                        paths: external.paths,
                        cut: external.cut.unwrap_or(false),
                    });
                    publish_shared_clipboard(clipboard);
                }
            }
        }

        self.refresh_shared_clipboard_state();
    }

    fn refresh_shared_clipboard_state(&mut self) {
        let (revision, clipboard) = {
            let shared = shared_clipboard()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            (shared.revision, shared.clipboard.clone())
        };
        if revision != self.clipboard_revision {
            self.apply_clipboard_snapshot(clipboard, revision);
        }
    }

    fn update_clipboard_path(&mut self, from: &Path, to: &Path) {
        let snapshot = {
            let mut shared = shared_clipboard()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let Some(clipboard) = &mut shared.clipboard else {
                return;
            };
            let mut changed = false;
            for path in &mut clipboard.paths {
                if path == from {
                    *path = to.to_path_buf();
                    changed = true;
                }
            }
            if !changed {
                return;
            }
            shared.revision = shared.revision.wrapping_add(1);
            (shared.clipboard.clone(), shared.revision)
        };
        self.apply_clipboard_snapshot(snapshot.0.clone(), snapshot.1);
        if let Some(clipboard) = &snapshot.0 {
            self.remember_system_clipboard();
            let _ = sysclip::write_paths(&clipboard.paths);
        }
    }

    fn remember_system_clipboard(&self) {
        let Some(mut external) = sysclip::read_clipboard() else {
            return;
        };
        external.paths.retain(|path| path.exists());
        external.paths.sort();
        external.paths.dedup();
        let mut shared = shared_clipboard()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        shared.last_external_clipboard = Some(external);
    }

    fn complete_clipboard_paths(&mut self, completed_paths: &[PathBuf]) {
        let Some(mut clipboard) = self.clipboard.clone() else {
            return;
        };
        let original_count = clipboard.paths.len();
        clipboard
            .paths
            .retain(|path| !completed_paths.iter().any(|completed| completed == path));
        if clipboard.paths.len() == original_count {
            return;
        }
        let next = (!clipboard.paths.is_empty()).then_some(clipboard);
        let (next, revision) = publish_shared_clipboard(next);
        self.apply_clipboard_snapshot(next.clone(), revision);
        self.remember_system_clipboard();
        if let Some(clipboard) = next {
            let _ = sysclip::write_paths(&clipboard.paths);
        } else {
            let _ = sysclip::clear();
        }
    }

    fn set_clipboard(&mut self, mut paths: Vec<PathBuf>, cut: bool) {
        paths.retain(|path| path.exists());
        paths.sort();
        paths = ops::normalize_transfer_sources(&paths);
        if paths.is_empty() {
            self.set_error("Nothing to put on the clipboard.");
            return;
        }
        self.remember_system_clipboard();
        match sysclip::write_paths(&paths) {
            Ok(()) => {}
            Err(note) => {
                self.status = QString::from(note);
                self.status_changed();
            }
        }
        let count = paths.len();
        let clipboard = Clipboard { paths, cut };
        let (clipboard, revision) = publish_shared_clipboard(Some(clipboard));
        self.apply_clipboard_snapshot(clipboard, revision);
        self.status = QString::from(format!(
            "{} {count} {}",
            if cut { "Cut" } else { "Copied" },
            item_word(count)
        ));
        self.status_changed();
    }

    fn drop_paths(
        &mut self,
        dest: QString,
        paths: QVariantList,
        move_items: bool,
        auto_move: bool,
        policy: QString,
    ) {
        self.transfer_dropped_paths(dest, list_paths(&paths), move_items, auto_move, policy);
    }

    fn drop_external_paths(&mut self, dest: QString, urls: QVariantList, move_items: bool) {
        let dest = PathBuf::from(dest.to_string());
        let mut paths = Vec::with_capacity(urls.len());
        for value in 0..urls.len() {
            let Some(url) = QString::from_qvariant(urls[value].clone()) else {
                self.set_error("The dropped item has an invalid URL.");
                return;
            };
            let url = url.to_string();
            if !url.starts_with("file://") {
                self.set_error("Only files from this computer can be dropped here.");
                return;
            }
            let Some(path) = paths::expand_path(&url, &dest) else {
                self.set_error("The dropped file URL is invalid.");
                return;
            };
            paths.push(path);
        }
        self.transfer_dropped_paths(
            QString::from(dest.to_string_lossy().into_owned()),
            paths,
            move_items,
            false,
            QString::from("ask"),
        );
    }

    fn transfer_dropped_paths(
        &mut self,
        dest: QString,
        sources: Vec<PathBuf>,
        move_items: bool,
        auto_move: bool,
        policy: QString,
    ) {
        if self.op_active {
            self.set_error("Another operation is still running.");
            return;
        }
        let dest = PathBuf::from(dest.to_string());
        if is_virtual_collection(&dest) {
            self.set_error("Choose a real folder as the transfer destination.");
            return;
        }
        if !dest.is_dir() {
            self.set_error(format!("{} is not a folder.", dest.display()));
            return;
        }
        if sources.is_empty() {
            self.set_error("There are no files to transfer.");
            return;
        }
        let move_items = if auto_move {
            same_filesystem_selection(&sources, &dest)
        } else {
            move_items
        };
        self.execute_transfer_paths(dest, sources, move_items, policy, false);
    }

    fn paste(&mut self, dest: QString, policy: QString) {
        if self.op_active {
            self.set_error("Another operation is still running.");
            return;
        }
        self.refresh_clipboard_state();
        let dest = PathBuf::from(dest.to_string());
        if is_virtual_collection(&dest) {
            self.set_error("Choose a real folder as the transfer destination.");
            return;
        }
        if !dest.is_dir() {
            self.set_error(format!("{} is not a folder.", dest.display()));
            return;
        }
        let (sources, cut) = match self.clipboard.clone() {
            Some(clipboard) => (clipboard.paths, clipboard.cut),
            None => {
                let external = sysclip::read_paths();
                if external.is_empty() {
                    self.set_error("There are no files on the clipboard.");
                    return;
                }
                (external, false)
            }
        };
        if sources.is_empty() {
            self.set_error("There are no files on the clipboard.");
            return;
        }
        self.execute_transfer_paths(dest, sources, cut, policy, true);
    }

    fn execute_transfer_paths(
        &mut self,
        dest: PathBuf,
        sources: Vec<PathBuf>,
        cut: bool,
        policy: QString,
        from_clipboard: bool,
    ) {
        let sources = ops::normalize_transfer_sources(&sources);
        if sources.is_empty() {
            self.set_error("There are no files to transfer.");
            return;
        }
        let policy_value = ConflictPolicy::parse(&policy.to_string());
        if policy_value == Some(ConflictPolicy::Ask) || policy_value.is_none() {
            let mut names = Vec::new();
            for source in &sources {
                let Some(name) = source.file_name() else {
                    continue;
                };
                let target = dest.join(name);
                if target.exists() && target != *source {
                    names.push(name.to_string_lossy().into_owned());
                }
            }
            if !names.is_empty() {
                let mut list = QVariantList::default();
                for name in &names {
                    list.push(QVariant::from(QString::from(name.clone())));
                }
                self.conflict_names = list;
                self.conflict_detail = QString::from(format!(
                    "{} {} already {} in {}.",
                    names.len(),
                    item_word(names.len()),
                    if names.len() == 1 { "exists" } else { "exist" },
                    ops::display_path(&dest)
                ));
                self.conflict_pending = true;
                self.conflict_changed();
                self.pending_conflict = Some(PendingConflict {
                    sources,
                    dest,
                    cut,
                    from_clipboard,
                });
                return;
            }
            self.start_transfer(dest, sources, cut, ConflictPolicy::Skip, from_clipboard);
            return;
        }
        self.start_transfer(dest, sources, cut, policy_value.unwrap(), from_clipboard);
    }

    fn cancel_conflicts(&mut self) {
        self.pending_conflict = None;
        self.clear_conflict_property();
        self.status = QString::from("Paste cancelled.");
        self.status_changed();
    }

    fn confirm_conflicts(&mut self, policy: QString) {
        let Some(pending) = self.pending_conflict.take() else {
            return;
        };
        self.clear_conflict_property();
        let policy = ConflictPolicy::parse(&policy.to_string()).unwrap_or(ConflictPolicy::Skip);
        self.start_transfer(
            pending.dest,
            pending.sources,
            pending.cut,
            policy,
            pending.from_clipboard,
        );
    }

    fn clear_conflict_property(&mut self) {
        self.conflict_pending = false;
        self.conflict_names = QVariantList::default();
        self.conflict_detail = QString::from("");
        self.conflict_changed();
    }

    fn begin_operation(&mut self, title: String, cancelable: bool) -> u64 {
        self.op_generation += 1;
        let generation = self.op_generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel_flag = Some(cancel);
        self.op_active = true;
        self.op_title = QString::from(title);
        self.op_detail = QString::from("");
        self.op_progress = 0.0;
        self.op_cancelable = cancelable;
        self.op_changed();
        generation
    }

    fn finish_operation(&mut self) {
        self.op_active = false;
        self.op_detail = QString::from("");
        self.op_progress = 0.0;
        self.op_cancelable = false;
        self.op_changed();
        self.cancel_flag = None;
    }

    fn non_transfer_operation_busy(&self) -> bool {
        self.op_active || transfer::is_busy()
    }

    fn start_transfer(
        &mut self,
        dest: PathBuf,
        sources: Vec<PathBuf>,
        cut: bool,
        policy: ConflictPolicy,
        from_clipboard: bool,
    ) {
        if self.op_active {
            self.set_error("Another operation is still running.");
            return;
        }
        let total = sources.len();
        let operation = if cut {
            Operation::Move
        } else {
            Operation::Copy
        };
        let qptr = QPointer::from(&*self);
        let done_cb = qmetaobject::queued_callback(move |done: TransferFinished| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_transfer_finished(done);
            }
        });

        let enqueue_result = transfer::enqueue(
            operation,
            sources.clone(),
            Some(dest.clone()),
            move |context| {
                set_transfer_inventory(&context, ops::transfer_inventory(&sources));

                let cancel = context.cancellation_token();
                let mut events =
                    |path: &Path, is_directory: bool, event: ops::ItemEvent| match event {
                        ops::ItemEvent::Started => context.start_item(path),
                        ops::ItemEvent::Completed if is_directory => {
                            context.finish_item(path, true, ItemResult::Completed, "");
                            context.finish_subtree(path, ItemResult::Completed, "");
                        }
                        ops::ItemEvent::Completed => {
                            context.finish_item(path, is_directory, ItemResult::Completed, "")
                        }
                        ops::ItemEvent::Skipped => {
                            context.finish_item(path, is_directory, ItemResult::Skipped, "");
                            context.finish_subtree(path, ItemResult::Skipped, "");
                        }
                        ops::ItemEvent::Cancelled => {
                            context.finish_item(path, is_directory, ItemResult::Cancelled, "");
                            context.finish_subtree(path, ItemResult::Cancelled, "");
                        }
                        ops::ItemEvent::Failed(error) => {
                            context.finish_item(
                                path,
                                is_directory,
                                ItemResult::Failed,
                                error.clone(),
                            );
                            context.finish_subtree(path, ItemResult::Failed, error);
                        }
                    };
                let outcome = ops::transfer_with_events(
                    &sources,
                    &dest,
                    cut,
                    policy,
                    &cancel,
                    &mut events,
                    &mut |progress| {
                        if !progress.path.as_os_str().is_empty() {
                            context.report_file_progress(
                                &progress.path,
                                progress.bytes_done,
                                progress.file_bytes,
                            );
                        }
                    },
                );

                let mut message = if outcome.cancelled {
                    format!(
                        "Cancelled: {} of {total} {} processed.",
                        outcome.created.len() + outcome.moves.len(),
                        item_word(total)
                    )
                } else if cut {
                    format!(
                        "Moved {} {}",
                        outcome.moves.len(),
                        item_word(outcome.moves.len())
                    )
                } else {
                    format!(
                        "Copied {} {}",
                        outcome.created.len(),
                        item_word(outcome.created.len())
                    )
                };
                if !outcome.skipped.is_empty() {
                    message.push_str(&format!(" · {} skipped", outcome.skipped.len()));
                }
                if !outcome.failed.is_empty() {
                    message.push_str(&format!(" · {} failed", outcome.failed.len()));
                }
                let undo = if cut {
                    (!outcome.moves.is_empty()).then(|| UndoOp::Move {
                        pairs: outcome.moves.clone(),
                    })
                } else {
                    (!outcome.created.is_empty()).then(|| UndoOp::Copy {
                        created: outcome.created.clone(),
                    })
                };
                let error = if outcome.failed.is_empty() {
                    None
                } else {
                    Some(format!(
                        "{} {} failed: {}",
                        outcome.failed.len(),
                        item_word(outcome.failed.len()),
                        summarize_failures(&outcome.failed)
                    ))
                };
                done_cb(TransferFinished {
                    undo,
                    completed_sources: outcome.completed_sources.clone(),
                    from_clipboard,
                    message: message.clone(),
                    error: error.clone(),
                });
                if outcome.cancelled {
                    JobResult::cancelled(message)
                } else if let Some(error) = error {
                    JobResult::failed(message, error)
                } else {
                    JobResult::completed(message)
                }
            },
        );
        if let Err(error) = enqueue_result {
            self.set_error(error);
            return;
        }
        self.refresh_transfer_jobs();
    }

    fn apply_transfer_finished(&mut self, done: TransferFinished) {
        let cancelled = done.message.to_ascii_lowercase().contains("cancel");
        let title = if cancelled {
            "File operation cancelled"
        } else if done.error.is_some() {
            "File operation failed"
        } else {
            "File operation complete"
        };
        let body = done.error.as_ref().map_or_else(
            || done.message.clone(),
            |error| format!("{}\n{error}", done.message),
        );
        notifications::send(title, body);

        if done.from_clipboard && !done.completed_sources.is_empty() {
            self.complete_clipboard_paths(&done.completed_sources);
        }
        if let Some(undo) = done.undo {
            self.push_undo(undo);
        }
        if let Some(error) = done.error {
            self.set_transfer_error(error);
        } else {
            self.error = QString::from("");
            self.error_changed();
            if let Some(tab) = self.tabs_data.get_mut(self.active) {
                tab.error.clear();
            }
        }
        self.status = QString::from(done.message);
        self.status_changed();
        self.refresh_transfer_jobs();
        self.refresh();
    }

    fn refresh_transfer_jobs(&mut self) {
        let jobs = transfer::snapshots();
        crate::transfer_export::publish(&jobs);
        let mut list = QVariantList::default();
        for job in &jobs {
            list.push(QVariant::from(transfer_job_variant(job)));
        }
        self.transfer_jobs = list;
        self.transfer_jobs_changed();
    }

    fn transfer_job_items(&self, id: QString) -> QVariantList {
        let Ok(id) = id.to_string().parse::<u64>() else {
            return QVariantList::default();
        };
        let Some(job) = transfer::job(id) else {
            return QVariantList::default();
        };
        let mut list = QVariantList::default();
        for item in job.items {
            let mut map = QVariantMap::default();
            map.insert(
                QString::from("path"),
                QVariant::from(QString::from(item.path.to_string_lossy().into_owned())),
            );
            map.insert(
                QString::from("name"),
                QVariant::from(QString::from(
                    item.path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| item.path.display().to_string()),
                )),
            );
            map.insert(
                QString::from("isDirectory"),
                QVariant::from(item.is_directory),
            );
            map.insert(
                QString::from("size"),
                QVariant::from(QString::from(format_size(item.size_bytes))),
            );
            map.insert(
                QString::from("status"),
                QVariant::from(QString::from(item.status.as_str())),
            );
            map.insert(
                QString::from("error"),
                QVariant::from(QString::from(item.error)),
            );
            list.push(QVariant::from(map));
        }
        list
    }

    fn cancel_transfer_job(&mut self, id: QString) {
        match id.to_string().parse::<u64>() {
            Ok(id) if transfer::cancel(id) => self.refresh_transfer_jobs(),
            _ => self.set_error("That transfer job is no longer available."),
        }
    }

    fn cancel_pending_transfers(&mut self) {
        transfer::cancel_pending();
        self.refresh_transfer_jobs();
    }

    fn clear_completed_transfers(&mut self) {
        transfer::clear_completed();
        self.refresh_transfer_jobs();
    }

    fn retry_transfer_job(&mut self, id: QString) {
        let Ok(id) = id.to_string().parse::<u64>() else {
            self.set_error("That transfer job is no longer available.");
            return;
        };
        let Some(job) = transfer::job(id) else {
            self.set_error("That transfer job is no longer available.");
            return;
        };
        if job.status != transfer::JobStatus::Failed {
            self.set_error("Only failed transfer jobs can be retried.");
            return;
        }
        let failed_sources = job
            .items
            .iter()
            .filter(|item| {
                item.status == transfer::ItemStatus::Failed && job.sources.contains(&item.path)
            })
            .map(|item| item.path.clone())
            .collect::<Vec<_>>();
        let sources = if failed_sources.is_empty() {
            job.sources.clone()
        } else {
            failed_sources
        };
        match job.operation {
            Operation::Copy | Operation::Move => {
                let Some(dest) = job.destination else {
                    self.set_error("The transfer destination is no longer available.");
                    return;
                };
                self.start_transfer(
                    dest,
                    sources,
                    job.operation == Operation::Move,
                    ConflictPolicy::Skip,
                    false,
                );
            }
            Operation::Trash => {
                let mut paths = QVariantList::default();
                for path in sources {
                    paths.push(QVariant::from(QString::from(
                        path.to_string_lossy().into_owned(),
                    )));
                }
                self.trash_paths(paths);
            }
            Operation::Delete => {
                let mut paths = QVariantList::default();
                for path in sources {
                    paths.push(QVariant::from(QString::from(
                        path.to_string_lossy().into_owned(),
                    )));
                }
                self.delete_paths(paths, true);
            }
            Operation::EmptyTrash => self.empty_trash(),
            Operation::Restore => {
                self.set_error("Restore jobs must be retried from the Trash view.");
            }
        }
    }

    fn apply_op_tick(&mut self, tick: OpTick) {
        if tick.generation != self.op_generation || !self.op_active {
            return;
        }
        self.op_detail = QString::from(tick.detail);
        self.op_progress = tick.progress;
        self.op_changed();
    }

    fn push_undo(&mut self, op: UndoOp) {
        self.undo_stack.push(op);
        if self.undo_stack.len() > MAX_UNDO_STEPS {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.sync_undo_props();
    }

    fn sync_undo_props(&mut self) {
        self.can_undo = !self.undo_stack.is_empty();
        self.can_redo = !self.redo_stack.is_empty();
        self.undo_label = QString::from(
            self.undo_stack
                .last()
                .map(|op| op.description())
                .unwrap_or_default(),
        );
        self.redo_label = QString::from(
            self.redo_stack
                .last()
                .map(|op| op.description())
                .unwrap_or_default(),
        );
        self.undo_changed();
    }

    fn rename(&mut self, path: QString, name: QString) -> bool {
        self.renaming_path = QString::from("");
        self.renaming_changed();
        let source = PathBuf::from(path.to_string());
        let name = name.to_string();
        let name = match ops::validate_file_name(&name) {
            Ok(name) => name.to_owned(),
            Err(error) => {
                self.set_error(error);
                return false;
            }
        };
        let Some(parent) = source.parent() else {
            self.set_error("That entry cannot be renamed.".to_owned());
            return false;
        };
        let target = parent.join(&name);
        if target == source {
            return true;
        }
        if target.exists() {
            self.set_error(format!(
                "{} already exists in {}.",
                name,
                ops::display_path(parent)
            ));
            return false;
        }
        match fs::rename(&source, &target) {
            Ok(()) => {
                let (dev, ino) = fs::metadata(&target)
                    .map(|meta| {
                        use std::os::unix::fs::MetadataExt;
                        (meta.dev(), meta.ino())
                    })
                    .unwrap_or((0, 0));
                self.tag_store.note_move(&source, &target, dev, ino);
                let _ = self.tag_store.save();
                self.update_clipboard_path(&source, &target);
                self.push_undo(UndoOp::Rename {
                    from: source,
                    to: target,
                });
                self.status = QString::from(format!("Renamed to {name}"));
                self.status_changed();
                self.refresh();
                true
            }
            Err(error) => {
                self.set_error(format!("Could not rename {}: {error}", source.display()));
                false
            }
        }
    }

    fn begin_rename(&mut self, path: QString) {
        self.renaming_path = path;
        self.renaming_changed();
    }

    fn cancel_rename(&mut self) {
        self.renaming_path = QString::from("");
        self.renaming_changed();
    }

    fn create_folder(&mut self, dest: QString, name: QString) -> QString {
        self.create_entry(PathBuf::from(dest.to_string()), name.to_string(), true)
    }

    fn create_file(&mut self, dest: QString, name: QString) -> QString {
        self.create_entry(PathBuf::from(dest.to_string()), name.to_string(), false)
    }

    fn create_entry(&mut self, dest: PathBuf, name: String, dir: bool) -> QString {
        if is_virtual_collection(&dest) {
            self.set_error("Choose a real folder to create items.");
            return QString::from("");
        }
        if !dest.is_dir() {
            self.set_error(format!("{} is not a folder.", dest.display()));
            return QString::from("");
        }
        let name = match ops::validate_file_name(&name) {
            Ok(name) => name.to_owned(),
            Err(error) => {
                self.set_error(error);
                return QString::from("");
            }
        };
        let path = dest.join(&name);
        if path.exists() {
            self.set_error(format!("{} already exists.", path.display()));
            return QString::from("");
        }
        let result = if dir {
            fs::create_dir(&path)
        } else {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map(|_| ())
        };
        match result {
            Ok(()) => {
                self.push_undo(UndoOp::Create {
                    path: path.clone(),
                    dir,
                });
                self.status = QString::from(format!(
                    "Created {} {name}",
                    if dir { "folder" } else { "file" }
                ));
                self.status_changed();
                self.refresh();
                QString::from(path.to_string_lossy().into_owned())
            }
            Err(error) => {
                self.set_error(format!("Could not create {}: {error}", path.display()));
                QString::from("")
            }
        }
    }

    fn suggest_name(&self, dest: QString, base: QString) -> QString {
        let dest = PathBuf::from(dest.to_string());
        let base = base.to_string();
        let candidate = ops::unique_destination(&dest, &base);
        QString::from(
            candidate
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or(base),
        )
    }

    fn request_batch_preview(
        &mut self,
        paths: QVariantList,
        configuration: QString,
        excluded: QVariantList,
    ) {
        if let Some(cancel) = self.batch_preview_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.batch_preview_generation = self.batch_preview_generation.wrapping_add(1);
        let generation = self.batch_preview_generation;
        self.batch_plan = None;
        let paths = list_paths(&paths);
        if paths.is_empty() {
            self.batch_preview_rows = QVariantList::default();
            self.batch_preview_id = Default::default();
            self.batch_preview_busy = false;
            self.batch_preview_error = QString::from("Select at least one file to rename.");
            self.batch_preview_changed();
            return;
        }
        let options = match batch_options_from_json(&configuration.to_string()) {
            Ok(options) => options,
            Err(error) => {
                self.batch_preview_rows = QVariantList::default();
                self.batch_preview_id = Default::default();
                self.batch_preview_busy = false;
                self.batch_preview_error = QString::from(error);
                self.batch_preview_changed();
                return;
            }
        };
        let excluded = list_paths(&excluded).into_iter().collect::<HashSet<_>>();
        let cancel = Arc::new(AtomicBool::new(false));
        self.batch_preview_cancel = Some(cancel.clone());
        self.batch_preview_rows = QVariantList::default();
        self.batch_preview_id = QString::from(generation.to_string());
        self.batch_preview_busy = true;
        self.batch_preview_error = Default::default();
        self.batch_preview_changed();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: BatchPreviewResult| {
            if let Some(backend) = qptr.as_pinned() {
                let mut backend = backend.borrow_mut();
                if result.generation != backend.batch_preview_generation {
                    return;
                }
                backend.batch_preview_cancel = None;
                backend.batch_preview_busy = false;
                match result.result {
                    Ok(rows) => {
                        backend.batch_preview_rows = batch_rows_to_variant(&rows);
                        backend.batch_plan = Some((result.generation, rows));
                        backend.batch_preview_error = Default::default();
                    }
                    Err(error) => {
                        backend.batch_plan = None;
                        backend.batch_preview_rows = QVariantList::default();
                        backend.batch_preview_error = QString::from(error);
                    }
                }
                backend.batch_preview_changed();
            }
        });
        std::thread::spawn(move || {
            callback(BatchPreviewResult {
                generation,
                result: batch::plan_cancellable(&paths, &options, &excluded, &cancel),
            });
        });
    }

    fn clear_batch_preview(&mut self) {
        if let Some(cancel) = self.batch_preview_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.batch_preview_generation = self.batch_preview_generation.wrapping_add(1);
        self.batch_plan = None;
        self.batch_preview_rows = QVariantList::default();
        self.batch_preview_id = Default::default();
        self.batch_preview_busy = false;
        self.batch_preview_error = Default::default();
        self.batch_preview_changed();
    }

    fn execute_batch_rename(&mut self, plan_id: QString) -> bool {
        if self.non_transfer_operation_busy() {
            self.set_error("Wait for the current file operation to finish first.");
            return false;
        }
        let generation = match plan_id.to_string().parse::<u64>() {
            Ok(generation) => generation,
            Err(_) => {
                self.set_error("The batch preview is no longer valid. Recalculate it first.");
                return false;
            }
        };
        let Some((current_generation, rows)) = self.batch_plan.clone() else {
            self.set_error("The batch preview is no longer available. Recalculate it first.");
            return false;
        };
        if generation != current_generation || generation != self.batch_preview_generation {
            self.set_error("The batch preview is stale. Recalculate it before confirming.");
            return false;
        }
        if batch::has_errors(&rows) {
            self.set_error("Resolve every invalid name and conflict before confirming.");
            return false;
        }
        let changed = rows
            .iter()
            .filter(|row| row.included && !row.unchanged)
            .count();
        if changed == 0 {
            self.set_error("There are no included files to rename.");
            return false;
        }

        let operation_generation = self.begin_operation(format!("Renaming {changed} items"), true);
        self.batch_plan = None;
        let cancel = self
            .cancel_flag
            .clone()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let qptr = QPointer::from(&*self);
        let tick_cb = qmetaobject::queued_callback(move |tick: OpTick| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_op_tick(tick);
            }
        });
        let qptr = QPointer::from(&*self);
        let done_cb = qmetaobject::queued_callback(move |done: BatchRenameDone| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_batch_rename_done(done);
            }
        });
        std::thread::spawn(move || {
            let mut last_update = Instant::now();
            let result = batch::apply_with_progress(&rows, Some(&cancel), |index, count, path| {
                let now = Instant::now();
                if now.duration_since(last_update) < Duration::from_millis(80) && index < count {
                    return;
                }
                last_update = now;
                tick_cb(OpTick {
                    generation: operation_generation,
                    detail: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string()),
                    progress: if count == 0 {
                        1.0
                    } else {
                        index as f64 / count as f64
                    },
                });
            });
            let (pairs, error) = match result {
                Ok(pairs) => (pairs, None),
                Err(error) => (Vec::new(), Some(error)),
            };
            done_cb(BatchRenameDone {
                generation: operation_generation,
                rows,
                pairs,
                error,
            });
        });
        true
    }

    fn apply_batch_rename_done(&mut self, mut done: BatchRenameDone) {
        if done.generation != self.op_generation {
            return;
        }
        self.finish_operation();
        let execution_error = done.error.clone();
        let failed = execution_error.is_some();
        if let Some(error) = done.error {
            self.set_error(format!("Batch rename failed: {error}"));
            self.status = QString::from("Batch rename did not complete.");
        } else {
            for (from, to) in &done.pairs {
                if let Ok(metadata) = fs::symlink_metadata(to) {
                    use std::os::unix::fs::MetadataExt;
                    self.tag_store
                        .note_move(from, to, metadata.dev(), metadata.ino());
                }
            }
            if let Err(error) = self.tag_store.save() {
                self.set_error(format!(
                    "Files were renamed, but their tags could not be saved: {error}"
                ));
            }
            self.push_undo(UndoOp::RenameMany {
                pairs: done.pairs.clone(),
            });
            self.status = QString::from(format!(
                "Renamed {} {}",
                done.pairs.len(),
                item_word(done.pairs.len())
            ));
        }
        let notification_body = execution_error.as_ref().map_or_else(
            || format!("Renamed {} {}", done.pairs.len(), item_word(done.pairs.len())),
            |error| format!("Batch rename failed: {error}"),
        );
        let notification_title = match execution_error.as_deref() {
            Some(error) if error.to_ascii_lowercase().contains("cancel") => {
                "File operation cancelled"
            }
            Some(_) => "File operation failed",
            None => "File operation complete",
        };
        notifications::send(notification_title, notification_body);
        let renamed = done
            .pairs
            .iter()
            .map(|(from, _)| from.clone())
            .collect::<HashSet<_>>();
        for row in &mut done.rows {
            if !row.included {
                row.result = "Excluded".to_owned();
            } else if row.unchanged {
                row.result = "Unchanged".to_owned();
            } else if renamed.contains(&row.origin) {
                row.result = "Renamed".to_owned();
            } else if failed {
                let destination = row
                    .origin
                    .parent()
                    .unwrap_or(Path::new(""))
                    .join(&row.new_name);
                let source_exists = fs::symlink_metadata(&row.origin).is_ok();
                let destination_exists = fs::symlink_metadata(&destination).is_ok();
                row.result = match (source_exists, destination_exists) {
                    (true, false) => "Not renamed",
                    (false, true) => "Renamed; recovery needed",
                    (true, true) => "Recovery conflict",
                    (false, false) => "Source unavailable",
                }
                .to_owned();
                row.error = execution_error.clone().unwrap_or_default();
            }
        }
        self.batch_preview_rows = batch_rows_to_variant(&done.rows);
        self.batch_preview_changed();
        self.status_changed();
        self.refresh();
    }

    fn trash_paths(&mut self, paths: QVariantList) {
        if self.op_active {
            self.set_error("Another operation is still running.");
            return;
        }
        let paths = list_paths(&paths);
        if paths.is_empty() {
            return;
        }
        if self.in_trash {
            self.set_error(
                "These items are already in the Trash. Use Shift+Delete for permanent deletion.",
            );
            return;
        }
        let total = paths.len();
        let qptr = QPointer::from(&*self);
        let done_cb = qmetaobject::queued_callback(move |done: TransferFinished| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_transfer_finished(done);
            }
        });

        let enqueue_result =
            transfer::enqueue(Operation::Trash, paths.clone(), None, move |context| {
                set_transfer_inventory(&context, ops::transfer_inventory(&paths));
                let mut items: Vec<TrashItem> = Vec::new();
                let mut failed: Vec<(PathBuf, String)> = Vec::new();
                for path in &paths {
                    if context.is_cancelled() {
                        break;
                    }
                    context.start_item(path);
                    match ops::trash(path) {
                        Ok(item) => {
                            ops::report_tree_entries(
                                &item.file,
                                path,
                                ops::ItemEvent::Completed,
                                &mut |item_path, is_directory, event| match event {
                                    ops::ItemEvent::Started => context.start_item(item_path),
                                    ops::ItemEvent::Completed if is_directory => {
                                        context.finish_item(
                                            item_path,
                                            true,
                                            ItemResult::Completed,
                                            "",
                                        );
                                        context.finish_subtree(
                                            item_path,
                                            ItemResult::Completed,
                                            "",
                                        );
                                    }
                                    ops::ItemEvent::Completed => context.finish_item(
                                        item_path,
                                        is_directory,
                                        ItemResult::Completed,
                                        "",
                                    ),
                                    ops::ItemEvent::Skipped => context.finish_item(
                                        item_path,
                                        is_directory,
                                        ItemResult::Skipped,
                                        "",
                                    ),
                                    ops::ItemEvent::Cancelled => context.finish_item(
                                        item_path,
                                        is_directory,
                                        ItemResult::Cancelled,
                                        "",
                                    ),
                                    ops::ItemEvent::Failed(error) => context.finish_item(
                                        item_path,
                                        is_directory,
                                        ItemResult::Failed,
                                        error,
                                    ),
                                },
                            );
                            context.finish_subtree(path, ItemResult::Completed, "");
                            items.push(item);
                        }
                        Err(error) => {
                            context.finish_subtree(path, ItemResult::Failed, error.clone());
                            failed.push((path.clone(), error));
                        }
                    }
                }
                let cancelled = context.is_cancelled();
                let mut message = if cancelled {
                    format!("Cancelled: {} of {total} processed.", items.len())
                } else {
                    format!("Moved {} {} to Trash", items.len(), item_word(items.len()))
                };
                if !failed.is_empty() {
                    message.push_str(&format!(" · {} failed", failed.len()));
                }
                let error = if failed.is_empty() {
                    None
                } else {
                    Some(format!(
                        "{} {} could not be trashed: {}",
                        failed.len(),
                        item_word(failed.len()),
                        summarize_failures(&failed)
                    ))
                };
                done_cb(TransferFinished {
                    undo: (!items.is_empty()).then_some(UndoOp::Trash { items }),
                    completed_sources: Vec::new(),
                    from_clipboard: false,
                    message: message.clone(),
                    error: error.clone(),
                });
                if cancelled {
                    JobResult::cancelled(message)
                } else if let Some(error) = error {
                    JobResult::failed(message, error)
                } else {
                    JobResult::completed(message)
                }
            });
        if let Err(error) = enqueue_result {
            self.set_error(error);
            return;
        }
        self.refresh_transfer_jobs();
    }

    fn delete_paths(&mut self, paths: QVariantList, confirmed: bool) {
        if !confirmed {
            self.set_error("Permanent deletion needs explicit confirmation.");
            return;
        }
        if self.op_active {
            self.set_error("Another operation is still running.");
            return;
        }
        let paths = list_paths(&paths);
        if paths.is_empty() {
            return;
        }
        let total = paths.len();
        let qptr = QPointer::from(&*self);
        let done_cb = qmetaobject::queued_callback(move |done: TransferFinished| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_transfer_finished(done);
            }
        });

        let enqueue_result =
            transfer::enqueue(Operation::Delete, paths.clone(), None, move |context| {
                set_transfer_inventory(&context, ops::transfer_inventory(&paths));
                let mut removed = 0usize;
                let mut failed: Vec<(PathBuf, String)> = Vec::new();
                for path in &paths {
                    if context.is_cancelled() {
                        break;
                    }
                    match ops::remove_tree_detailed(
                        path,
                        &context.cancellation_token(),
                        &mut |item_path, is_directory, event| match event {
                            ops::ItemEvent::Started => context.start_item(item_path),
                            ops::ItemEvent::Completed if is_directory => {
                                context.finish_item(item_path, true, ItemResult::Completed, "");
                                context.finish_subtree(item_path, ItemResult::Completed, "");
                            }
                            ops::ItemEvent::Completed => {
                                context.finish_item(item_path, false, ItemResult::Completed, "")
                            }
                            ops::ItemEvent::Skipped => {
                                context.finish_item(
                                    item_path,
                                    is_directory,
                                    ItemResult::Skipped,
                                    "",
                                );
                            }
                            ops::ItemEvent::Cancelled => {
                                context.finish_item(
                                    item_path,
                                    is_directory,
                                    ItemResult::Cancelled,
                                    "",
                                );
                            }
                            ops::ItemEvent::Failed(error) => context.finish_item(
                                item_path,
                                is_directory,
                                ItemResult::Failed,
                                error,
                            ),
                        },
                    ) {
                        Ok(true) => removed += 1,
                        Ok(false) => break,
                        Err(error) => failed.push((path.clone(), error)),
                    }
                }
                let cancelled = context.is_cancelled();
                let mut message = if cancelled {
                    format!("Cancelled: {removed} of {total} deleted permanently.")
                } else {
                    format!(
                        "Permanently deleted {removed} {}. This cannot be undone.",
                        item_word(removed)
                    )
                };
                if !failed.is_empty() {
                    message.push_str(&format!(" · {} failed", failed.len()));
                }
                let error = if failed.is_empty() {
                    None
                } else {
                    Some(format!(
                        "{} {} could not be deleted: {}",
                        failed.len(),
                        item_word(failed.len()),
                        summarize_failures(&failed)
                    ))
                };
                done_cb(TransferFinished {
                    undo: None,
                    completed_sources: Vec::new(),
                    from_clipboard: false,
                    message: message.clone(),
                    error: error.clone(),
                });
                if cancelled {
                    JobResult::cancelled(message)
                } else if let Some(error) = error {
                    JobResult::failed(message, error)
                } else {
                    JobResult::completed(message)
                }
            });
        if let Err(error) = enqueue_result {
            self.set_error(error);
            return;
        }
        self.refresh_transfer_jobs();
    }

    fn restore_paths(&mut self, paths: QVariantList) {
        if self.op_active {
            self.set_error("Another operation is still running.");
            return;
        }
        if !self.in_trash {
            self.set_error("Only items in the Trash can be restored.");
            return;
        }
        let paths = list_paths(&paths);
        if paths.is_empty() {
            return;
        }
        let items: Vec<TrashItem> = ops::trashed_items_in(&ops::home_trash())
            .into_iter()
            .filter(|item| paths.contains(&item.file))
            .collect();
        if items.is_empty() {
            self.set_error("Those items are no longer in the Trash.");
            return;
        }
        let qptr = QPointer::from(&*self);
        let done_cb = qmetaobject::queued_callback(move |done: TransferFinished| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_transfer_finished(done);
            }
        });
        let paths = items
            .iter()
            .map(|item| item.file.clone())
            .collect::<Vec<_>>();
        let enqueue_result =
            transfer::enqueue(Operation::Restore, paths.clone(), None, move |context| {
                set_transfer_inventory(&context, ops::transfer_inventory(&paths));
                let mut restored = 0usize;
                let mut failed: Vec<(PathBuf, String)> = Vec::new();
                for item in &items {
                    if context.is_cancelled() {
                        break;
                    }
                    context.start_item(&item.file);
                    match ops::restore_trashed(item) {
                        Ok(restored_path) => {
                            ops::report_tree_entries(
                                &restored_path,
                                &item.file,
                                ops::ItemEvent::Completed,
                                &mut |item_path, is_directory, event| match event {
                                    ops::ItemEvent::Started => context.start_item(item_path),
                                    ops::ItemEvent::Completed if is_directory => {
                                        context.finish_item(
                                            item_path,
                                            true,
                                            ItemResult::Completed,
                                            "",
                                        );
                                        context.finish_subtree(
                                            item_path,
                                            ItemResult::Completed,
                                            "",
                                        );
                                    }
                                    ops::ItemEvent::Completed => context.finish_item(
                                        item_path,
                                        is_directory,
                                        ItemResult::Completed,
                                        "",
                                    ),
                                    ops::ItemEvent::Skipped => context.finish_item(
                                        item_path,
                                        is_directory,
                                        ItemResult::Skipped,
                                        "",
                                    ),
                                    ops::ItemEvent::Cancelled => context.finish_item(
                                        item_path,
                                        is_directory,
                                        ItemResult::Cancelled,
                                        "",
                                    ),
                                    ops::ItemEvent::Failed(error) => context.finish_item(
                                        item_path,
                                        is_directory,
                                        ItemResult::Failed,
                                        error,
                                    ),
                                },
                            );
                            context.finish_subtree(&item.file, ItemResult::Completed, "");
                            restored += 1;
                        }
                        Err(error) => {
                            context.finish_subtree(&item.file, ItemResult::Failed, error.clone());
                            failed.push((item.file.clone(), error));
                        }
                    }
                }
                let cancelled = context.is_cancelled();
                let mut message = format!("Restored {restored} {}", item_word(restored));
                if cancelled {
                    message.insert_str(0, "Cancelled: ");
                }
                if !failed.is_empty() {
                    message.push_str(&format!(" · {} failed", failed.len()));
                }
                let error = if failed.is_empty() {
                    None
                } else {
                    Some(format!(
                        "{} {} could not be restored: {}",
                        failed.len(),
                        item_word(failed.len()),
                        summarize_failures(&failed)
                    ))
                };
                done_cb(TransferFinished {
                    undo: None,
                    completed_sources: Vec::new(),
                    from_clipboard: false,
                    message: message.clone(),
                    error: error.clone(),
                });
                if cancelled {
                    JobResult::cancelled(message)
                } else if let Some(error) = error {
                    JobResult::failed(message, error)
                } else {
                    JobResult::completed(message)
                }
            });
        if let Err(error) = enqueue_result {
            self.set_error(error);
            return;
        }
        self.refresh_transfer_jobs();
    }

    fn empty_trash(&mut self) {
        if self.op_active {
            self.set_error("Another operation is still running.");
            return;
        }
        let root = ops::home_trash();
        let qptr = QPointer::from(&*self);
        let done_cb = qmetaobject::queued_callback(move |done: TransferFinished| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_transfer_finished(done);
            }
        });
        let enqueue_result = transfer::enqueue(
            Operation::EmptyTrash,
            vec![root.clone()],
            None,
            move |context| {
                let items = ops::trashed_items_in(&root);
                let paths = items
                    .iter()
                    .map(|item| item.file.clone())
                    .collect::<Vec<_>>();
                set_transfer_inventory(&context, ops::transfer_inventory(&paths));
                let mut removed = 0usize;
                let mut failed = Vec::new();
                for item in &items {
                    if context.is_cancelled() {
                        break;
                    }
                    match ops::remove_tree_detailed(
                        &item.file,
                        &context.cancellation_token(),
                        &mut |path, is_directory, event| match event {
                            ops::ItemEvent::Started => context.start_item(path),
                            ops::ItemEvent::Completed if is_directory => {
                                context.finish_item(path, true, ItemResult::Completed, "");
                                context.finish_subtree(path, ItemResult::Completed, "");
                            }
                            ops::ItemEvent::Completed => {
                                context.finish_item(path, false, ItemResult::Completed, "")
                            }
                            ops::ItemEvent::Skipped => {
                                context.finish_item(path, is_directory, ItemResult::Skipped, "")
                            }
                            ops::ItemEvent::Cancelled => {
                                context.finish_item(path, is_directory, ItemResult::Cancelled, "")
                            }
                            ops::ItemEvent::Failed(error) => {
                                context.finish_item(path, is_directory, ItemResult::Failed, error)
                            }
                        },
                    ) {
                        Ok(true) => {
                            removed += 1;
                            let _ = fs::remove_file(&item.info);
                        }
                        Ok(false) => break,
                        Err(error) => failed.push((item.file.clone(), error)),
                    }
                }
                let cancelled = context.is_cancelled();
                let mut message = format!(
                    "Removed {removed} {} from the Trash. This cannot be undone.",
                    item_word(removed)
                );
                if !failed.is_empty() {
                    message.push_str(&format!(" · {} failed", failed.len()));
                }
                let error = if failed.is_empty() {
                    None
                } else {
                    Some(format!(
                        "{} {} could not be removed: {}",
                        failed.len(),
                        item_word(failed.len()),
                        summarize_failures(&failed)
                    ))
                };
                done_cb(TransferFinished {
                    undo: None,
                    completed_sources: Vec::new(),
                    from_clipboard: false,
                    message: message.clone(),
                    error: error.clone(),
                });
                if cancelled {
                    JobResult::cancelled(message)
                } else if let Some(error) = error {
                    JobResult::failed(message, error)
                } else {
                    JobResult::completed(message)
                }
            },
        );
        if let Err(error) = enqueue_result {
            self.set_error(error);
            return;
        }
        self.refresh_transfer_jobs();
    }

    fn undo(&mut self) {
        if self.non_transfer_operation_busy() {
            self.set_error("Wait for the current operation to finish before undoing.");
            return;
        }
        let Some(op) = self.undo_stack.pop() else {
            self.status = QString::from("Nothing to undo.");
            self.status_changed();
            return;
        };
        let description = op.description();
        self.sync_undo_props();
        let generation = self.begin_operation(format!("Undoing {description}"), false);
        let qptr = QPointer::from(&*self);
        let done_cb = qmetaobject::queued_callback(move |done: UndoDone| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_undo_done(done);
            }
        });
        std::thread::spawn(move || {
            let (redo, error) = match op.clone().undo() {
                Ok(redo) => (redo, None),
                Err(error) => (None, Some(error)),
            };
            done_cb(UndoDone {
                generation,
                description,
                original: op,
                redo,
                error,
            });
        });
    }

    fn apply_undo_done(&mut self, done: UndoDone) {
        if done.generation != self.op_generation {
            return;
        }
        self.finish_operation();
        match done.error {
            Some(error) => {
                notifications::send(
                    "File operation failed",
                    format!("Could not undo {}: {error}", done.description),
                );
                // Keep the operation so the user can look and try again.
                self.undo_stack.push(done.original);
                self.sync_undo_props();
                self.set_error(format!("Could not undo: {error}"));
            }
            None => {
                notifications::send(
                    "File operation complete",
                    format!("Undid {}", done.description),
                );
                if let Some(redo) = done.redo {
                    self.redo_stack.push(redo);
                }
                self.sync_undo_props();
                self.status = QString::from(format!("Undid {}", done.description));
                self.status_changed();
            }
        }
        self.refresh();
    }

    fn redo(&mut self) {
        if self.non_transfer_operation_busy() {
            self.set_error("Wait for the current operation to finish before redoing.");
            return;
        }
        let Some(op) = self.redo_stack.pop() else {
            self.status = QString::from("Nothing to redo.");
            self.status_changed();
            return;
        };
        let description = op.description();
        self.sync_undo_props();
        let generation = self.begin_operation(format!("Redoing {description}"), false);
        let qptr = QPointer::from(&*self);
        let done_cb = qmetaobject::queued_callback(move |done: RedoDone| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_redo_done(done);
            }
        });
        std::thread::spawn(move || {
            let (undo, error) = match op.clone().apply() {
                Ok(undo) => (Some(undo), None),
                Err(error) => (None, Some(error)),
            };
            done_cb(RedoDone {
                generation,
                description,
                original: op,
                undo,
                error,
            });
        });
    }

    fn apply_redo_done(&mut self, done: RedoDone) {
        if done.generation != self.op_generation {
            return;
        }
        self.finish_operation();
        match done.error {
            Some(error) => {
                notifications::send(
                    "File operation failed",
                    format!("Could not redo {}: {error}", done.description),
                );
                self.redo_stack.push(done.original);
                self.sync_undo_props();
                self.set_error(format!("Could not redo: {error}"));
            }
            None => {
                notifications::send(
                    "File operation complete",
                    format!("Redid {}", done.description),
                );
                if let Some(undo) = done.undo {
                    self.undo_stack.push(undo);
                    if self.undo_stack.len() > MAX_UNDO_STEPS {
                        self.undo_stack.remove(0);
                    }
                }
                self.sync_undo_props();
                self.status = QString::from(format!("Redid {}", done.description));
                self.status_changed();
            }
        }
        self.refresh();
    }

    fn cancel_operation(&mut self) {
        if let Some(cancel) = &self.cancel_flag {
            cancel.store(true, Ordering::Relaxed);
            self.op_detail = QString::from("Cancelling…");
            self.op_changed();
        } else if let Some(id) = transfer::active_job_id() {
            transfer::cancel(id);
            self.refresh_transfer_jobs();
        }
    }

    // ---- Bookmarks ------------------------------------------------------

    fn push_bookmarks_property(&mut self) {
        let mut list = QVariantList::default();
        for (index, bookmark) in self.bookmark_store.items.iter().enumerate() {
            let mut map = QVariantMap::default();
            map.insert(QString::from("index"), QVariant::from(index as i32));
            map.insert(
                QString::from("label"),
                QVariant::from(QString::from(bookmark.label.clone())),
            );
            map.insert(
                QString::from("path"),
                QVariant::from(QString::from(bookmark.path.clone())),
            );
            list.push(QVariant::from(map));
        }
        self.bookmarks = list;
        self.bookmarks_changed();
    }

    fn add_bookmark(&mut self, path: QString, label: QString) -> bool {
        let path = path.to_string();
        let label = label.to_string();
        let label = (!label.trim().is_empty()).then_some(label.as_str());
        match self.bookmark_store.add(&path, label) {
            Ok(_) => {
                if let Err(error) = self.bookmark_store.save() {
                    self.set_error(error);
                    return false;
                }
                self.push_bookmarks_property();
                self.status = QString::from(format!(
                    "Bookmarked {}",
                    ops::display_path(Path::new(&path))
                ));
                self.status_changed();
                true
            }
            Err(error) => {
                self.set_error(error);
                false
            }
        }
    }

    fn toggle_bookmark(&mut self, path: QString) -> bool {
        let path = paths::clean_lexically(Path::new(&path.to_string()))
            .to_string_lossy()
            .into_owned();
        if let Some(index) = self
            .bookmark_store
            .items
            .iter()
            .position(|bookmark| bookmark.path == path)
        {
            if self.bookmark_store.remove(index) {
                if let Err(error) = self.bookmark_store.save() {
                    self.set_error(error);
                    return false;
                }
                self.push_bookmarks_property();
                self.status = QString::from(format!(
                    "Removed bookmark for {}",
                    ops::display_path(Path::new(&path))
                ));
                self.status_changed();
                return true;
            }
            return false;
        }
        self.add_bookmark(QString::from(path), QString::default())
    }

    fn remove_bookmark(&mut self, index: i32) {
        if self.bookmark_store.remove(index.max(0) as usize) {
            if let Err(error) = self.bookmark_store.save() {
                self.set_error(error);
            }
            self.push_bookmarks_property();
        }
    }

    fn rename_bookmark(&mut self, index: i32, label: QString) {
        match self
            .bookmark_store
            .rename(index.max(0) as usize, &label.to_string())
        {
            Ok(()) => {
                if let Err(error) = self.bookmark_store.save() {
                    self.set_error(error);
                }
                self.push_bookmarks_property();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn move_bookmark(&mut self, index: i32, delta: i32) {
        if self
            .bookmark_store
            .move_item(index.max(0) as usize, delta as isize)
        {
            if let Err(error) = self.bookmark_store.save() {
                self.set_error(error);
            }
            self.push_bookmarks_property();
        }
    }

    fn clear_recent_history(&mut self) {
        self.recent_generation = self.recent_generation.wrapping_add(1);
        self.recent_store = RecentStore::default();
        self.persist_recent_store();
        if is_recent_path(&self.current_dir()) {
            if self.search_mode {
                self.clear_search();
            }
            self.refresh();
        }
        self.status = QString::from("Recent history cleared");
        self.status_changed();
    }

    // ---- Search ---------------------------------------------------------

    fn set_search_recursive(&mut self, recursive: bool) {
        self.update_search_recursive(recursive, None);
    }

    fn set_search_recursive_advanced(&mut self, query: QString, filters: QString, recursive: bool) {
        let query = query.to_string();
        let filters = filters.to_string();
        self.search_recursive = recursive;
        self.config.ui.search_recursive = recursive;
        self.search_changed();
        self.persist_config();
        if !query.trim().is_empty() || !filters.trim().is_empty() {
            self.search_advanced(QString::from(query), QString::from(filters), recursive);
        }
    }

    fn update_search_recursive(&mut self, recursive: bool, filters: Option<String>) {
        self.search_recursive = recursive;
        self.config.ui.search_recursive = recursive;
        self.search_changed();
        self.persist_config();
        if self.search_mode {
            let (query, current_filters) = self
                .tabs_data
                .get(self.active)
                .and_then(|tab| tab.search.as_ref())
                .map(|state| (state.query.clone(), state.filters.clone()))
                .unwrap_or_default();
            self.search_advanced(
                QString::from(query),
                QString::from(filters.unwrap_or(current_filters)),
                recursive,
            );
        }
    }

    fn search(&mut self, query: QString, recursive: bool) {
        self.search_advanced(query, QString::default(), recursive);
    }

    fn search_advanced(&mut self, query: QString, filters: QString, recursive: bool) {
        let query = query.to_string();
        let filters = filters.to_string();
        if query.trim().is_empty() && filters.trim().is_empty() {
            self.clear_search();
            return;
        }
        let Some(tab) = self.tabs_data.get_mut(self.active) else {
            return;
        };
        if tab.search.is_none() {
            tab.folder_entries = std::mem::take(&mut tab.entries);
        }
        tab.cancel_search();
        tab.search = Some(SearchState {
            query: query.clone(),
            filters,
            recursive,
            truncated: false,
            visited: 0,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        self.search_recursive = recursive;
        self.search_changed();
        self.run_search(self.active);
    }

    fn run_search(&mut self, index: usize) {
        let Some(tab) = self.tabs_data.get_mut(index) else {
            return;
        };
        let Some(state) = tab.search.as_mut() else {
            return;
        };
        tab.search_generation = tab.search_generation.wrapping_add(1);
        state.generation = tab.search_generation;
        let generation = state.generation;
        let query = state.query.clone();
        let filters = state.filters.clone();
        let recursive = state.recursive;
        let cancel = state.restart();
        let root = tab.path.clone();
        let tab_id = tab.id;
        let collection_entries = is_virtual_collection(&root).then(|| tab.folder_entries.clone());
        let show_hidden = self.show_hidden;
        let tags = self.tag_store.clone();
        let icons = self.icons.clone();

        tab.search_running = true;
        if index == self.active {
            self.sync_active(false);
        }

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: SearchResult| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_search_result(result);
            }
        });
        std::thread::spawn(move || {
            let mut publish = |update| {
                callback(SearchResult {
                    tab_id,
                    generation,
                    update,
                });
            };
            if let Some(entries) = collection_entries {
                search::search_entries_with_updates(
                    &query,
                    &filters,
                    &entries,
                    &tags,
                    &cancel,
                    MAX_SEARCH_RESULTS,
                    &mut publish,
                );
            } else {
                search::search_with_updates(
                    &root,
                    &query,
                    &filters,
                    recursive,
                    show_hidden,
                    &tags,
                    &icons,
                    &cancel,
                    MAX_SEARCH_RESULTS,
                    &mut publish,
                );
            };
        });
    }

    fn apply_search_result(&mut self, result: SearchResult) {
        let Some(index) = self.tab_index(result.tab_id) else {
            return;
        };
        let current = self.tabs_data.get(index).is_some_and(|tab| {
            tab.search.as_ref().is_some_and(|state| {
                state.generation == result.generation && tab.search_generation == result.generation
            })
        });
        if !current {
            return;
        }

        let active = index == self.active;
        let append = result.update.append;
        let insert_count = if append {
            result
                .update
                .hits
                .len()
                .min(MAX_SEARCH_RESULTS.saturating_sub(self.tabs_data[index].entries.len()))
        } else {
            0
        };
        if active && append && insert_count > 0 {
            let first = self.tabs_data[index].entries.len();
            (self as &mut dyn QAbstractListModel)
                .begin_insert_rows(first as i32, (first + insert_count - 1) as i32);
        } else if active && !append {
            (self as &mut dyn QAbstractListModel).begin_reset_model();
        }
        {
            let tab = &mut self.tabs_data[index];
            if append {
                let remaining = MAX_SEARCH_RESULTS.saturating_sub(tab.entries.len());
                tab.entries
                    .extend(result.update.hits.into_iter().take(remaining));
            } else {
                tab.entries = result.update.hits;
            }
            if let Some(state) = tab.search.as_mut() {
                state.truncated = result.update.truncated;
                state.visited = result.update.visited;
            }
            if result.update.complete {
                tab.search_running = false;
            }
        }
        if active && append && insert_count > 0 {
            (self as &mut dyn QAbstractListModel).end_insert_rows();
        } else if active && !append {
            (self as &mut dyn QAbstractListModel).end_reset_model();
        }
        if active {
            self.sync_active(false);
        }
    }

    fn clear_search(&mut self) {
        let Some(tab) = self.tabs_data.get_mut(self.active) else {
            return;
        };
        tab.finish_search();
        tab.tree_rows = flatten_tree(&tab.entries, &tab.tree_children, &tab.tree_expanded);
        (self as &mut dyn QAbstractListModel).begin_reset_model();
        (self as &mut dyn QAbstractListModel).end_reset_model();
        self.sync_active(false);
        self.load_expanded_tree_folders(self.active);
    }

    fn copy_paths_text(&mut self, paths: QVariantList) -> bool {
        let paths = list_paths(&paths);
        if paths.is_empty() {
            return false;
        }

        match sysclip::write_text(&sysclip::paths_text(&paths)) {
            Ok(()) => {
                self.status = QString::from(format!(
                    "Copied {} path{}",
                    paths.len(),
                    if paths.len() == 1 { "" } else { "s" }
                ));
                self.status_changed();
                true
            }
            Err(error) => {
                self.set_error(error);
                false
            }
        }
    }

    fn paths_uri_list(&self, paths: QVariantList) -> QString {
        QString::from(sysclip::uri_list(&list_paths(&paths)))
    }

    // ---- Live directory watching ---------------------------------------

    fn stop_watch(&mut self) {
        if let Some(cancel) = self.watch_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.watch_path = None;
        self.watch_tab = None;
        self.watch_paths.clear();
    }

    fn ensure_watch(&mut self) {
        let Some(tab) = self.tabs_data.get(self.active) else {
            return;
        };
        if tab.search.is_some() {
            self.stop_watch();
            return;
        }
        let path = tab.path.clone();
        if is_recent_path(&path) {
            self.stop_watch();
            return;
        }
        let tab_id = tab.id;
        let mut paths = if is_tag_collection_path(&path) {
            self.tag_store
                .known_dirs
                .iter()
                .map(PathBuf::from)
                .collect::<HashSet<_>>()
        } else {
            let mut paths = tab.tree_expanded.clone();
            paths.extend(tab.tree_children.keys().cloned());
            paths.insert(path.clone());
            paths
        };
        paths.retain(|directory| directory.is_dir());
        if self.watch_path.as_deref() == Some(path.as_path())
            && self.watch_tab == Some(tab_id)
            && self.watch_paths == paths
            && self.watch_cancel.is_some()
        {
            return;
        }
        self.stop_watch();

        let cancel = Arc::new(AtomicBool::new(false));
        self.watch_cancel = Some(cancel.clone());
        self.watch_path = Some(path.clone());
        self.watch_tab = Some(tab_id);
        self.watch_paths = paths.clone();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |(id, path): (u64, PathBuf)| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().watch_fired(id, path);
            }
        });

        std::thread::spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
            let watched_paths = paths;
            let event_paths = watched_paths.clone();
            let mut watcher: RecommendedWatcher = match notify::recommended_watcher(
                move |event: Result<notify::Event, notify::Error>| {
                    if let Ok(event) = event {
                        for changed in event.paths {
                            if let Some(directory) = event_paths
                                .iter()
                                .filter(|directory| {
                                    changed.starts_with(directory) && directory.is_dir()
                                })
                                .max_by_key(|directory| directory.components().count())
                            {
                                let _ = tx.send(directory.clone());
                            }
                        }
                    }
                },
            ) {
                Ok(watcher) => watcher,
                Err(_) => return,
            };
            for path in &watched_paths {
                if watcher.watch(path, RecursiveMode::NonRecursive).is_err() {
                    continue;
                }
            }
            loop {
                let first = match rx.recv_timeout(Duration::from_millis(300)) {
                    Ok(directory) => directory,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        continue;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                };
                // Coalesce bursts: file managers and editors touch a directory
                // several times for one logical change.
                let deadline = Instant::now() + Duration::from_millis(350);
                let mut changed_directories = HashSet::from([first]);
                while Instant::now() < deadline {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    match rx.recv_timeout(Duration::from_millis(60)) {
                        Ok(directory) => {
                            changed_directories.insert(directory);
                        }
                        Err(_) => break,
                    }
                }
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                for directory in changed_directories {
                    callback((tab_id, directory));
                }
            }
        });
    }

    fn refresh_seen_directory(&mut self, tab_id: u64, path: PathBuf) {
        if !self.seen_refreshes.insert(path.clone()) {
            return;
        }
        let Some(index) = self.tab_index(tab_id) else {
            self.seen_refreshes.remove(&path);
            return;
        };
        let spec = self.tabs_data[index].sort;
        let show_hidden = self.show_hidden;
        let icons = self.icons.clone();
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |result: SeenDirectoryResult| {
            if let Some(backend) = qptr.as_pinned() {
                backend.borrow_mut().apply_seen_directory_result(result);
            }
        });
        std::thread::spawn(move || {
            let entries = read_directory(&path, spec, show_hidden, &icons);
            callback(SeenDirectoryResult {
                tab_id,
                path,
                entries,
            });
        });
    }

    fn apply_seen_directory_result(&mut self, result: SeenDirectoryResult) {
        self.seen_refreshes.remove(&result.path);
        let Some(index) = self.tab_index(result.tab_id) else {
            return;
        };
        match result.entries {
            Ok(entries) => {
                for entry in entries {
                    self.tag_store
                        .record_seen(&entry.path, entry.dev, entry.ino);
                }
                self.tag_store.record_directory(&result.path);
                self.push_tags_property();
                if self.active == index
                    && is_tag_collection_path(&self.tabs_data[index].path)
                    && !self.tabs_data[index].loading
                {
                    let current = self.tabs_data[index].path.clone();
                    self.load_tab(index, current, HistoryIntent::Current);
                }
            }
            Err(error) => {
                if self.active == index {
                    self.set_error(error);
                }
            }
        }
    }

    fn prune_missing_tag_paths(&mut self, paths: &[PathBuf]) -> bool {
        if paths.is_empty() {
            return false;
        }
        let previous = self.tag_store.clone();
        let changed = paths.iter().fold(false, |changed, path| {
            self.tag_store.forget_missing(path) || changed
        });
        if !changed {
            return false;
        }
        if let Err(error) = self.tag_store.save() {
            self.tag_store = previous;
            self.set_error(error);
            return false;
        }
        true
    }

    fn watch_fired(&mut self, tab_id: u64, changed_directory: PathBuf) {
        if self.op_active || transfer::is_busy() || self.loading {
            // Transfers refresh only their initiating window, so keep watcher
            // events from other windows until the process-wide queue is idle.
            self.pending_watch_events
                .insert((tab_id, changed_directory));
            return;
        }
        let Some(index) = self.tab_index(tab_id) else {
            return;
        };
        if index != self.active || self.tabs_data[index].search.is_some() {
            return;
        }
        let path = self.tabs_data[index].path.clone();
        if is_tag_collection_path(&path) {
            self.refresh_seen_directory(tab_id, changed_directory);
            return;
        }
        if changed_directory == path {
            if !path.is_dir() {
                return;
            }
            self.load_tab(index, path, HistoryIntent::Current);
            return;
        }
        if !changed_directory.is_dir() {
            return;
        }
        let expanded = self.tabs_data[index]
            .tree_expanded
            .contains(&changed_directory);
        self.tabs_data[index]
            .tree_children
            .remove(&changed_directory);
        self.ensure_watch();
        if expanded {
            self.start_tree_load(index, changed_directory);
        }
    }

    fn flush_pending_watch_events(&mut self) {
        if self.op_active || transfer::is_busy() || self.loading {
            return;
        }
        let pending = std::mem::take(&mut self.pending_watch_events);
        for (tab_id, changed_directory) in pending {
            self.watch_fired(tab_id, changed_directory);
        }
    }

    // ---- Property helpers ----------------------------------------------

    fn push_tags_property(&mut self) {
        let (counts, untagged_count) = self.tag_store.collection_counts();
        let mut list = QVariantList::default();
        for index in 0..self.tag_store.tags.len() {
            let mut map = QVariantMap::default();
            map.insert(QString::from("index"), QVariant::from(index as i32));
            map.insert(
                QString::from("name"),
                QVariant::from(QString::from(self.tag_store.name(index))),
            );
            map.insert(
                QString::from("color"),
                QVariant::from(QString::from(self.tag_store.color(index).unwrap_or(""))),
            );
            map.insert(
                QString::from("count"),
                QVariant::from(counts[index].min(i32::MAX as usize) as i32),
            );
            list.push(QVariant::from(map));
        }
        self.tags = list;
        self.untagged_count = untagged_count.min(i32::MAX as usize) as i32;
        self.tags_changed();
    }

    fn set_tag(&mut self, paths: QVariantList, index: i32) {
        self.update_tag_assignments(paths, index, false);
    }

    fn toggle_tag(&mut self, paths: QVariantList, index: i32) {
        self.update_tag_assignments(paths, index, true);
    }

    fn tag_dropped_paths(&mut self, uri_list: QString, index: i32) {
        let mut paths = QVariantList::default();
        for uri in uri_list.to_string().lines() {
            let uri = uri.trim();
            if uri.is_empty() || uri.starts_with('#') {
                continue;
            }
            let Some(path) = paths::expand_path(uri, &home_dir()) else {
                self.set_error("A dropped item has an invalid file URL.");
                return;
            };
            paths.push(QVariant::from(QString::from(
                path.to_string_lossy().into_owned(),
            )));
        }
        self.set_tag(paths, index);
    }

    fn update_tag_assignments(&mut self, paths: QVariantList, index: i32, toggle: bool) {
        if !(-1..crate::tags::TAG_COUNT as i32).contains(&index) {
            self.set_error("Choose a valid tag.");
            return;
        }
        let paths = list_paths(&paths);
        if paths.is_empty() {
            self.set_error("Select at least one item to tag.");
            return;
        }
        use std::os::unix::fs::MetadataExt;
        let mut identities = Vec::with_capacity(paths.len());
        for path in &paths {
            let metadata = match fs::metadata(path).or_else(|_| fs::symlink_metadata(path)) {
                Ok(metadata) => metadata,
                Err(error) => {
                    self.set_error(format!("Could not tag {}: {error}", path.display()));
                    return;
                }
            };
            identities.push((path.clone(), metadata.dev(), metadata.ino()));
        }

        let previous = self.tag_store.clone();
        if toggle && index >= 0 {
            let remove = identities.iter().all(|(path, device, inode)| {
                self.tag_store
                    .tags_for(path, *device, *inode)
                    .contains(&index)
            });
            for (path, device, inode) in &identities {
                if remove {
                    self.tag_store
                        .toggle_tag(path, *device, *inode, index as usize);
                } else {
                    self.tag_store
                        .set_tag(path, *device, *inode, Some(index as usize));
                }
            }
        } else {
            for (path, device, inode) in &identities {
                self.tag_store.set_tag(
                    path,
                    *device,
                    *inode,
                    (index >= 0).then_some(index as usize),
                );
            }
        }
        if let Err(error) = self.tag_store.save() {
            self.tag_store = previous;
            self.set_error(error);
            return;
        }
        self.push_tags_property();

        let mut changed = false;
        for tab in &mut self.tabs_data {
            for entry in tab
                .entries
                .iter_mut()
                .chain(tab.folder_entries.iter_mut())
                .chain(tab.tree_children.values_mut().flatten())
                .chain(tab.tree_rows.iter_mut())
            {
                if identities.iter().any(|(path, _, _)| *path == entry.path) {
                    entry.tag = self.tag_store.tag_for(&entry.path, entry.dev, entry.ino);
                    changed = true;
                }
            }
        }
        let current_path = self.current_dir();
        if is_tag_collection_path(&current_path) {
            self.load_tab(self.active, current_path, HistoryIntent::Current);
        } else if changed {
            (self as &mut dyn QAbstractListModel).begin_reset_model();
            (self as &mut dyn QAbstractListModel).end_reset_model();
        }
        if self
            .tabs_data
            .get(self.active)
            .is_some_and(|tab| tab.search.is_some())
        {
            self.run_search(self.active);
        }
        self.status = QString::from(if index < 0 {
            format!("Removed tags from {} items", paths.len())
        } else if toggle {
            format!("Updated {} items", paths.len())
        } else {
            format!(
                "Tagged {} items as {}",
                paths.len(),
                self.tag_store.name(index as usize)
            )
        });
        self.status_changed();
    }

    fn set_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.error = QString::from(message.clone());
        self.error_changed();
        if let Some(tab) = self.tabs_data.get_mut(self.active) {
            tab.error = message;
        }
        self.loading = false;
        self.loading_changed();
        if self.op_active {
            self.finish_operation();
        }
    }

    fn set_transfer_error(&mut self, message: String) {
        self.error = QString::from(message.clone());
        self.error_changed();
        if let Some(tab) = self.tabs_data.get_mut(self.active) {
            tab.error = message;
        }
    }

    fn persist_config(&mut self) {
        let (mut latest, warning) = Config::load();
        if let Some(warning) = warning {
            self.set_error(warning);
            return;
        }
        latest.merge_changes_from(&self.persisted_config, &self.config);
        latest.performance = crate::lifecycle::performance_config();
        match latest.save() {
            Ok(()) => {
                self.config = latest.clone();
                self.persisted_config = latest;
            }
            Err(error) => self.set_error(error),
        }
    }
}

fn search_summary(state: &SearchState, entries: &[Entry]) -> String {
    let scope = if state.recursive {
        "this folder and below"
    } else {
        "this folder"
    };
    if entries.is_empty() {
        if state.truncated {
            format!("No results for “{}” in {scope}", state.query)
        } else {
            format!("No matches for “{}” in {scope}", state.query)
        }
    } else {
        format!(
            "{} {} for “{}” in {scope}",
            entries.len(),
            item_word(entries.len()),
            state.query
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    fn test_backend_with_tabs() -> Backend {
        let mut backend = Backend::default();
        backend.view_mode = QString::from("grid");
        backend.tabs_data = vec![
            Tab::new(1, PathBuf::from("/tmp/search-one"), SortSpec::default()),
            Tab::new(2, PathBuf::from("/tmp/search-two"), SortSpec::default()),
        ];
        backend.rebuild_tabs_property();
        backend.sync_active(false);
        backend
    }

    #[test]
    fn terminal_launch_uses_the_login_shell() {
        let command = terminal_command("foot", Path::new("/tmp"));
        let expected = login_shell();
        let configured = command
            .get_envs()
            .find(|(key, _)| *key == std::ffi::OsStr::new("SHELL"))
            .and_then(|(_, value)| value);
        assert_eq!(configured, expected.as_deref().map(std::ffi::OsStr::new));
    }

    #[test]
    fn tab_loading_indicator_ignores_same_path_refreshes() {
        let path = PathBuf::from("/tmp/empty-folder");
        let mut tab = Tab::new(1, path.clone(), SortSpec::default());

        tab.loading = true;
        assert!(tab.shows_loading_indicator());

        tab.loaded_path = Some(path.clone());
        assert!(!tab.shows_loading_indicator());

        tab.path = PathBuf::from("/tmp/another-folder");
        assert!(tab.shows_loading_indicator());

        tab.loading = false;
        assert!(!tab.shows_loading_indicator());
    }

    #[test]
    fn watch_events_are_coalesced_while_an_operation_is_busy() {
        let mut backend = test_backend_with_tabs();
        backend.op_active = true;
        let changed_directory = PathBuf::from("/tmp/search-one");

        backend.watch_fired(1, changed_directory.clone());
        backend.watch_fired(1, changed_directory.clone());
        assert_eq!(backend.pending_watch_events.len(), 1);

        backend.flush_pending_watch_events();
        assert_eq!(backend.pending_watch_events.len(), 1);
    }

    #[test]
    fn separate_backends_sync_the_process_clipboard() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        fs::write(&source, "clipboard").unwrap();
        publish_shared_clipboard(Some(Clipboard {
            paths: vec![source.clone()],
            cut: true,
        }));

        let mut destination_backend = Backend::default();
        destination_backend.refresh_shared_clipboard_state();

        assert_eq!(destination_backend.clipboard_count, 1);
        assert!(destination_backend.clipboard_cut);
        assert_eq!(
            QString::from_qvariant(destination_backend.clipboard_paths[0].clone())
                .unwrap()
                .to_string(),
            source.to_string_lossy()
        );
        publish_shared_clipboard(None);
    }

    fn test_search_state(generation: u64) -> SearchState {
        SearchState {
            query: "needle".to_owned(),
            filters: String::new(),
            recursive: false,
            truncated: false,
            visited: 0,
            generation,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn test_entry(name: &str) -> Entry {
        Entry {
            name: name.to_owned(),
            path: PathBuf::from(format!("/tmp/{name}")),
            is_dir: false,
            is_symlink: false,
            is_hidden: false,
            size: 0,
            modified_ms: 0.0,
            kind: String::new(),
            icon_url: String::new(),
            extension: String::new(),
            dev: 0,
            ino: 0,
            tag: -1,
            display_path: String::new(),
            tree_depth: 0,
            tree_expanded: false,
        }
    }

    fn test_search_result(tab_id: u64, generation: u64, name: &str) -> SearchResult {
        SearchResult {
            tab_id,
            generation,
            update: search::SearchUpdate {
                hits: vec![test_entry(name)],
                append: false,
                complete: true,
                truncated: false,
                visited: 1,
            },
        }
    }

    #[test]
    fn partial_search_results_append_while_search_remains_active() {
        let mut backend = test_backend_with_tabs();
        let tab = &mut backend.tabs_data[0];
        tab.search = Some(test_search_state(1));
        tab.search_generation = 1;
        tab.search_running = true;
        backend.sync_active(false);

        backend.apply_search_result(SearchResult {
            tab_id: 1,
            generation: 1,
            update: search::SearchUpdate {
                hits: vec![test_entry("first.txt")],
                append: true,
                complete: false,
                truncated: false,
                visited: 32,
            },
        });

        assert!(backend.searching);
        assert!(backend.tabs_data[0].search_running);
        assert_eq!(backend.tabs_data[0].entries[0].name, "first.txt");

        backend.apply_search_result(test_search_result(1, 1, "ranked.txt"));
        assert!(!backend.searching);
        assert_eq!(backend.tabs_data[0].entries.len(), 1);
        assert_eq!(backend.tabs_data[0].entries[0].name, "ranked.txt");
    }

    #[test]
    fn arbitrary_thumbnail_files_are_not_accepted_as_cache_hits() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("photo.jpg");
        fs::write(&source, b"source").unwrap();
        fs::write(dir.path().join("photo-thumb.png"), b"cached thumbnail").unwrap();
        let mut backend = Backend::default();
        let source_key = source.to_string_lossy().into_owned();

        assert!(backend.prepare_thumbnail_request(&source));
        assert!(backend.thumbnail_pending.contains(&source_key));
        assert!(backend.thumbnail_ready_path.to_string().is_empty());
    }

    #[test]
    fn thumbnail_cache_miss_starts_at_most_one_request() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("photo.jpg");
        let mut backend = Backend::default();
        let key = source.to_string_lossy().into_owned();

        assert!(backend.prepare_thumbnail_request(&source));
        assert!(!backend.prepare_thumbnail_request(&source));
        assert!(backend.thumbnail_pending.contains(&key));
        assert!(backend.thumbnail_ready_path.to_string().is_empty());
    }

    #[test]
    fn asynchronous_thumbnail_completion_notifies_its_source_path() {
        let mut backend = Backend::default();
        let path = "/tmp/photo.jpg".to_owned();
        let cache_url = "file:///tmp/cache/photo.png".to_owned();
        backend.thumbnail_pending.insert(path.clone());

        backend.apply_thumbnail_result(ThumbnailResult {
            path: path.clone(),
            result: Ok(cache_url.clone()),
        });

        assert!(!backend.thumbnail_pending.contains(&path));
        assert_eq!(backend.thumbnail_ready_path.to_string(), path);
        assert_eq!(backend.thumbnail_ready_cache_url, cache_url);
    }

    #[test]
    fn active_searching_state_tracks_the_selected_tab_not_its_search_session() {
        let mut backend = test_backend_with_tabs();
        backend.tabs_data[0].search = Some(test_search_state(1));
        backend.tabs_data[1].search = Some(test_search_state(1));
        backend.tabs_data[1].search_running = true;

        backend.sync_active(false);
        assert!(backend.search_mode);
        assert!(!backend.searching);

        backend.active = 1;
        backend.sync_active(false);
        assert!(backend.search_mode);
        assert!(backend.searching);

        backend.active = 0;
        backend.sync_active(false);
        assert!(!backend.searching);
        assert!(backend.tabs_data[0].search.is_some());
        assert!(backend.tabs_data[1].search.is_some());
    }

    #[test]
    fn completed_search_stops_running_but_keeps_the_search_session() {
        let mut backend = test_backend_with_tabs();
        let tab = &mut backend.tabs_data[0];
        tab.search = Some(test_search_state(1));
        tab.search_generation = 1;
        tab.search_running = true;
        backend.sync_active(false);

        backend.apply_search_result(test_search_result(1, 1, "match.txt"));

        assert!(!backend.searching);
        assert!(backend.search_mode);
        assert!(backend.tabs_data[0].search.is_some());
        assert!(!backend.tabs_data[0].search_running);
        assert_eq!(backend.tabs_data[0].entries[0].name, "match.txt");
    }

    #[test]
    fn stale_overlapping_search_result_cannot_replace_the_newer_result() {
        let mut backend = test_backend_with_tabs();
        let tab = &mut backend.tabs_data[0];
        tab.search = Some(test_search_state(2));
        tab.search_generation = 2;
        tab.search_running = true;
        tab.entries = vec![test_entry("current.txt")];
        backend.sync_active(false);

        backend.apply_search_result(test_search_result(1, 1, "stale.txt"));
        assert!(backend.searching);
        assert!(backend.tabs_data[0].search_running);
        assert_eq!(backend.tabs_data[0].entries[0].name, "current.txt");

        backend.apply_search_result(test_search_result(1, 2, "latest.txt"));
        assert!(!backend.searching);
        assert!(!backend.tabs_data[0].search_running);
        assert_eq!(backend.tabs_data[0].entries[0].name, "latest.txt");
    }

    #[test]
    fn restarting_search_cancels_the_previous_worker_token() {
        let mut state = test_search_state(1);
        let previous = state.cancel.clone();

        let current = state.restart();

        assert!(previous.load(Ordering::Relaxed));
        assert!(!current.load(Ordering::Relaxed));
        assert!(Arc::ptr_eq(&current, &state.cancel));
    }

    #[test]
    fn clearing_search_restores_folder_listing_reloaded_during_search() {
        let mut tab = Tab::new(1, PathBuf::from("/tmp/search-one"), SortSpec::default());
        tab.search = Some(test_search_state(2));
        tab.search_running = true;
        tab.entries = vec![test_entry("old-match.txt")];

        tab.install_folder_entries(vec![test_entry("refreshed-item.txt")]);
        assert_eq!(tab.entries[0].name, "old-match.txt");
        tab.entries = vec![test_entry("new-match.txt")];

        tab.finish_search();

        assert!(tab.search.is_none());
        assert!(!tab.search_running);
        assert!(tab.folder_entries.is_empty());
        assert_eq!(tab.entries[0].name, "refreshed-item.txt");
    }

    #[test]
    fn clearing_search_restores_an_empty_refreshed_folder() {
        let mut tab = Tab::new(1, PathBuf::from("/tmp/search-one"), SortSpec::default());
        tab.search = Some(test_search_state(1));
        tab.entries = vec![test_entry("stale-match.txt")];
        tab.install_folder_entries(Vec::new());

        tab.finish_search();

        assert!(tab.entries.is_empty());
    }

    #[test]
    fn default_application_id_is_parsed_from_gio_mime_output() {
        assert_eq!(
            default_application_id(
                "Default application for \"video/mp4\": org.videolan.VLC.desktop\nRegistered applications:\norg.gnome.Totem.desktop\n"
            ),
            Some("org.videolan.VLC.desktop")
        );
        assert_eq!(
            default_application_id(
                "Default application for \"application/octet-stream\": (none)\n"
            ),
            None
        );
        assert_eq!(
            default_application_id("Default application for \"text/plain\": /tmp/evil.desktop\n"),
            None
        );
        assert_eq!(
            default_application_id(
                "Default application for “audio/mpeg”: com.example.Player.desktop\n"
            ),
            Some("com.example.Player.desktop")
        );
    }

    #[test]
    fn gio_launch_and_default_open_pass_absolute_media_and_document_paths() {
        let dir = tempfile::tempdir().unwrap();
        let data_home = dir.path().join("data");
        let config_home = dir.path().join("config");
        let applications = data_home.join("applications");
        fs::create_dir_all(&applications).unwrap();
        fs::create_dir_all(&config_home).unwrap();

        let received_args = dir.path().join("received-args");
        let handler = dir.path().join("capture-handler");
        fs::write(
            &handler,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n",
                received_args.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&handler).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&handler, permissions).unwrap();

        let app_id = "filem-test-handler.desktop";
        let desktop_file = applications.join(app_id);
        fs::write(
            &desktop_file,
            format!(
                "[Desktop Entry]\nType=Application\nName=Filem Test Handler\nExec={} %f\nMimeType=video/mp4;audio/wav;application/pdf;\nNoDisplay=true\n",
                handler.display()
            ),
        )
        .unwrap();
        let associations = "[Default Applications]\nvideo/mp4=filem-test-handler.desktop;\naudio/wav=filem-test-handler.desktop;\napplication/pdf=filem-test-handler.desktop;\n";
        fs::write(config_home.join("mimeapps.list"), associations).unwrap();
        fs::write(applications.join("mimeapps.list"), associations).unwrap();

        let video = dir.path().join("clip sample.mp4");
        let video_status = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=64x48:d=0.25",
                "-c:v",
                "mpeg4",
                "-y",
            ])
            .arg(&video)
            .status()
            .expect("ffmpeg is required in the development shell");
        assert!(video_status.success());

        let audio = dir.path().join("tone.wav");
        let sample_rate = 8000u32;
        let sample_count = 80u32;
        let data_bytes = sample_count * 2;
        let mut wav = fs::File::create(&audio).unwrap();
        wav.write_all(b"RIFF").unwrap();
        wav.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
        wav.write_all(b"WAVEfmt ").unwrap();
        wav.write_all(&16u32.to_le_bytes()).unwrap();
        wav.write_all(&1u16.to_le_bytes()).unwrap();
        wav.write_all(&1u16.to_le_bytes()).unwrap();
        wav.write_all(&sample_rate.to_le_bytes()).unwrap();
        wav.write_all(&(sample_rate * 2).to_le_bytes()).unwrap();
        wav.write_all(&2u16.to_le_bytes()).unwrap();
        wav.write_all(&16u16.to_le_bytes()).unwrap();
        wav.write_all(b"data").unwrap();
        wav.write_all(&data_bytes.to_le_bytes()).unwrap();
        wav.write_all(&vec![0; data_bytes as usize]).unwrap();

        let pdf = dir.path().join("report.pdf");
        fs::write(
            &pdf,
            b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n",
        )
        .unwrap();

        for path in [&video, &audio, &pdf] {
            let absolute = absolute_paths(vec![path.to_path_buf()]).unwrap();
            assert!(absolute[0].is_absolute());

            let mut selected_app = open_with_command(&desktop_file, &absolute);
            let status = selected_app
                .env("XDG_DATA_HOME", &data_home)
                .env("XDG_CONFIG_HOME", &config_home)
                .output()
                .unwrap();
            assert!(
                status.status.success(),
                "Open With failed for {}: {}",
                path.display(),
                String::from_utf8_lossy(&status.stderr)
            );
            assert_eq!(
                fs::read_to_string(&received_args).unwrap().lines().last(),
                Some(path.to_str().unwrap())
            );

            let mut default_app = default_open_command(&absolute);
            let status = default_app
                .env("XDG_DATA_HOME", &data_home)
                .env("XDG_CONFIG_HOME", &config_home)
                .output()
                .unwrap();
            assert!(
                status.status.success(),
                "Open failed for {}: {}",
                path.display(),
                String::from_utf8_lossy(&status.stderr)
            );
            assert_eq!(
                fs::read_to_string(&received_args).unwrap().lines().last(),
                Some(path.to_str().unwrap())
            );
        }
    }

    #[test]
    fn same_entries_detects_model_content_changes() {
        let entry = |name: &str| Entry {
            name: name.to_owned(),
            path: PathBuf::from(format!("/tmp/{name}")),
            is_dir: false,
            is_symlink: false,
            is_hidden: false,
            size: 1,
            modified_ms: 10.0,
            kind: "Text".to_owned(),
            icon_url: String::new(),
            extension: "txt".to_owned(),
            dev: 1,
            ino: 2,
            tag: -1,
            display_path: String::new(),
            tree_depth: 0,
            tree_expanded: false,
        };
        let before = vec![entry("a.txt")];
        let unchanged = vec![entry("a.txt")];
        let renamed = vec![entry("b.txt")];

        assert!(same_entries(&before, &unchanged));
        assert!(!same_entries(&before, &renamed));
        assert!(!same_entries(&before, &[]));
    }

    #[test]
    fn tree_rows_preserve_branches_and_never_follow_symlinks() {
        let entry = |path: &str, name: &str, is_dir: bool, is_symlink: bool| Entry {
            name: name.to_owned(),
            path: PathBuf::from(path),
            is_dir,
            is_symlink,
            is_hidden: false,
            size: 0,
            modified_ms: 0.0,
            kind: String::new(),
            icon_url: String::new(),
            extension: String::new(),
            dev: 0,
            ino: 0,
            tag: -1,
            display_path: String::new(),
            tree_depth: 0,
            tree_expanded: false,
        };
        let roots = vec![
            entry("/root/alpha", "alpha", true, false),
            entry("/root/beta", "beta", true, false),
            entry("/root/shortcut", "shortcut", true, true),
        ];
        let mut children = HashMap::new();
        children.insert(
            PathBuf::from("/root/alpha"),
            vec![
                entry("/root/alpha/nested", "nested", true, false),
                entry("/root/alpha/a.txt", "a.txt", false, false),
            ],
        );
        children.insert(
            PathBuf::from("/root/alpha/nested"),
            vec![entry(
                "/root/alpha/nested/deep.txt",
                "deep.txt",
                false,
                false,
            )],
        );
        children.insert(
            PathBuf::from("/root/beta"),
            vec![entry("/root/beta/b.txt", "b.txt", false, false)],
        );
        children.insert(
            PathBuf::from("/root/shortcut"),
            vec![entry(
                "/root/shortcut/outside.txt",
                "outside.txt",
                false,
                false,
            )],
        );
        let expanded = HashSet::from([
            PathBuf::from("/root/alpha"),
            PathBuf::from("/root/alpha/nested"),
            PathBuf::from("/root/beta"),
            PathBuf::from("/root/shortcut"),
        ]);

        let rows = flatten_tree(&roots, &children, &expanded);
        assert_eq!(
            rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
            ["alpha", "nested", "deep.txt", "a.txt", "beta", "b.txt", "shortcut"]
        );
        assert_eq!(rows[2].tree_depth, 2);
        assert!(rows[0].tree_expanded);
        assert!(!rows[6].tree_expanded);

        let mut collapsed = expanded;
        collapsed.remove(Path::new("/root/alpha"));
        let rows = flatten_tree(&roots, &children, &collapsed);
        assert_eq!(
            rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
            ["alpha", "beta", "b.txt", "shortcut"]
        );
    }

    #[test]
    fn setting_tree_state_is_idempotent_and_preserves_other_branches() {
        let alpha = PathBuf::from("/root/alpha");
        let beta = PathBuf::from("/root/beta");
        let mut expanded = HashSet::new();

        assert!(set_expanded_path(&mut expanded, alpha.clone(), true));
        assert!(!set_expanded_path(&mut expanded, alpha.clone(), true));
        assert!(set_expanded_path(&mut expanded, beta.clone(), true));
        assert!(expanded.contains(&alpha));
        assert!(expanded.contains(&beta));
        assert!(set_expanded_path(&mut expanded, alpha.clone(), false));
        assert!(!expanded.contains(&alpha));
        assert!(expanded.contains(&beta));
    }

    #[test]
    fn batch_rename_executes_and_tracks_undo() {
        let mut backend = Backend::default();
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.txt");
        let second = dir.path().join("b.txt");
        fs::write(&first, b"A").unwrap();
        fs::write(&second, b"B").unwrap();
        let options = batch::BatchOptions {
            rules: vec![batch::RenameRule::Prefix("IMG_".to_owned())],
            numbering: true,
            start: 1,
            step: 1,
            padding: 2,
            number_placement: batch::NumberPlacement::Prefix,
            ..Default::default()
        };
        let rows = batch::plan(&[first, second], &options, &HashSet::new());
        let pairs = batch::apply(&rows).expect("batch rename should succeed");
        assert!(dir.path().join("01_IMG_a.txt").exists());
        assert!(dir.path().join("02_IMG_b.txt").exists());
        backend.push_undo(UndoOp::RenameMany { pairs });
        assert!(
            !backend.undo_stack.is_empty(),
            "rename should add undo state"
        );
    }

    #[test]
    fn batch_configuration_parser_rejects_unknown_rules() {
        let parsed = batch_options_from_json(
            r#"{"rules":[{"type":"replace","find":"draft","replace":"final","regex":true}],"extensionMode":"preserve","numbering":true,"start":4,"step":2,"padding":3,"numberPlacement":"template","sort":"name"}"#,
        )
        .unwrap();
        assert!(parsed.numbering);
        assert_eq!(parsed.start, 4);
        assert_eq!(parsed.step, 2);
        assert_eq!(parsed.padding, 3);
        assert_eq!(parsed.sort, batch::SortMode::Name);
        assert!(batch_options_from_json(r#"{"rules":[{"type":"mystery"}]}"#).is_err());
    }

    #[test]
    fn drag_drop_paths_move_selection_into_destination_folder() {
        let mut backend = Backend::default();
        let dir = tempfile::tempdir().unwrap();
        let source_dir = dir.path().join("source");
        let target_dir = dir.path().join("target");
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&target_dir).unwrap();
        let file = source_dir.join("note.txt");
        fs::write(&file, b"hello").unwrap();

        let mut paths = QVariantList::default();
        paths.push(QVariant::from(QString::from(
            file.to_string_lossy().to_string(),
        )));

        backend.drop_paths(
            QString::from(target_dir.to_string_lossy().to_string()),
            paths,
            true,
            false,
            QString::from("skip"),
        );

        for _ in 0..200 {
            if !file.exists() && target_dir.join("note.txt").exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(
            !file.exists(),
            "drag/drop should move the file out of the source folder"
        );
        assert_eq!(
            fs::read_to_string(target_dir.join("note.txt")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn drag_drop_copy_keeps_source_and_copies_to_destination() {
        let mut backend = Backend::default();
        let dir = tempfile::tempdir().unwrap();
        let source_dir = dir.path().join("source");
        let target_dir = dir.path().join("target");
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&target_dir).unwrap();
        let file = source_dir.join("note.txt");
        fs::write(&file, b"hello").unwrap();

        let mut paths = QVariantList::default();
        paths.push(QVariant::from(QString::from(
            file.to_string_lossy().to_string(),
        )));

        backend.drop_paths(
            QString::from(target_dir.to_string_lossy().to_string()),
            paths,
            false,
            false,
            QString::from("skip"),
        );

        let copied = target_dir.join("note.txt");
        for _ in 0..200 {
            if copied.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(file.exists(), "copy drag must leave the source in place");
        assert_eq!(fs::read(copied).unwrap(), b"hello");
    }

    #[test]
    fn drag_drop_automatically_moves_on_the_same_filesystem() {
        let mut backend = Backend::default();
        let dir = tempfile::tempdir().unwrap();
        let source_dir = dir.path().join("source");
        let target_dir = dir.path().join("target");
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&target_dir).unwrap();
        let file = source_dir.join("note.txt");
        fs::write(&file, b"hello").unwrap();

        let mut paths = QVariantList::default();
        paths.push(QVariant::from(QString::from(
            file.to_string_lossy().to_string(),
        )));

        backend.drop_paths(
            QString::from(target_dir.to_string_lossy().to_string()),
            paths,
            true,
            true,
            QString::from("skip"),
        );

        let moved = target_dir.join("note.txt");
        for _ in 0..200 {
            if moved.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(
            !file.exists(),
            "same-filesystem automatic drops should move"
        );
        assert_eq!(fs::read(moved).unwrap(), b"hello");
    }

    #[test]
    fn external_file_drop_copies_local_file_urls_by_default() {
        let mut backend = Backend::default();
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("notes & plans.txt");
        let target = dir.path().join("target");
        fs::write(&source, b"hello").unwrap();
        fs::create_dir(&target).unwrap();
        let mut urls = QVariantList::default();
        urls.push(QVariant::from(QString::from(paths::path_to_file_url(
            &source,
        ))));

        backend.drop_external_paths(
            QString::from(target.to_string_lossy().to_string()),
            urls,
            false,
        );

        for _ in 0..200 {
            if target.join(source.file_name().unwrap()).exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(source.exists(), "a copy drop must preserve its source");
        assert_eq!(
            fs::read_to_string(target.join(source.file_name().unwrap())).unwrap(),
            "hello"
        );
    }

    #[test]
    fn external_file_drop_rejects_non_local_urls() {
        let mut backend = Backend::default();
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::create_dir(&target).unwrap();
        let mut urls = QVariantList::default();
        urls.push(QVariant::from(QString::from(
            "https://example.com/file.txt",
        )));

        backend.drop_external_paths(
            QString::from(target.to_string_lossy().to_string()),
            urls,
            false,
        );

        assert!(backend
            .error
            .to_string()
            .contains("Only files from this computer"));
        assert_eq!(fs::read_dir(target).unwrap().count(), 0);
    }

    #[test]
    fn preview_map_contains_title_and_text_for_text_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        fs::write(&path, "hello\nworld\n").unwrap();

        let preview = Backend::preview_to_map(preview::build_pdf_page(&path, 1, 1200));
        let title = QString::from_qvariant(preview[QString::from("title")].clone()).unwrap();
        let kind = QString::from_qvariant(preview[QString::from("kind")].clone()).unwrap();
        let text = QString::from_qvariant(preview[QString::from("text")].clone()).unwrap();

        assert_eq!(title, QString::from("notes.txt"));
        assert!(kind.to_string().contains("Text"));
        assert!(text.to_string().contains("hello"));
    }

    #[test]
    fn set_view_mode_updates_backend_state() {
        let mut backend = Backend::default();
        backend.set_view_mode(QString::from("grid"));
        assert_eq!(backend.view_mode.to_string(), "grid");
        backend.set_view_mode(QString::from("column"));
        assert_eq!(backend.view_mode.to_string(), "column");
        backend.set_view_mode(QString::from("list"));
        assert_eq!(backend.view_mode.to_string(), "list");
    }
}
