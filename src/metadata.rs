//! On-demand, bounded metadata extraction for Properties.

use std::collections::VecDeque;
use std::fs;
use std::io::{BufReader, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const MAX_CACHE_ENTRIES: usize = 96;
const MAX_TOOL_OUTPUT: usize = 2 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ARCHIVE_ITEMS: usize = 100_000;
const MAX_DISPLAYED_ARCHIVE_ITEMS: usize = 250;
const MAX_EXTERNAL_TOOL_TIME: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct MetadataField {
    pub section: String,
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Default)]
pub struct MetadataReport {
    pub mime_type: String,
    pub actual_type: String,
    pub fields: Vec<MetadataField>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    path: PathBuf,
    device: u64,
    inode: u64,
    size: u64,
    modified_sec: i64,
    modified_nsec: i64,
}

fn cache() -> &'static Mutex<VecDeque<(CacheKey, MetadataReport)>> {
    static CACHE: OnceLock<Mutex<VecDeque<(CacheKey, MetadataReport)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub fn clear_cache() {
    if let Ok(mut entries) = cache().lock() {
        entries.clear();
    }
}

fn extraction_slots() -> &'static (Mutex<usize>, Condvar) {
    static SLOTS: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
    SLOTS.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

struct ExtractionSlot;

impl ExtractionSlot {
    fn acquire(cancel: &AtomicBool) -> Option<Self> {
        let (active, available) = extraction_slots();
        let mut active = active.lock().ok()?;
        while *active >= 2 {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let (next, _) = available
                .wait_timeout(active, Duration::from_millis(100))
                .ok()?;
            active = next;
        }
        *active += 1;
        Some(Self)
    }
}

pub struct ExtractionJob {
    _slot: ExtractionSlot,
}

pub fn begin_job(cancel: &AtomicBool) -> Result<ExtractionJob, String> {
    ExtractionSlot::acquire(cancel)
        .map(|slot| ExtractionJob { _slot: slot })
        .ok_or_else(|| "Metadata extraction was cancelled.".to_owned())
}

impl Drop for ExtractionSlot {
    fn drop(&mut self) {
        let (active, available) = extraction_slots();
        if let Ok(mut active) = active.lock() {
            *active = active.saturating_sub(1);
            available.notify_one();
        }
    }
}

fn cache_key(path: &Path) -> Result<CacheKey, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    Ok(CacheKey {
        path: path.to_path_buf(),
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.len(),
        modified_sec: metadata.mtime(),
        modified_nsec: metadata.mtime_nsec(),
    })
}

pub fn extract_with_job(
    path: &Path,
    cancel: &AtomicBool,
    _job: &ExtractionJob,
) -> Result<MetadataReport, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Metadata extraction was cancelled.".to_owned());
    }
    let key = cache_key(path)?;
    if let Ok(mut entries) = cache().lock() {
        if let Some(position) = entries.iter().position(|(existing, _)| existing == &key) {
            let cached = entries.remove(position).expect("position came from cache");
            let report = cached.1.clone();
            entries.push_front(cached);
            return Ok(report);
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Metadata extraction was cancelled.".to_owned());
    }
    let report = extract_uncached(path, cancel)?;
    if !cancel.load(Ordering::Relaxed) {
        if let Ok(mut entries) = cache().lock() {
            entries.retain(|(existing, _)| existing.path != key.path);
            entries.push_front((key, report.clone()));
            entries.truncate(MAX_CACHE_ENTRIES);
        }
    }
    Ok(report)
}

fn extract_uncached(path: &Path, cancel: &AtomicBool) -> Result<MetadataReport, String> {
    let mut report = MetadataReport::default();
    report.mime_type = match run_tool("file", &["--brief", "--mime-type", "--"], path, cancel) {
        Ok(output) if !output.trim().is_empty() => output.trim().to_owned(),
        Ok(_) => "application/octet-stream".to_owned(),
        Err(error) => {
            report.warnings.push(error);
            "application/octet-stream".to_owned()
        }
    };
    match run_tool("file", &["--brief", "--"], path, cancel) {
        Ok(actual_type) => report.actual_type = actual_type.trim().to_owned(),
        Err(error) => report.warnings.push(error),
    }
    let mime_type = report.mime_type.clone();
    add(&mut report, "General", "MIME type", &mime_type);
    let actual_type = report.actual_type.clone();
    add(&mut report, "General", "Detected file type", &actual_type);

    if cancel.load(Ordering::Relaxed) {
        return Err("Metadata extraction was cancelled.".to_owned());
    }
    let mime = report.mime_type.clone();
    let path_name = path.file_name().unwrap_or_default().to_string_lossy();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
    {
        // Filesystem folder counts and recursive size come from ops::properties.
    } else if is_office_document(&mime, &extension) {
        inspect_office(path, &mut report, cancel);
    } else if crate::ops::archive_format(path).is_some()
        || matches!(extension.as_str(), "7z" | "rar")
        || mime.contains("7z-compressed")
        || mime.contains("vnd.rar")
        || is_zip_signature(path)
    {
        inspect_archive(path, &mut report, cancel);
    } else if mime == "application/pdf" || extension == "pdf" {
        inspect_pdf(path, &mut report, cancel);
    } else if mime.starts_with("audio/") || mime.starts_with("video/") {
        inspect_media(path, &mut report, cancel);
    } else if mime.starts_with("image/") {
        inspect_image(path, &mut report, cancel);
    } else if mime.starts_with("text/") || is_text_extension(&extension) {
        inspect_text(path, &mut report, cancel);
    } else if !path_name.is_empty() {
        report
            .warnings
            .push("No specialized parser is available for this detected file type.".to_owned());
    }
    Ok(report)
}

fn is_zip_signature(path: &Path) -> bool {
    let mut signature = [0; 4];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut signature))
        .is_ok()
        && signature[0..2] == *b"PK"
        && matches!(&signature[2..4], [3, 4] | [5, 6] | [7, 8])
}

fn add(report: &mut MetadataReport, section: &str, label: &str, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        report.fields.push(MetadataField {
            section: section.to_owned(),
            label: label.to_owned(),
            value: value.to_owned(),
        });
    }
}

fn run_tool(
    program: &str,
    args: &[&str],
    path: &Path,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Metadata extraction was cancelled.".to_owned());
    }
    let mut child = Command::new(program)
        .args(args)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("{program} is unavailable: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("Could not read {program} output."))?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        BufReader::new(stdout)
            .take((MAX_TOOL_OUTPUT + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + MAX_EXTERNAL_TOOL_TIME;
    loop {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(if cancel.load(Ordering::Relaxed) {
                "Metadata extraction was cancelled.".to_owned()
            } else {
                format!("{program} exceeded the metadata time limit.")
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let bytes = reader
                    .join()
                    .map_err(|_| format!("{program} output reader failed."))?
                    .map_err(|error| format!("Could not read {program} output: {error}"))?;
                if bytes.len() > MAX_TOOL_OUTPUT {
                    return Err(format!("{program} output exceeded the size limit."));
                }
                if !status.success() {
                    return Err(format!("{program} could not parse this file."));
                }
                return Ok(String::from_utf8_lossy(&bytes).into_owned());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(format!("Could not wait for {program}: {error}"));
            }
        }
    }
}

fn inspect_archive(path: &Path, report: &mut MetadataReport, cancel: &AtomicBool) {
    if crate::ops::archive_format(path) == Some(crate::ops::ArchiveFormat::Zip)
        || is_zip_signature(path)
    {
        inspect_zip(path, report);
    } else if crate::ops::archive_format(path).is_some() {
        match crate::ops::archive_summary(path, MAX_ARCHIVE_ITEMS) {
            Ok(summary) => {
                add(report, "Type-specific Details", "Archive format", "TAR");
                add(
                    report,
                    "Type-specific Details",
                    "Entry count",
                    &summary.entries.to_string(),
                );
                let compressed = fs::metadata(path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                add(
                    report,
                    "Type-specific Details",
                    "Compressed size",
                    &format_bytes(compressed),
                );
                add(
                    report,
                    "Type-specific Details",
                    "Uncompressed size",
                    &format_bytes(summary.uncompressed_bytes),
                );
                add(
                    report,
                    "Type-specific Details",
                    "Compression ratio",
                    &ratio(compressed, summary.uncompressed_bytes),
                );
                match crate::ops::archive_list(path, MAX_DISPLAYED_ARCHIVE_ITEMS) {
                    Ok(items) => {
                        for item in items {
                            add(report, "Contained files", "Entry", &item);
                        }
                    }
                    Err(error) => report.warnings.push(error),
                }
                if summary.truncated {
                    report.warnings.push(
                        "Archive listing stopped at the 100,000-entry safety limit.".to_owned(),
                    );
                }
                if path
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .contains(".tar.")
                {
                    add(
                        report,
                        "Type-specific Details",
                        "Compression",
                        path.extension()
                            .and_then(|value| value.to_str())
                            .unwrap_or("compressed"),
                    );
                }
            }
            Err(error) => report.warnings.push(error),
        }
    } else if let Ok(output) = run_tool("7z", &["l", "-slt", "--"], path, cancel) {
        inspect_7z_listing(&output, report);
    } else {
        report
            .warnings
            .push("No installed parser supports this archive format.".to_owned());
    }
}

fn inspect_zip(path: &Path, report: &mut MetadataReport) {
    let result = (|| -> Result<(), String> {
        let file = fs::File::open(path).map_err(|error| error.to_string())?;
        let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
            .map_err(|error| format!("Could not parse ZIP archive: {error}"))?;
        let mut uncompressed = 0u64;
        let mut compressed = 0u64;
        let mut encrypted = false;
        let total_entries = archive.len();
        for index in 0..total_entries.min(MAX_ARCHIVE_ITEMS) {
            let entry = archive
                .by_index(index)
                .map_err(|error| format!("Could not inspect ZIP entry: {error}"))?;
            uncompressed = uncompressed.saturating_add(entry.size());
            compressed = compressed.saturating_add(entry.compressed_size());
            encrypted |= entry.encrypted();
            if index < MAX_DISPLAYED_ARCHIVE_ITEMS {
                let name = entry.name().to_owned();
                add(report, "Contained files", "Entry", &name);
            }
        }
        add(report, "Type-specific Details", "Archive format", "ZIP");
        add(
            report,
            "Type-specific Details",
            "Entry count",
            &total_entries.to_string(),
        );
        add(
            report,
            "Type-specific Details",
            "Compressed size",
            &format_bytes(compressed),
        );
        add(
            report,
            "Type-specific Details",
            "Uncompressed size",
            &format_bytes(uncompressed),
        );
        add(
            report,
            "Type-specific Details",
            "Compression ratio",
            &ratio(compressed, uncompressed),
        );
        add(
            report,
            "Type-specific Details",
            "Encrypted entries",
            if encrypted { "Yes" } else { "No" },
        );
        let comment = String::from_utf8_lossy(archive.comment());
        add(report, "Type-specific Details", "Archive comment", &comment);
        if total_entries > MAX_ARCHIVE_ITEMS {
            report
                .warnings
                .push("Archive inspection stopped at the 100,000-entry safety limit.".to_owned());
        }
        Ok(())
    })();
    if let Err(error) = result {
        report.warnings.push(error);
    }
}

fn inspect_7z_listing(output: &str, report: &mut MetadataReport) {
    let mut entries = 0usize;
    let mut compressed = 0u64;
    let mut uncompressed = 0u64;
    let mut compressed_known = false;
    let mut uncompressed_known = false;
    let mut encrypted = false;
    let mut encryption_known = false;
    let mut archive_comment = String::new();
    for record in output.split("\n\n") {
        let fields = record
            .lines()
            .filter_map(|line| line.split_once(" = "))
            .collect::<std::collections::HashMap<_, _>>();
        let Some(path) = fields.get("Path").copied() else {
            continue;
        };
        if fields.contains_key("Type") || fields.contains_key("Physical Size") {
            if let Some(comment) = fields.get("Comment") {
                archive_comment = (*comment).to_owned();
            }
            continue;
        }
        entries += 1;
        if entries <= MAX_DISPLAYED_ARCHIVE_ITEMS {
            add(report, "Contained files", "Entry", path);
        }
        if let Some(size) = fields
            .get("Size")
            .and_then(|value| value.parse::<u64>().ok())
        {
            uncompressed = uncompressed.saturating_add(size);
            uncompressed_known = true;
        }
        if let Some(size) = fields
            .get("Packed Size")
            .and_then(|value| value.parse::<u64>().ok())
        {
            compressed = compressed.saturating_add(size);
            compressed_known = true;
        }
        if let Some(value) = fields.get("Encrypted") {
            encryption_known = true;
            encrypted |= value.eq_ignore_ascii_case("+");
        }
        if let Some(comment) = fields.get("Comment") {
            archive_comment = (*comment).to_owned();
        }
    }
    let archive_format = if report.actual_type.is_empty() {
        "7z / RAR".to_owned()
    } else {
        report.actual_type.clone()
    };
    add(
        report,
        "Type-specific Details",
        "Archive format",
        &archive_format,
    );
    add(
        report,
        "Type-specific Details",
        "Entry count",
        &entries.to_string(),
    );
    if compressed_known {
        add(
            report,
            "Type-specific Details",
            "Compressed size",
            &format_bytes(compressed),
        );
    }
    if uncompressed_known {
        add(
            report,
            "Type-specific Details",
            "Uncompressed size",
            &format_bytes(uncompressed),
        );
    }
    if compressed_known && uncompressed_known {
        add(
            report,
            "Type-specific Details",
            "Compression ratio",
            &ratio(compressed, uncompressed),
        );
    }
    if encryption_known {
        add(
            report,
            "Type-specific Details",
            "Encrypted entries",
            if encrypted { "Yes" } else { "No" },
        );
    }
    add(
        report,
        "Type-specific Details",
        "Archive comment",
        &archive_comment,
    );
}

fn inspect_pdf(path: &Path, report: &mut MetadataReport, cancel: &AtomicBool) {
    match run_tool("pdfinfo", &["-isodates", "-box", "--"], path, cancel) {
        Ok(output) => {
            for line in output.lines() {
                if let Some((label, value)) = line.split_once(':') {
                    add(report, "Type-specific Details", label, value);
                }
            }
        }
        Err(error) => report.warnings.push(error),
    }
}

fn inspect_media(path: &Path, report: &mut MetadataReport, cancel: &AtomicBool) {
    match run_tool(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-show_chapters",
            "-of",
            "json",
            "--",
        ],
        path,
        cancel,
    ) {
        Ok(output) => match serde_json::from_str::<serde_json::Value>(&output) {
            Ok(value) => inspect_ffprobe_json(&value, report),
            Err(error) => report
                .warnings
                .push(format!("Could not parse ffprobe metadata: {error}")),
        },
        Err(error) => report.warnings.push(error),
    }
    inspect_exiftool(path, report, cancel);
}

fn inspect_ffprobe_json(value: &serde_json::Value, report: &mut MetadataReport) {
    if let Some(format) = value.get("format") {
        add_json(
            format,
            "Container",
            "format_name",
            "Container format",
            report,
        );
        add_json(format, "Container", "format_long_name", "Format", report);
        add_json(
            format,
            "Container",
            "duration",
            "Duration (seconds)",
            report,
        );
        add_json(
            format,
            "Container",
            "size",
            "Container size (bytes)",
            report,
        );
        add_json(
            format,
            "Container",
            "bit_rate",
            "Overall bitrate (bps)",
            report,
        );
        add_json(
            format,
            "Container",
            "probe_score",
            "Probe confidence",
            report,
        );
        if let Some(tags) = format.get("tags").and_then(serde_json::Value::as_object) {
            for (key, value) in tags {
                add_json_value(value, "Embedded metadata", key, report);
            }
        }
    }
    if let Some(streams) = value.get("streams").and_then(serde_json::Value::as_array) {
        for stream in streams {
            let index = stream
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let kind = stream
                .get("codec_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Other");
            let section = format!("{} track {}", capitalize(kind), index + 1);
            if stream
                .get("disposition")
                .and_then(|disposition| disposition.get("attached_pic"))
                .and_then(serde_json::Value::as_u64)
                == Some(1)
            {
                add(report, &section, "Embedded artwork", "Yes");
            }
            for (key, label) in [
                ("codec_name", "Codec"),
                ("codec_long_name", "Codec description"),
                ("profile", "Profile"),
                ("width", "Width"),
                ("height", "Height"),
                ("pix_fmt", "Pixel format"),
                ("bits_per_sample", "Bit depth"),
                ("sample_rate", "Sample rate (Hz)"),
                ("channels", "Channels"),
                ("channel_layout", "Channel layout"),
                ("bit_rate", "Bitrate (bps)"),
                ("duration", "Duration (seconds)"),
                ("avg_frame_rate", "Average frame rate"),
                ("r_frame_rate", "Nominal frame rate"),
                ("display_aspect_ratio", "Display aspect ratio"),
                ("sample_aspect_ratio", "Sample aspect ratio"),
                ("level", "Codec level"),
                ("time_base", "Time base"),
                ("start_time", "Start time"),
            ] {
                if matches!(key, "avg_frame_rate" | "r_frame_rate") {
                    if let Some(value) = stream.get(key).and_then(serde_json::Value::as_str) {
                        if let Some(rate) = frame_rate(value) {
                            add(report, &section, label, &rate);
                        }
                    }
                } else {
                    add_json(stream, &section, key, label, report);
                }
            }
            if let Some(tags) = stream.get("tags").and_then(serde_json::Value::as_object) {
                for (key, value) in tags {
                    add_json_value(value, &section, key, report);
                }
            }
        }
    }
    if let Some(chapters) = value.get("chapters").and_then(serde_json::Value::as_array) {
        add(
            report,
            "Container",
            "Chapter count",
            &chapters.len().to_string(),
        );
        for (index, chapter) in chapters.iter().enumerate().take(200) {
            if let Some(tags) = chapter.get("tags").and_then(serde_json::Value::as_object) {
                for (key, value) in tags {
                    add_json_value(value, "Chapters", &format!("{} {}", index + 1, key), report);
                }
            }
        }
    }
}

fn add_json(
    object: &serde_json::Value,
    section: &str,
    key: &str,
    label: &str,
    report: &mut MetadataReport,
) {
    if let Some(value) = object.get(key) {
        add_json_value(value, section, label, report);
    }
}

fn add_json_value(
    value: &serde_json::Value,
    section: &str,
    label: &str,
    report: &mut MetadataReport,
) {
    if let Some(value) = value.as_str() {
        add(report, section, label, value);
    } else if value.is_number() || value.is_boolean() {
        add(report, section, label, &value.to_string());
    } else if let Some(values) = value.as_array() {
        let values = values
            .iter()
            .filter_map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| value.is_number().then(|| value.to_string()))
            })
            .collect::<Vec<_>>()
            .join(", ");
        add(report, section, label, &values);
    }
}

fn frame_rate(value: &str) -> Option<String> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    if denominator == 0.0 {
        return None;
    }
    let rate = numerator / denominator;
    (rate.is_finite() && rate > 0.0).then(|| format!("{rate:.3} fps"))
}

fn inspect_image(path: &Path, report: &mut MetadataReport, cancel: &AtomicBool) {
    inspect_exiftool(path, report, cancel);
}

fn inspect_exiftool(path: &Path, report: &mut MetadataReport, cancel: &AtomicBool) {
    let output = run_tool(
        "exiftool",
        &["-json", "-G1", "-n", "-s", "--"],
        path,
        cancel,
    );
    match output {
        Ok(output) => match serde_json::from_str::<serde_json::Value>(&output) {
            Ok(value) => {
                if let Some(object) = value
                    .as_array()
                    .and_then(|values| values.first())
                    .and_then(serde_json::Value::as_object)
                {
                    for (key, value) in object {
                        if key == "SourceFile" {
                            continue;
                        }
                        add_json_value(value, "Metadata", key, report);
                    }
                }
            }
            Err(error) => report
                .warnings
                .push(format!("Could not parse ExifTool metadata: {error}")),
        },
        Err(error) => report.warnings.push(error),
    }
}

fn is_office_document(mime: &str, extension: &str) -> bool {
    mime.contains("officedocument")
        || mime.contains("msword")
        || mime.contains("ms-excel")
        || mime.contains("ms-powerpoint")
        || matches!(
            extension,
            "docx"
                | "docm"
                | "xlsx"
                | "xlsm"
                | "pptx"
                | "pptm"
                | "odt"
                | "ods"
                | "odp"
                | "doc"
                | "dot"
                | "xls"
                | "xlt"
                | "ppt"
                | "pot"
                | "rtf"
        )
}

fn inspect_office(path: &Path, report: &mut MetadataReport, cancel: &AtomicBool) {
    let Ok(file) = fs::File::open(path) else {
        report
            .warnings
            .push("Could not open the Office document.".to_owned());
        return;
    };
    let Ok(mut archive) = zip::ZipArchive::new(std::io::BufReader::new(file)) else {
        if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("doc" | "dot" | "xls" | "xlt" | "ppt" | "pot" | "rtf")
        ) {
            inspect_exiftool(path, report, cancel);
        } else {
            report.warnings.push(
                "This Office document is not a readable ZIP-based document; ExifTool may still provide generic metadata."
                    .to_owned(),
            );
            inspect_exiftool(path, report, cancel);
        }
        return;
    };
    for (member, section) in [
        ("docProps/core.xml", "Document metadata"),
        ("docProps/app.xml", "Document statistics"),
        ("meta.xml", "Document metadata"),
    ] {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let Ok(mut entry) = archive.by_name(member) else {
            continue;
        };
        if entry.size() > 2 * 1024 * 1024 {
            report
                .warnings
                .push(format!("{member} exceeds the metadata size limit."));
            continue;
        }
        let mut xml = String::new();
        if entry.read_to_string(&mut xml).is_err() {
            report.warnings.push(format!("Could not read {member}."));
            continue;
        }
        for (tag, label) in [
            ("dc:title", "Title"),
            ("dc:subject", "Subject"),
            ("dc:creator", "Author"),
            ("cp:lastModifiedBy", "Last modified by"),
            ("cp:keywords", "Keywords"),
            ("dcterms:created", "Created"),
            ("dcterms:modified", "Modified"),
            ("Application", "Application"),
            ("Pages", "Pages"),
            ("Words", "Words"),
            ("Characters", "Characters"),
            ("Slides", "Slides"),
            ("Company", "Company"),
            ("Paragraphs", "Paragraphs"),
        ] {
            if let Some(value) = xml_element_text(&xml, tag) {
                add(report, section, label, &value);
            }
        }
    }
    add(
        report,
        "Type-specific Details",
        "Office package",
        "ZIP-based OOXML / OpenDocument",
    );
}

fn xml_element_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = xml.find(&open)?;
    let content_start = xml[start..].find('>')? + start + 1;
    let close = format!("</{tag}>");
    let content_end = xml[content_start..].find(&close)? + content_start;
    let text = &xml[content_start..content_end];
    Some(
        text.replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'"),
    )
}

fn is_text_extension(extension: &str) -> bool {
    matches!(
        extension,
        "txt"
            | "md"
            | "markdown"
            | "csv"
            | "tsv"
            | "log"
            | "json"
            | "xml"
            | "html"
            | "htm"
            | "yaml"
            | "yml"
            | "toml"
            | "ini"
            | "conf"
            | "rs"
            | "c"
            | "h"
            | "cpp"
            | "py"
            | "js"
            | "ts"
            | "sh"
            | "css"
            | "sql"
            | "go"
            | "java"
            | "kt"
            | "swift"
            | "rb"
            | "php"
            | "lua"
            | "nix"
            | "tex"
            | "rst"
    )
}

fn inspect_text(path: &Path, report: &mut MetadataReport, cancel: &AtomicBool) {
    let Ok(file) = fs::File::open(path) else {
        return;
    };
    let Ok(metadata) = file.metadata() else {
        return;
    };
    if metadata.len() > MAX_TEXT_BYTES {
        report.warnings.push(format!(
            "Text statistics are unavailable for files larger than {} MiB.",
            MAX_TEXT_BYTES / (1024 * 1024)
        ));
        return;
    }
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    if BufReader::new(file).read_to_end(&mut bytes).is_err() {
        report
            .warnings
            .push("Could not read text for document statistics.".to_owned());
        return;
    }
    let (text, encoding) = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        (
            String::from_utf8_lossy(&bytes[3..]).into_owned(),
            "UTF-8 BOM",
        )
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        (
            String::from_utf16_lossy(
                &bytes[2..]
                    .chunks_exact(2)
                    .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                    .collect::<Vec<_>>(),
            ),
            "UTF-16 LE",
        )
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        (
            String::from_utf16_lossy(
                &bytes[2..]
                    .chunks_exact(2)
                    .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                    .collect::<Vec<_>>(),
            ),
            "UTF-16 BE",
        )
    } else if let Ok(text) = String::from_utf8(bytes) {
        (text, "UTF-8")
    } else {
        report.warnings.push(
            "Text statistics are unavailable because the encoding is unsupported.".to_owned(),
        );
        return;
    };
    add(report, "Type-specific Details", "Encoding", encoding);
    add(
        report,
        "Type-specific Details",
        "Line count",
        &text.lines().count().to_string(),
    );
    add(
        report,
        "Type-specific Details",
        "Word count",
        &text.split_whitespace().count().to_string(),
    );
    add(
        report,
        "Type-specific Details",
        "Character count",
        &text.chars().count().to_string(),
    );
    if let Some(language) = detect_language(&text) {
        add(
            report,
            "Type-specific Details",
            "Language (heuristic)",
            language,
        );
    }
}

fn detect_language(text: &str) -> Option<&'static str> {
    let sample = text.chars().take(10_000).collect::<String>().to_lowercase();
    let counts = [
        ("English", [" the ", " and ", " of ", " to "]),
        ("Spanish", [" el ", " la ", " de ", " que "]),
        ("French", [" le ", " la ", " de ", " et "]),
        ("German", [" der ", " die ", " und ", " ist "]),
    ]
    .map(|(language, tokens)| {
        (
            language,
            tokens
                .iter()
                .map(|token| sample.matches(token).count())
                .sum::<usize>(),
        )
    });
    let (language, count) = counts.into_iter().max_by_key(|(_, count)| *count)?;
    (count >= 2).then_some(language)
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0usize;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

fn ratio(compressed: u64, uncompressed: u64) -> String {
    if uncompressed == 0 {
        "—".to_owned()
    } else {
        format!("{:.1}%", compressed as f64 * 100.0 / uncompressed as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::Command;

    #[test]
    fn detects_text_statistics_without_using_filename_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.unknown");
        fs::write(&path, "The quick brown fox and the dog.\nSecond line.").unwrap();
        let report = extract_uncached(&path, &AtomicBool::new(false)).unwrap();
        assert!(report.mime_type.starts_with("text/"));
        assert!(report
            .fields
            .iter()
            .any(|field| field.label == "Line count"));
        assert!(report
            .fields
            .iter()
            .any(|field| field.label == "Word count"));
    }

    #[test]
    fn xml_text_decodes_common_entities() {
        assert_eq!(
            xml_element_text("<dc:title>A &amp; B</dc:title>", "dc:title"),
            Some("A & B".to_owned())
        );
    }

    #[test]
    fn byte_ratio_is_safe_for_empty_archives() {
        assert_eq!(ratio(0, 0), "—");
        assert_eq!(ratio(50, 100), "50.0%");
        assert_eq!(format_bytes(1024), "1.0 KiB");
    }

    #[test]
    fn frame_rates_are_converted_and_invalid_values_are_omitted() {
        assert_eq!(frame_rate("30000/1001"), Some("29.970 fps".to_owned()));
        assert_eq!(frame_rate("0/0"), None);
        assert_eq!(frame_rate("invalid"), None);
    }

    #[test]
    fn extensionless_zip_is_detected_and_summarized() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive");
        let file = fs::File::create(&path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file("notes.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"archive contents").unwrap();
        archive.finish().unwrap();

        let report = extract_uncached(&path, &AtomicBool::new(false)).unwrap();
        assert!(report
            .fields
            .iter()
            .any(|field| { field.label == "Archive format" && field.value == "ZIP" }));
        assert!(report
            .fields
            .iter()
            .any(|field| field.label == "Entry count" && field.value == "1"));
    }

    #[test]
    fn extracts_real_seven_zip_archive_listing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.txt"), "archive contents").unwrap();
        let archive = dir.path().join("sample.7z");
        let status = Command::new("7z")
            .current_dir(dir.path())
            .args(["a", "-y"])
            .arg(&archive)
            .arg("note.txt")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("7z is required in the development shell");
        assert!(status.success());

        let report = extract_uncached(&archive, &AtomicBool::new(false)).unwrap();
        assert!(report
            .fields
            .iter()
            .any(|field| { field.label == "Entry count" && field.value == "1" }));
        assert!(report
            .fields
            .iter()
            .any(|field| field.label == "Entry" && field.value == "note.txt"));
    }

    #[test]
    fn seven_zip_listing_totals_all_entries() {
        let mut report = MetadataReport {
            actual_type: "7-zip archive data".to_owned(),
            ..MetadataReport::default()
        };
        inspect_7z_listing(
            "Path = archive.7z\nType = 7z\nPhysical Size = 30\n\n----------\nPath = a.txt\nSize = 10\nPacked Size = 4\nEncrypted = -\n\nPath = b.txt\nSize = 20\nPacked Size = 8\nEncrypted = +\n",
            &mut report,
        );
        assert!(report
            .fields
            .iter()
            .any(|field| field.label == "Entry count" && field.value == "2"));
        assert!(report
            .fields
            .iter()
            .any(|field| field.label == "Uncompressed size" && field.value == "30.0 B"));
        assert!(report
            .fields
            .iter()
            .any(|field| { field.label == "Encrypted entries" && field.value == "Yes" }));
    }

    #[test]
    fn public_extraction_detects_file_type_and_reuses_cached_results() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unicode-文件");
        fs::write(&path, "The file has two lines.\nAnd a second line.").unwrap();

        let cancel = AtomicBool::new(false);
        let job = begin_job(&cancel).unwrap();
        let first = extract_with_job(&path, &cancel, &job).unwrap();
        let second = extract_with_job(&path, &cancel, &job).unwrap();
        assert_eq!(first.mime_type, second.mime_type);
        assert!(first.mime_type.starts_with("text/"));
        assert!(first
            .fields
            .iter()
            .any(|field| field.label == "Line count" && field.value == "2"));
    }

    #[test]
    fn ffprobe_reports_streams_and_embedded_artwork_metadata() {
        let input = serde_json::json!({
            "format": {
                "format_name": "mp4",
                "duration": "12.5",
                "tags": {"title": "Sample"}
            },
            "streams": [{
                "index": 0,
                "codec_type": "video",
                "codec_name": "h264",
                "width": 640,
                "height": 360,
                "avg_frame_rate": "30000/1001",
                "disposition": {"attached_pic": 1}
            }]
        });
        let mut report = MetadataReport::default();
        inspect_ffprobe_json(&input, &mut report);
        assert!(report
            .fields
            .iter()
            .any(|field| { field.label == "Average frame rate" && field.value == "29.970 fps" }));
        assert!(report
            .fields
            .iter()
            .any(|field| field.label == "Embedded artwork" && field.value == "Yes"));
        assert!(report.fields.iter().any(|field| {
            field.section == "Video track 1" && field.label == "Width" && field.value == "640"
        }));
    }

    #[test]
    fn extracts_real_image_and_media_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("sample.png");
        let image_status = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=64x48",
                "-frames:v",
                "1",
                "-y",
            ])
            .arg(&image)
            .status()
            .expect("ffmpeg is required in the development shell");
        assert!(image_status.success());
        let image_report = extract_uncached(&image, &AtomicBool::new(false)).unwrap();
        assert!(image_report.mime_type.starts_with("image/"));
        assert!(image_report
            .fields
            .iter()
            .any(|field| { field.label.ends_with("ImageWidth") && field.value == "64" }));

        let media = dir.path().join("sample.mp4");
        let media_status = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=64x48:d=0.5",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:duration=0.5",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
                "-shortest",
                "-metadata",
                "title=Filem metadata fixture",
                "-y",
            ])
            .arg(&media)
            .status()
            .expect("ffmpeg is required in the development shell");
        assert!(media_status.success());
        let media_report = extract_uncached(&media, &AtomicBool::new(false)).unwrap();
        assert!(media_report
            .fields
            .iter()
            .any(|field| { field.label == "Codec" && field.value == "mpeg4" }));
        assert!(media_report
            .fields
            .iter()
            .any(|field| { field.label == "Width" && field.value == "64" }));
        assert!(media_report
            .fields
            .iter()
            .any(|field| field.value == "Filem metadata fixture"));
    }

    #[test]
    fn oversized_text_is_not_loaded_for_statistics() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.txt");
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_TEXT_BYTES + 1)
            .unwrap();
        let mut report = MetadataReport::default();
        inspect_text(&path, &mut report, &AtomicBool::new(false));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("larger than")));
        assert!(!report
            .fields
            .iter()
            .any(|field| field.label == "Word count"));
    }

    #[test]
    fn metadata_cache_invalidates_on_size_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.txt");
        fs::write(&path, "one").unwrap();
        let first = cache_key(&path).unwrap();
        std::thread::sleep(Duration::from_millis(2));
        fs::write(&path, "longer").unwrap();
        let second = cache_key(&path).unwrap();
        assert_ne!(first, second);
    }
}
