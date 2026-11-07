use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::paths;
use crate::transfer::JobSnapshot;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static EXPORT_ERROR_REPORTED: AtomicBool = AtomicBool::new(false);

fn export_directory() -> PathBuf {
    paths::cache_dir().join("filem/noctalia")
}

fn status_file(directory: &Path, pid: u32) -> PathBuf {
    directory.join(format!("transfers-{pid}.json"))
}

fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn payload(pid: u32, updated_at_ms: u64, jobs: &[JobSnapshot]) -> Value {
    let jobs = jobs
        .iter()
        .map(|job| {
            json!({
                "id": format!("{pid}-{}", job.id),
                "operation": job.operation.label(),
                "status": job.status.as_str(),
                "source": job.sources.first()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                "sourceCount": job.sources.len(),
                "destination": job.destination.as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                "currentItem": job.current_item.to_string_lossy().into_owned(),
                "progress": job.progress,
                "completedFiles": job.completed_files,
                "totalFiles": job.total_files,
                "completedFolders": job.completed_folders,
                "totalFolders": job.total_folders,
                "pendingItems": job.pending_items,
                "skippedItems": job.skipped_items,
                "failedItems": job.failed_items,
                "bytesCompleted": job.bytes_completed,
                "totalBytes": job.total_bytes,
                "sizeKnown": job.inventory_ready,
                "bytesPerSecond": job.bytes_per_second,
                "etaSeconds": job.eta_seconds,
                "startedAtMs": job.started_at_ms,
                "finishedAtMs": job.finished_at_ms,
                "summary": job.summary,
                "error": job.error,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "schema": 1,
        "pid": pid,
        "updatedAtMs": updated_at_ms,
        "jobs": jobs,
    })
}

fn write_status(
    directory: &Path,
    pid: u32,
    updated_at_ms: u64,
    jobs: &[JobSnapshot],
) -> io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let destination = status_file(directory, pid);
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(".transfers-{pid}-{updated_at_ms}-{sequence}.tmp"));

    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        serde_json::to_writer(&mut file, &payload(pid, updated_at_ms, jobs))
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        file.flush()?;
        fs::rename(&temporary, &destination)?;
        Ok(destination.clone())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(temporary);
    }

    write_result
}

pub fn publish(jobs: &[JobSnapshot]) {
    match write_status(&export_directory(), std::process::id(), epoch_millis(), jobs) {
        Ok(_) => EXPORT_ERROR_REPORTED.store(false, Ordering::Relaxed),
        Err(error) => {
            if !EXPORT_ERROR_REPORTED.swap(true, Ordering::Relaxed) {
                eprintln!("Could not publish Filem transfer status for Noctalia: {error}");
            }
        }
    }
}

pub fn cleanup() {
    match fs::remove_file(status_file(&export_directory(), std::process::id())) {
        Ok(()) => EXPORT_ERROR_REPORTED.store(false, Ordering::Relaxed),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            if !EXPORT_ERROR_REPORTED.swap(true, Ordering::Relaxed) {
                eprintln!("Could not remove Filem transfer status for Noctalia: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::{JobStatus, Operation};
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn status_export_is_private_and_contains_live_job_details() {
        let directory = tempfile::tempdir().unwrap();
        let job = JobSnapshot {
            id: 7,
            operation: Operation::Copy,
            status: JobStatus::Running,
            sources: vec![PathBuf::from("/source/photo.jpg")],
            destination: Some(PathBuf::from("/destination")),
            total_files: 1,
            total_folders: 0,
            completed_files: 0,
            completed_folders: 0,
            skipped_items: 0,
            failed_items: 0,
            pending_items: 1,
            current_item: PathBuf::from("/source/photo.jpg"),
            progress: 0.4,
            bytes_per_second: 1024,
            eta_seconds: Some(5),
            bytes_completed: 4096,
            total_bytes: 10240,
            inventory_ready: true,
            started_at_ms: Some(100),
            finished_at_ms: None,
            summary: String::new(),
            error: String::new(),
            items: Vec::new(),
            omitted_item_details: 0,
        };

        let file = write_status(directory.path(), 42, 123, &[job]).unwrap();
        let value: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();

        assert_eq!(value["schema"], 1);
        assert_eq!(value["pid"], 42);
        assert_eq!(value["updatedAtMs"], 123);
        assert_eq!(value["jobs"][0]["id"], "42-7");
        assert_eq!(value["jobs"][0]["source"], "/source/photo.jpg");
        assert_eq!(value["jobs"][0]["destination"], "/destination");
        assert_eq!(value["jobs"][0]["progress"], 0.4);
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
