//! Process-wide sequential transfer queue and bounded job history.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Instant;

const MAX_HISTORY: usize = 40;
const MAX_JOBS: usize = 120;
const MAX_ITEM_DETAILS: usize = 2_000;
const MAX_RETAINED_ITEM_DETAILS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Copy,
    Move,
    Trash,
    Delete,
    EmptyTrash,
    Restore,
}

impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::Move => "Move",
            Self::Trash => "Move to Trash",
            Self::Delete => "Permanent delete",
            Self::EmptyTrash => "Empty Trash",
            Self::Restore => "Restore",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    Waiting,
    Running,
    Completed,
    Skipped,
    Failed,
    Cancelled,
}

impl ItemStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Skipped | Self::Failed | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone)]
pub struct ItemSnapshot {
    pub path: PathBuf,
    pub is_directory: bool,
    pub status: ItemStatus,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct JobSnapshot {
    pub id: u64,
    pub operation: Operation,
    pub status: JobStatus,
    pub sources: Vec<PathBuf>,
    pub destination: Option<PathBuf>,
    pub total_files: usize,
    pub total_folders: usize,
    pub completed_files: usize,
    pub completed_folders: usize,
    pub skipped_items: usize,
    pub failed_items: usize,
    pub pending_items: usize,
    pub current_item: PathBuf,
    pub progress: f64,
    pub bytes_per_second: u64,
    pub eta_seconds: Option<u64>,
    pub bytes_completed: u64,
    pub total_bytes: u64,
    pub summary: String,
    pub error: String,
    pub items: Vec<ItemSnapshot>,
    pub omitted_item_details: usize,
}

impl JobSnapshot {
    pub fn source_summary(&self) -> String {
        self.sources
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemResult {
    Completed,
    Skipped,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct ItemInfo {
    pub path: PathBuf,
    pub is_directory: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct JobResult {
    pub status: JobStatus,
    pub summary: String,
    pub error: String,
}

impl JobResult {
    pub fn completed(summary: impl Into<String>) -> Self {
        Self {
            status: JobStatus::Completed,
            summary: summary.into(),
            error: String::new(),
        }
    }

    pub fn failed(summary: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            status: JobStatus::Failed,
            summary: summary.into(),
            error: error.into(),
        }
    }

    pub fn cancelled(summary: impl Into<String>) -> Self {
        Self {
            status: JobStatus::Cancelled,
            summary: summary.into(),
            error: String::new(),
        }
    }
}

type Worker = Box<dyn FnOnce(JobContext) -> JobResult + Send + 'static>;

struct JobEntry {
    snapshot: JobSnapshot,
    cancel: Arc<AtomicBool>,
    worker: Option<Worker>,
    started_at: Option<Instant>,
    last_progress_at: Option<Instant>,
    last_progress_bytes: u64,
    last_progress_item: PathBuf,
    last_progress_item_bytes: u64,
    untracked_completed_files: usize,
    untracked_completed_folders: usize,
    untracked_skipped: usize,
    untracked_failed: usize,
}

struct QueueState {
    jobs: VecDeque<JobEntry>,
    pending: VecDeque<u64>,
    active: Option<u64>,
    next_id: u64,
}

pub struct TransferManager {
    state: Mutex<QueueState>,
}

impl Default for TransferManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TransferManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(QueueState {
                jobs: VecDeque::new(),
                pending: VecDeque::new(),
                active: None,
                next_id: 1,
            }),
        }
    }

    pub fn enqueue(
        self: &Arc<Self>,
        operation: Operation,
        sources: Vec<PathBuf>,
        destination: Option<PathBuf>,
        worker: impl FnOnce(JobContext) -> JobResult + Send + 'static,
    ) -> Result<u64, String> {
        let id = {
            let mut state = self.lock_state();
            prune_history(&mut state);
            if state.jobs.len() >= MAX_JOBS {
                return Err(
                    "The transfer queue is full. Clear completed jobs and try again.".into(),
                );
            }
            let id = state.next_id;
            state.next_id = state.next_id.wrapping_add(1).max(1);
            let cancel = Arc::new(AtomicBool::new(false));
            let source_items = sources
                .iter()
                .map(|path| ItemSnapshot {
                    path: path.clone(),
                    is_directory: false,
                    status: ItemStatus::Waiting,
                    error: String::new(),
                })
                .collect::<Vec<_>>();
            state.jobs.push_back(JobEntry {
                snapshot: JobSnapshot {
                    id,
                    operation,
                    status: JobStatus::Queued,
                    sources,
                    destination,
                    total_files: source_items.len(),
                    total_folders: 0,
                    completed_files: 0,
                    completed_folders: 0,
                    skipped_items: 0,
                    failed_items: 0,
                    pending_items: source_items.len(),
                    current_item: PathBuf::new(),
                    progress: 0.0,
                    bytes_per_second: 0,
                    eta_seconds: None,
                    bytes_completed: 0,
                    total_bytes: 0,
                    summary: String::new(),
                    error: String::new(),
                    items: source_items,
                    omitted_item_details: 0,
                },
                cancel,
                worker: Some(Box::new(worker)),
                started_at: None,
                last_progress_at: None,
                last_progress_bytes: 0,
                last_progress_item: PathBuf::new(),
                last_progress_item_bytes: 0,
                untracked_completed_files: 0,
                untracked_completed_folders: 0,
                untracked_skipped: 0,
                untracked_failed: 0,
            });
            state.pending.push_back(id);
            id
        };
        self.start_next();
        Ok(id)
    }

    pub fn snapshots(&self) -> Vec<JobSnapshot> {
        self.lock_state()
            .jobs
            .iter()
            .map(|entry| entry.snapshot.clone())
            .collect()
    }

    pub fn active_snapshot(&self) -> Option<JobSnapshot> {
        let state = self.lock_state();
        let id = state.active?;
        state
            .jobs
            .iter()
            .find(|entry| entry.snapshot.id == id)
            .map(|entry| entry.snapshot.clone())
    }

    pub fn cancel(&self, id: u64) -> bool {
        let mut state = self.lock_state();
        let Some(entry) = state.jobs.iter_mut().find(|entry| entry.snapshot.id == id) else {
            return false;
        };
        match entry.snapshot.status {
            JobStatus::Queued => {
                entry.cancel.store(true, Ordering::Relaxed);
                entry.worker = None;
                entry.snapshot.status = JobStatus::Cancelled;
                entry.snapshot.summary = "Cancelled before starting".into();
                for item in &mut entry.snapshot.items {
                    item.status = ItemStatus::Cancelled;
                }
                entry.snapshot.pending_items = 0;
                state.pending.retain(|pending_id| *pending_id != id);
                true
            }
            JobStatus::Running => {
                entry.cancel.store(true, Ordering::Relaxed);
                true
            }
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled => false,
        }
    }

    pub fn cancel_pending(&self) -> usize {
        let mut state = self.lock_state();
        let ids = state.pending.drain(..).collect::<Vec<_>>();
        for id in &ids {
            if let Some(entry) = state.jobs.iter_mut().find(|entry| entry.snapshot.id == *id) {
                entry.cancel.store(true, Ordering::Relaxed);
                entry.worker = None;
                entry.snapshot.status = JobStatus::Cancelled;
                entry.snapshot.summary = "Cancelled before starting".into();
                for item in &mut entry.snapshot.items {
                    item.status = ItemStatus::Cancelled;
                }
                entry.snapshot.pending_items = 0;
            }
        }
        ids.len()
    }

    pub fn clear_completed(&self) {
        self.lock_state()
            .jobs
            .retain(|entry| entry.snapshot.status != JobStatus::Completed);
    }

    fn start_next(self: &Arc<Self>) {
        let task = {
            let mut state = self.lock_state();
            if state.active.is_some() {
                return;
            }
            let id = loop {
                match state.pending.pop_front() {
                    Some(id) => {
                        if let Some(entry) = state.jobs.iter_mut().find(|entry| {
                            entry.snapshot.id == id && entry.snapshot.status == JobStatus::Queued
                        }) {
                            entry.snapshot.status = JobStatus::Running;
                            entry.started_at = Some(Instant::now());
                            break id;
                        }
                    }
                    None => return,
                }
            };
            state.active = Some(id);
            let entry = state
                .jobs
                .iter_mut()
                .find(|entry| entry.snapshot.id == id)
                .expect("claimed transfer job exists");
            let worker = entry.worker.take().expect("queued transfer has a worker");
            (id, Arc::clone(&entry.cancel), worker)
        };

        let task_id = task.0;
        let manager = Arc::clone(self);
        let spawn_result = std::thread::Builder::new()
            .name(format!("filem-transfer-{}", task.0))
            .spawn(move || {
                let context = JobContext {
                    id: task.0,
                    cancel: task.1,
                    manager: Arc::downgrade(&manager),
                };
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (task.2)(context)))
                        .unwrap_or_else(|_| {
                            JobResult::failed(
                                "Transfer worker stopped unexpectedly",
                                "The operation encountered an internal error.",
                            )
                        });
                manager.finish(task.0, result);
                manager.start_next();
            });
        if let Err(error) = spawn_result {
            self.finish(
                task_id,
                JobResult::failed(
                    "Could not start transfer worker",
                    format!("Could not start transfer worker: {error}"),
                ),
            );
            self.start_next();
        }
    }

    fn finish(&self, id: u64, result: JobResult) {
        let mut state = self.lock_state();
        let Some(entry) = state.jobs.iter_mut().find(|entry| entry.snapshot.id == id) else {
            return;
        };
        entry.snapshot.status = result.status;
        entry.snapshot.summary = result.summary;
        entry.snapshot.error = result.error;
        entry.snapshot.current_item = PathBuf::new();
        if result.status == JobStatus::Cancelled {
            for item in &mut entry.snapshot.items {
                if matches!(item.status, ItemStatus::Waiting | ItemStatus::Running) {
                    item.status = ItemStatus::Cancelled;
                }
            }
        }
        refresh_counts(entry);
        if entry.snapshot.items.len() > MAX_RETAINED_ITEM_DETAILS {
            entry.snapshot.items.truncate(MAX_RETAINED_ITEM_DETAILS);
            entry.snapshot.omitted_item_details = entry
                .snapshot
                .total_files
                .saturating_add(entry.snapshot.total_folders)
                .saturating_sub(entry.snapshot.items.len());
        }
        if state.active == Some(id) {
            state.active = None;
        }
        prune_history(&mut state);
    }

    fn update(&self, id: u64, update: impl FnOnce(&mut JobEntry)) {
        let mut state = self.lock_state();
        if let Some(entry) = state.jobs.iter_mut().find(|entry| entry.snapshot.id == id) {
            update(entry);
            refresh_counts(entry);
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn prune_history(state: &mut QueueState) {
    while state
        .jobs
        .iter()
        .filter(|entry| entry.snapshot.status.terminal())
        .count()
        > MAX_HISTORY
    {
        let Some(index) = state
            .jobs
            .iter()
            .position(|entry| entry.snapshot.status.terminal())
        else {
            break;
        };
        state.jobs.remove(index);
    }
}

fn refresh_counts(entry: &mut JobEntry) {
    let tracked_files = entry
        .snapshot
        .items
        .iter()
        .filter(|item| !item.is_directory)
        .count();
    let tracked_folders = entry.snapshot.items.len().saturating_sub(tracked_files);
    let untracked_files = entry.snapshot.total_files.saturating_sub(tracked_files);
    let untracked_folders = entry.snapshot.total_folders.saturating_sub(tracked_folders);

    entry.snapshot.completed_files = entry
        .snapshot
        .items
        .iter()
        .filter(|item| !item.is_directory && item.status == ItemStatus::Completed)
        .count()
        + entry.untracked_completed_files.min(untracked_files);
    entry.snapshot.completed_folders = entry
        .snapshot
        .items
        .iter()
        .filter(|item| item.is_directory && item.status == ItemStatus::Completed)
        .count()
        + entry.untracked_completed_folders.min(untracked_folders);
    entry.snapshot.skipped_items = entry
        .snapshot
        .items
        .iter()
        .filter(|item| item.status == ItemStatus::Skipped)
        .count()
        + entry
            .untracked_skipped
            .min(untracked_files.saturating_add(untracked_folders));
    entry.snapshot.failed_items = entry
        .snapshot
        .items
        .iter()
        .filter(|item| item.status == ItemStatus::Failed)
        .count()
        + entry
            .untracked_failed
            .min(untracked_files.saturating_add(untracked_folders));

    let terminal = entry
        .snapshot
        .items
        .iter()
        .filter(|item| item.status.terminal())
        .count()
        + entry.untracked_completed_files
        + entry.untracked_completed_folders
        + entry.untracked_skipped
        + entry.untracked_failed;
    let running = entry
        .snapshot
        .items
        .iter()
        .filter(|item| item.status == ItemStatus::Running)
        .count();
    let total = entry
        .snapshot
        .total_files
        .saturating_add(entry.snapshot.total_folders);
    entry.snapshot.pending_items = total.saturating_sub(terminal + running);
    entry.snapshot.progress = if total == 0 {
        if entry.snapshot.status == JobStatus::Completed {
            1.0
        } else {
            0.0
        }
    } else {
        (terminal as f64 / total as f64).clamp(0.0, 1.0)
    };
}

fn global_manager() -> &'static Arc<TransferManager> {
    static MANAGER: OnceLock<Arc<TransferManager>> = OnceLock::new();
    MANAGER.get_or_init(|| Arc::new(TransferManager::new()))
}

pub fn enqueue(
    operation: Operation,
    sources: Vec<PathBuf>,
    destination: Option<PathBuf>,
    worker: impl FnOnce(JobContext) -> JobResult + Send + 'static,
) -> Result<u64, String> {
    global_manager().enqueue(operation, sources, destination, worker)
}

pub fn snapshots() -> Vec<JobSnapshot> {
    global_manager().snapshots()
}

pub fn active_job_id() -> Option<u64> {
    global_manager().active_snapshot().map(|job| job.id)
}

pub fn is_busy() -> bool {
    let state = global_manager().lock_state();
    state.active.is_some() || !state.pending.is_empty()
}

pub fn job(id: u64) -> Option<JobSnapshot> {
    global_manager()
        .snapshots()
        .into_iter()
        .find(|job| job.id == id)
}

pub fn cancel(id: u64) -> bool {
    global_manager().cancel(id)
}

pub fn cancel_pending() -> usize {
    global_manager().cancel_pending()
}

pub fn clear_completed() {
    global_manager().clear_completed();
}

pub struct JobContext {
    id: u64,
    cancel: Arc<AtomicBool>,
    manager: Weak<TransferManager>,
}

impl JobContext {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn cancellation_token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    pub fn set_inventory(&self, items: &[ItemInfo]) {
        let files = items.iter().filter(|item| !item.is_directory).count();
        let folders = items.len().saturating_sub(files);
        let total_bytes = items
            .iter()
            .filter(|item| !item.is_directory)
            .map(|item| item.size_bytes)
            .sum();
        self.set_inventory_counted(items, files, folders, total_bytes);
    }

    pub fn set_inventory_counted(
        &self,
        items: &[ItemInfo],
        total_files: usize,
        total_folders: usize,
        total_bytes: u64,
    ) {
        self.update(|entry| {
            entry.snapshot.total_files = total_files;
            entry.snapshot.total_folders = total_folders;
            entry.snapshot.total_bytes = total_bytes;
            entry.untracked_completed_files = 0;
            entry.untracked_completed_folders = 0;
            entry.untracked_skipped = 0;
            entry.untracked_failed = 0;
            entry.snapshot.items = items
                .iter()
                .take(MAX_ITEM_DETAILS)
                .map(|item| ItemSnapshot {
                    path: item.path.clone(),
                    is_directory: item.is_directory,
                    status: ItemStatus::Waiting,
                    error: String::new(),
                })
                .collect();
            entry.snapshot.omitted_item_details = total_files
                .saturating_add(total_folders)
                .saturating_sub(entry.snapshot.items.len());
        });
    }

    pub fn start_item(&self, path: &std::path::Path) {
        self.update(|entry| {
            entry.snapshot.current_item = path.to_path_buf();
            if let Some(item) = entry
                .snapshot
                .items
                .iter_mut()
                .find(|item| item.path == path && item.status == ItemStatus::Waiting)
            {
                item.status = ItemStatus::Running;
            }
        });
    }

    pub fn finish_item(
        &self,
        path: &std::path::Path,
        is_directory: bool,
        result: ItemResult,
        error: impl Into<String>,
    ) {
        let error = error.into();
        self.update(|entry| {
            if entry.snapshot.current_item == path {
                entry.snapshot.current_item = PathBuf::new();
            }
            let Some(item) = entry
                .snapshot
                .items
                .iter_mut()
                .find(|item| item.path == path)
            else {
                match result {
                    ItemResult::Completed if is_directory => entry.untracked_completed_folders += 1,
                    ItemResult::Completed => entry.untracked_completed_files += 1,
                    ItemResult::Skipped => entry.untracked_skipped += 1,
                    ItemResult::Failed => entry.untracked_failed += 1,
                    ItemResult::Cancelled => {}
                }
                return;
            };
            let previous = item.status;
            if previous.terminal() {
                return;
            }
            item.status = match result {
                ItemResult::Completed => ItemStatus::Completed,
                ItemResult::Skipped => ItemStatus::Skipped,
                ItemResult::Failed => ItemStatus::Failed,
                ItemResult::Cancelled => ItemStatus::Cancelled,
            };
            item.error = error;
        });
    }

    pub fn finish_subtree(
        &self,
        root: &std::path::Path,
        result: ItemResult,
        error: impl Into<String>,
    ) {
        let error = error.into();
        self.update(|entry| {
            if entry.snapshot.current_item == root || entry.snapshot.current_item.starts_with(root)
            {
                entry.snapshot.current_item = PathBuf::new();
            }
            for item in entry
                .snapshot
                .items
                .iter_mut()
                .filter(|item| item.path == root || item.path.starts_with(root))
            {
                if item.status.terminal()
                    && !(result == ItemResult::Failed && item.status == ItemStatus::Completed)
                {
                    continue;
                }
                item.status = match result {
                    ItemResult::Completed => ItemStatus::Completed,
                    ItemResult::Skipped => ItemStatus::Skipped,
                    ItemResult::Failed => ItemStatus::Failed,
                    ItemResult::Cancelled => ItemStatus::Cancelled,
                };
                item.error = error.clone();
            }
        });
    }

    pub fn report_file_progress(&self, path: &std::path::Path, completed: u64, _file_total: u64) {
        self.update(|entry| {
            let now = Instant::now();
            if entry.last_progress_item != path {
                entry.last_progress_item = path.to_path_buf();
                entry.last_progress_item_bytes = 0;
            }
            if completed >= entry.last_progress_item_bytes {
                entry.snapshot.bytes_completed = entry
                    .snapshot
                    .bytes_completed
                    .saturating_add(completed - entry.last_progress_item_bytes);
                entry.last_progress_item_bytes = completed;
            } else {
                entry.last_progress_item_bytes = completed;
            }
            if let Some(previous_at) = entry.last_progress_at {
                let elapsed = now.duration_since(previous_at).as_secs_f64();
                if elapsed >= 0.25 {
                    let bytes = entry
                        .snapshot
                        .bytes_completed
                        .saturating_sub(entry.last_progress_bytes);
                    entry.snapshot.bytes_per_second = (bytes as f64 / elapsed) as u64;
                    entry.last_progress_at = Some(now);
                    entry.last_progress_bytes = entry.snapshot.bytes_completed;
                }
            } else {
                entry.last_progress_at = Some(now);
                entry.last_progress_bytes = entry.snapshot.bytes_completed;
            }
            if entry.snapshot.total_bytes > 0 && entry.snapshot.bytes_per_second > 0 {
                entry.snapshot.eta_seconds = Some(
                    entry
                        .snapshot
                        .total_bytes
                        .saturating_sub(entry.snapshot.bytes_completed)
                        .div_ceil(entry.snapshot.bytes_per_second),
                );
            }
        });
    }

    fn update(&self, update: impl FnOnce(&mut JobEntry)) {
        if let Some(manager) = self.manager.upgrade() {
            manager.update(self.id, update);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn wait_for(manager: &TransferManager, id: u64, status: JobStatus) -> JobSnapshot {
        for _ in 0..200 {
            if let Some(job) = manager
                .snapshots()
                .into_iter()
                .find(|job| job.id == id && job.status == status)
            {
                return job;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("job {id} did not reach {status:?}");
    }

    #[test]
    fn jobs_run_sequentially_and_keep_terminal_history() {
        let manager = Arc::new(TransferManager::new());
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let first = manager
            .enqueue(Operation::Copy, vec![], None, move |_| {
                started_tx.send(1).unwrap();
                release_rx.recv().unwrap();
                JobResult::completed("first done")
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = manager
            .enqueue(Operation::Move, vec![], None, |_| {
                JobResult::completed("second done")
            })
            .unwrap();

        assert_eq!(
            manager
                .snapshots()
                .iter()
                .find(|job| job.id == second)
                .unwrap()
                .status,
            JobStatus::Queued
        );
        release_tx.send(()).unwrap();
        wait_for(&manager, first, JobStatus::Completed);
        wait_for(&manager, second, JobStatus::Completed);
    }

    #[test]
    fn cancelling_queued_job_prevents_worker_execution() {
        let manager = Arc::new(TransferManager::new());
        let (release_tx, release_rx) = mpsc::channel();
        let (ran_tx, ran_rx) = mpsc::channel();
        let _first = manager
            .enqueue(Operation::Copy, vec![], None, move |_| {
                release_rx.recv().unwrap();
                JobResult::completed("first done")
            })
            .unwrap();
        let second = manager
            .enqueue(Operation::Copy, vec![], None, move |_| {
                ran_tx.send(()).unwrap();
                JobResult::completed("should not run")
            })
            .unwrap();
        assert!(manager.cancel(second));
        release_tx.send(()).unwrap();
        assert!(ran_rx.recv_timeout(Duration::from_millis(100)).is_err());
        assert_eq!(
            manager
                .snapshots()
                .iter()
                .find(|job| job.id == second)
                .unwrap()
                .status,
            JobStatus::Cancelled
        );
    }

    #[test]
    fn cancellation_is_isolated_to_the_active_job() {
        let manager = Arc::new(TransferManager::new());
        let (release_tx, release_rx) = mpsc::channel();
        let first = manager
            .enqueue(Operation::Copy, vec![], None, move |context| {
                while !context.is_cancelled() {
                    if release_rx.recv_timeout(Duration::from_millis(5)).is_ok() {
                        break;
                    }
                }
                if context.is_cancelled() {
                    JobResult::cancelled("cancelled")
                } else {
                    JobResult::completed("done")
                }
            })
            .unwrap();
        let second = manager
            .enqueue(Operation::Move, vec![], None, |_| {
                JobResult::completed("done")
            })
            .unwrap();
        assert!(manager.cancel(first));
        release_tx.send(()).unwrap();
        wait_for(&manager, first, JobStatus::Cancelled);
        wait_for(&manager, second, JobStatus::Completed);
    }

    #[test]
    fn cancel_pending_stops_every_waiting_worker() {
        let manager = Arc::new(TransferManager::new());
        let (release_tx, release_rx) = mpsc::channel();
        let (ran_tx, ran_rx) = mpsc::channel();
        let _active = manager
            .enqueue(Operation::Copy, vec![], None, move |_| {
                release_rx.recv().unwrap();
                JobResult::completed("active done")
            })
            .unwrap();
        let first = manager
            .enqueue(Operation::Copy, vec![], None, {
                let ran_tx = ran_tx.clone();
                move |_| {
                    ran_tx.send(()).unwrap();
                    JobResult::completed("unexpected")
                }
            })
            .unwrap();
        let second = manager
            .enqueue(Operation::Copy, vec![], None, move |_| {
                ran_tx.send(()).unwrap();
                JobResult::completed("unexpected")
            })
            .unwrap();
        assert_eq!(manager.cancel_pending(), 2);
        release_tx.send(()).unwrap();
        assert!(ran_rx.recv_timeout(Duration::from_millis(100)).is_err());
        assert_eq!(
            manager
                .snapshots()
                .iter()
                .filter(|job| [first, second].contains(&job.id))
                .filter(|job| job.status == JobStatus::Cancelled)
                .count(),
            2
        );
    }

    #[test]
    fn submissions_from_separate_backends_share_the_process_queue() {
        let manager = global_manager();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first = enqueue(Operation::Copy, vec![], None, move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            JobResult::completed("first window")
        })
        .unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = enqueue(Operation::Copy, vec![], None, |_| {
            JobResult::completed("second window")
        })
        .unwrap();
        assert_eq!(
            manager
                .snapshots()
                .into_iter()
                .find(|job| job.id == second)
                .unwrap()
                .status,
            JobStatus::Queued
        );
        release_tx.send(()).unwrap();
        wait_for(manager, first, JobStatus::Completed);
        wait_for(manager, second, JobStatus::Completed);
    }

    #[test]
    fn recursive_inventory_counts_and_item_results_are_preserved() {
        let manager = Arc::new(TransferManager::new());
        let (done_tx, done_rx) = mpsc::channel();
        let id = manager
            .enqueue(Operation::Copy, vec![], None, move |context| {
                context.set_inventory(&[
                    ItemInfo {
                        path: PathBuf::from("/source"),
                        is_directory: true,
                        size_bytes: 0,
                    },
                    ItemInfo {
                        path: PathBuf::from("/source/a"),
                        is_directory: false,
                        size_bytes: 10,
                    },
                    ItemInfo {
                        path: PathBuf::from("/source/b"),
                        is_directory: false,
                        size_bytes: 20,
                    },
                ]);
                context.start_item(PathBuf::from("/source/a").as_path());
                context.finish_item(
                    PathBuf::from("/source/a").as_path(),
                    false,
                    ItemResult::Completed,
                    "",
                );
                context.finish_item(
                    PathBuf::from("/source/b").as_path(),
                    false,
                    ItemResult::Failed,
                    "permission denied",
                );
                done_tx.send(()).unwrap();
                JobResult::failed("partial transfer", "one item failed")
            })
            .unwrap();
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let job = wait_for(&manager, id, JobStatus::Failed);
        assert_eq!(job.total_files, 2);
        assert_eq!(job.total_folders, 1);
        assert_eq!(job.completed_files, 1);
        assert_eq!(job.failed_items, 1);
        assert!(job.items.iter().any(|item| {
            item.path == PathBuf::from("/source/b")
                && item.status == ItemStatus::Failed
                && item.error == "permission denied"
        }));
    }
}
