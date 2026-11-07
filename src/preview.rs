//! Quick Look preview data. Everything here is read-only and bounded; a
//! preview never executes a file and never loads an unbounded amount of data.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, UNIX_EPOCH};

use crate::ops::{archive_format, archive_list};
use crate::paths::{format_size, path_to_file_url};

const MAX_TEXT_BYTES: u64 = 128 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 250;

#[derive(Debug, Clone, Default)]
pub struct PreviewData {
    pub title: String,
    pub kind: String,
    pub subtitle: String,
    pub text: String,
    pub image_url: String,
    pub archive: Vec<String>,
    pub note: String,
    pub size_text: String,
    pub modified_ms: f64,
    pub is_dir: bool,
    pub path: String,
    pub key: String,
    pub page_count: usize,
    pub page_number: usize,
}

fn is_text_extension(extension: &str) -> bool {
    matches!(
        extension,
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
            | "mjs"
            | "ts"
            | "tsx"
            | "jsx"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cc"
            | "cxx"
            | "hh"
            | "hxx"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "css"
            | "html"
            | "htm"
            | "xml"
            | "sql"
            | "conf"
            | "ini"
            | "cfg"
            | "csv"
            | "desktop"
            | "service"
            | "patch"
            | "diff"
            | "lock"
            | "env"
            | "gitignore"
            | "go"
            | "java"
            | "kt"
            | "kts"
            | "scala"
            | "swift"
            | "php"
            | "rb"
            | "lua"
            | "pl"
            | "pm"
            | "r"
            | "dart"
            | "ex"
            | "exs"
            | "erl"
            | "hrl"
            | "hs"
            | "lhs"
            | "elm"
            | "clj"
            | "cljs"
            | "cljc"
            | "edn"
            | "vue"
            | "svelte"
            | "astro"
            | "graphql"
            | "gql"
            | "proto"
            | "make"
            | "mk"
            | "cmake"
            | "dockerfile"
            | "properties"
            | "out"
            | "err"
            | "trace"
            | "journal"
    )
}

fn is_image_extension(extension: &str) -> bool {
    matches!(
        extension,
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
            | "heic"
            | "heif"
            | "jxl"
            | "tga"
            | "jp2"
            | "j2k"
    )
}

fn is_raster_image_extension(extension: &str) -> bool {
    matches!(
        extension,
        "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "gif"
            | "bmp"
            | "tif"
            | "tiff"
            | "avif"
            | "ico"
            | "heic"
            | "heif"
            | "jxl"
            | "tga"
            | "jp2"
            | "j2k"
    )
}

pub(crate) fn is_video_extension(extension: &str) -> bool {
    matches!(
        extension,
        "mp4"
            | "mkv"
            | "webm"
            | "avi"
            | "mov"
            | "m4v"
            | "mpg"
            | "mpeg"
            | "ogv"
            | "flv"
            | "wmv"
            | "3gp"
            | "3g2"
            | "ts"
            | "mts"
            | "m2ts"
            | "vob"
    )
}

const THUMBNAIL_CACHE_SIZES: [&str; 4] = ["normal", "large", "x-large", "xx-large"];
const THUMBNAIL_GENERATION_SIZE: &str = "large";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThumbnailSourceInfo {
    uri: String,
    mtime: u64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
    size: u64,
    device: u64,
    inode: u64,
}

fn thumbnail_uri(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    path_to_file_url(&absolute)
}

fn thumbnail_source_info(source: &Path) -> std::io::Result<ThumbnailSourceInfo> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::metadata(source)?;
    Ok(ThumbnailSourceInfo {
        uri: thumbnail_uri(source),
        mtime: metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        mtime_nsec: metadata.mtime_nsec(),
        ctime: metadata.ctime(),
        ctime_nsec: metadata.ctime_nsec(),
        size: metadata.size(),
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

fn thumbnail_cache_key(path: &Path) -> String {
    let uri = thumbnail_uri(path);
    format!("{:x}", md5::compute(uri.as_bytes()))
}

fn thumbnail_path_in(cache_dir: &Path, path: &Path, size: &str) -> PathBuf {
    cache_dir
        .join("thumbnails")
        .join(size)
        .join(format!("{}.png", thumbnail_cache_key(path)))
}

fn thumbnail_fail_path_in(cache_dir: &Path, path: &Path) -> PathBuf {
    cache_dir
        .join("thumbnails/fail/filem")
        .join(format!("{}.png", thumbnail_cache_key(path)))
}

fn cached_thumbnail_in(cache_dir: &Path, source: &Path) -> Option<PathBuf> {
    let source_info = thumbnail_source_info(source).ok()?;
    THUMBNAIL_CACHE_SIZES
        .iter()
        .map(|size| thumbnail_path_in(cache_dir, source, size))
        .find(|thumbnail| thumbnail_cache_valid_for(thumbnail, &source_info))
}

pub fn cached_thumbnail_path(source: &Path) -> Option<PathBuf> {
    cached_thumbnail_in(&crate::paths::cache_dir(), source)
}

fn thumbnail_cache_valid(thumbnail: &Path, source: &Path) -> bool {
    thumbnail_source_info(source).is_ok_and(|info| thumbnail_cache_valid_for(thumbnail, &info))
}

fn thumbnail_cache_valid_for(thumbnail: &Path, source: &ThumbnailSourceInfo) -> bool {
    let Some(text) = read_thumbnail_text_chunks(thumbnail) else {
        return false;
    };
    let Some(uri) = text.get("Thumb::URI") else {
        return false;
    };
    let Some(mtime) = text
        .get("Thumb::MTime")
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return false;
    };
    let Some(size) = text
        .get("Thumb::Size")
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return false;
    };
    if uri != &source.uri || mtime != source.mtime || size != source.size {
        return false;
    }

    let standard_metadata_matches = [
        ("Filem::MTimeNsec", source.mtime_nsec),
        ("Filem::CTime", source.ctime),
        ("Filem::CTimeNsec", source.ctime_nsec),
    ]
    .iter()
    .all(|(key, expected)| {
        text.get(*key)
            .is_none_or(|value| value.parse::<i64>().ok() == Some(*expected))
    });
    standard_metadata_matches
        && text
            .get("Filem::Device")
            .is_none_or(|value| value.parse::<u64>().ok() == Some(source.device))
        && text
            .get("Filem::Inode")
            .is_none_or(|value| value.parse::<u64>().ok() == Some(source.inode))
}

fn read_thumbnail_text_chunks(thumbnail: &Path) -> Option<HashMap<String, String>> {
    let mut file = fs::File::open(thumbnail).ok()?;
    let file_size = file.metadata().ok()?.len();
    let mut signature = [0; 8];
    file.read_exact(&mut signature).ok()?;
    if &signature != b"\x89PNG\r\n\x1a\n" {
        return None;
    }

    let mut text = HashMap::new();
    let mut saw_ihdr = false;
    let mut saw_idat = false;
    loop {
        let mut header = [0; 8];
        file.read_exact(&mut header).ok()?;
        let length = u32::from_be_bytes(header[..4].try_into().ok()?) as u64;
        let chunk_type = &header[4..8];
        let chunk_start = file.stream_position().ok()?;
        if length > file_size.saturating_sub(chunk_start).saturating_sub(4) {
            return None;
        }

        if chunk_type == b"IHDR" {
            if saw_ihdr || length != 13 {
                return None;
            }
            saw_ihdr = true;
        } else if chunk_type == b"IDAT" {
            if !saw_ihdr {
                return None;
            }
            saw_idat = true;
        } else if chunk_type == b"tEXt" {
            if length > 64 * 1024 {
                return None;
            }
            let mut data = vec![0; length as usize];
            file.read_exact(&mut data).ok()?;
            let mut crc = [0; 4];
            file.read_exact(&mut crc).ok()?;
            if u32::from_be_bytes(crc) != crc32_png(chunk_type, &data) {
                return None;
            }
            if let Some(separator) = data.iter().position(|byte| *byte == 0) {
                let key = std::str::from_utf8(&data[..separator]).ok()?.to_owned();
                let value = std::str::from_utf8(&data[separator + 1..]).ok()?.to_owned();
                text.insert(key, value);
            }
            continue;
        } else if chunk_type == b"IEND" && (length != 0 || !saw_ihdr || !saw_idat) {
            return None;
        }

        if chunk_type != b"tEXt" {
            file.seek(SeekFrom::Current(length as i64)).ok()?;
            let mut expected_crc = [0; 4];
            file.read_exact(&mut expected_crc).ok()?;
            if chunk_type == b"IEND" && u32::from_be_bytes(expected_crc) != crc32_png(b"IEND", b"")
            {
                return None;
            }
        }
        if chunk_type == b"IEND" {
            return (file.stream_position().ok()? == file_size).then_some(text);
        }
    }
}

/// Embed Freedesktop metadata before the generated thumbnail is atomically
/// installed in the shared cache.
fn embed_thumbnail_metadata(thumbnail: &Path, source: &ThumbnailSourceInfo) -> Result<(), String> {
    let mut bytes = fs::read(thumbnail)
        .map_err(|error| format!("Could not read generated thumbnail metadata: {error}"))?;
    if bytes.len() < 12 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("Thumbnail generator did not produce a valid PNG.".to_owned());
    }

    // Strip the trailing IEND chunk (always 12 bytes: len=0, type=IEND, crc).
    let iend_marker = b"IEND";
    let tail_start = bytes.len() - 12;
    if &bytes[tail_start + 4..tail_start + 8] != iend_marker {
        return Err("Generated thumbnail is missing its PNG end marker.".to_owned());
    }
    bytes.truncate(tail_start);

    // Build tEXt chunks for the three Freedesktop metadata keys.
    let pairs = [
        ("Thumb::URI", source.uri.clone()),
        ("Thumb::MTime", source.mtime.to_string()),
        ("Thumb::Size", source.size.to_string()),
        ("Filem::MTimeNsec", source.mtime_nsec.to_string()),
        ("Filem::CTime", source.ctime.to_string()),
        ("Filem::CTimeNsec", source.ctime_nsec.to_string()),
        ("Filem::Device", source.device.to_string()),
        ("Filem::Inode", source.inode.to_string()),
    ];
    for (key, value) in &pairs {
        let data: Vec<u8> = key
            .bytes()
            .chain(std::iter::once(0u8))
            .chain(value.bytes())
            .collect();
        let chunk_len = (data.len() as u32).to_be_bytes();
        let chunk_type = b"tEXt";
        // CRC covers type + data.
        let crc = crc32_png(chunk_type, &data);
        bytes.extend_from_slice(&chunk_len);
        bytes.extend_from_slice(chunk_type);
        bytes.extend_from_slice(&data);
        bytes.extend_from_slice(&crc.to_be_bytes());
    }

    // Re-append a valid IEND chunk.
    // IEND: length=0, type="IEND", crc of just the type bytes.
    let iend_crc = crc32_png(b"IEND", b"");
    bytes.extend_from_slice(&[0, 0, 0, 0]); // length = 0
    bytes.extend_from_slice(b"IEND");
    bytes.extend_from_slice(&iend_crc.to_be_bytes());

    fs::write(thumbnail, &bytes)
        .map_err(|error| format!("Could not write thumbnail metadata: {error}"))
}

/// Minimal CRC-32 for PNG chunks (polynomial 0xEDB88320, ISO 3309).
fn crc32_png(chunk_type: &[u8], data: &[u8]) -> u32 {
    const POLY: u32 = 0xEDB88320;
    let mut crc = 0xFFFFFFFF;
    for &byte in chunk_type.iter().chain(data) {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ POLY;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

pub(crate) fn is_audio_extension(extension: &str) -> bool {
    matches!(
        extension,
        "mp3"
            | "flac"
            | "ogg"
            | "oga"
            | "opus"
            | "m4a"
            | "m4b"
            | "wav"
            | "wave"
            | "aac"
            | "wma"
            | "aif"
            | "aiff"
            | "ape"
            | "wv"
            | "3ga"
    )
}

fn generate_thumbnail(
    path: &Path,
    target: &Path,
    extension: &str,
    source_info: &ThumbnailSourceInfo,
) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create thumbnail cache {}: {error}",
                parent.display()
            )
        })?;
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_prefix = target.with_file_name(format!(".{:x}.{stamp}", std::process::id()));

    let generated_path = std::path::PathBuf::from(format!("{}.png", temp_prefix.display()));

    let (program, args) = if extension == "pdf" {
        (
            "pdftoppm",
            vec![
                "-f".to_owned(),
                "1".to_owned(),
                "-l".to_owned(),
                "1".to_owned(),
                "-scale-to".to_owned(),
                "256".to_owned(),
                "-singlefile".to_owned(),
                "-png".to_owned(),
                path.to_string_lossy().into_owned(),
                temp_prefix.to_string_lossy().into_owned(),
            ],
        )
    } else if is_audio_extension(extension) {
        (
            "ffmpeg",
            vec![
                "-v".to_owned(),
                "error".to_owned(),
                "-threads".to_owned(),
                "1".to_owned(),
                "-i".to_owned(),
                path.to_string_lossy().into_owned(),
                "-map".to_owned(),
                "0:v:0".to_owned(),
                "-an".to_owned(),
                "-vframes".to_owned(),
                "1".to_owned(),
                "-vf".to_owned(),
                "scale=256:256:force_original_aspect_ratio=decrease".to_owned(),
                "-y".to_owned(),
                generated_path.to_string_lossy().into_owned(),
            ],
        )
    } else if is_raster_image_extension(extension) {
        (
            "ffmpeg",
            vec![
                "-v".to_owned(),
                "error".to_owned(),
                "-threads".to_owned(),
                "1".to_owned(),
                "-i".to_owned(),
                path.to_string_lossy().into_owned(),
                "-frames:v".to_owned(),
                "1".to_owned(),
                "-vf".to_owned(),
                "scale=256:256:force_original_aspect_ratio=decrease".to_owned(),
                "-y".to_owned(),
                generated_path.to_string_lossy().into_owned(),
            ],
        )
    } else {
        // Video: seek to 1 second so we get a real frame, not a black opener.
        (
            "ffmpeg",
            vec![
                "-v".to_owned(),
                "error".to_owned(),
                "-threads".to_owned(),
                "1".to_owned(),
                "-ss".to_owned(),
                "1".to_owned(),
                "-i".to_owned(),
                path.to_string_lossy().into_owned(),
                "-frames:v".to_owned(),
                "1".to_owned(),
                "-vf".to_owned(),
                "scale=256:256:force_original_aspect_ratio=decrease".to_owned(),
                "-y".to_owned(),
                generated_path.to_string_lossy().into_owned(),
            ],
        )
    };

    let mut attempts = vec![args.clone()];
    if is_video_extension(extension) {
        if let Some(seek_index) = args.iter().position(|argument| argument == "-ss") {
            let mut first_frame_args = args;
            first_frame_args[seek_index + 1] = "0".to_owned();
            attempts.push(first_frame_args);
        }
    }

    let mut generation_error = String::new();
    let mut generated = false;
    for attempt in attempts {
        let output = Command::new(program)
            .args(attempt)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| {
                let _ = fs::remove_file(&generated_path);
                format!("Could not start {program}: {error}")
            })?;
        if output.status.success() && generated_path.is_file() {
            generated = true;
            break;
        }
        let _ = fs::remove_file(&generated_path);
        generation_error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    }

    if !generated {
        return Err(format!(
            "{program} could not create a thumbnail for {}{}.",
            path.display(),
            if generation_error.is_empty() {
                String::new()
            } else {
                format!(": {generation_error}")
            }
        ));
    }

    if thumbnail_source_info(path).ok().as_ref() != Some(source_info) {
        let _ = fs::remove_file(&generated_path);
        return Err(format!(
            "{} changed while its thumbnail was being generated.",
            path.display()
        ));
    }

    if let Err(error) = embed_thumbnail_metadata(&generated_path, source_info) {
        let _ = fs::remove_file(&generated_path);
        return Err(error);
    }
    fs::rename(&generated_path, target).map_err(|error| {
        let _ = fs::remove_file(&generated_path);
        format!("Could not cache thumbnail {}: {error}", target.display())
    })?;

    if thumbnail_source_info(path).ok().as_ref() != Some(source_info) {
        let _ = fs::remove_file(target);
        return Err(format!(
            "{} changed while its thumbnail was being installed.",
            path.display()
        ));
    }

    Ok(())
}

static THUMBNAIL_GENERATION_LOCKS: OnceLock<Vec<Mutex<()>>> = OnceLock::new();

fn thumbnail_generation_lock(path: &Path) -> &'static Mutex<()> {
    let locks = THUMBNAIL_GENERATION_LOCKS
        .get_or_init(|| (0..64).map(|_| Mutex::new(())).collect::<Vec<_>>());
    let digest = md5::compute(thumbnail_uri(path).as_bytes());
    &locks[digest.0[0] as usize % locks.len()]
}

pub fn request_thumbnail(path: &Path) -> Result<std::path::PathBuf, String> {
    let _guard = thumbnail_generation_lock(path)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    request_thumbnail_in(path, &crate::paths::cache_dir())
}

fn request_thumbnail_in(path: &Path, cache_dir: &Path) -> Result<PathBuf, String> {
    request_thumbnail_in_with(path, cache_dir, generate_thumbnail)
}

fn request_thumbnail_in_with(
    path: &Path,
    cache_dir: &Path,
    generate: impl FnOnce(&Path, &Path, &str, &ThumbnailSourceInfo) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let source_info = thumbnail_source_info(path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // Skip extensions we cannot and will not thumbnail.
    // Audio without artwork is failure-cached below; non-thumbnailable formats
    // return an error immediately without writing a failure marker.
    let can_thumbnail = extension == "pdf"
        || is_raster_image_extension(&extension)
        || is_audio_extension(&extension)
        || is_video_extension(&extension);
    if !can_thumbnail {
        return Err(format!("No thumbnailer for .{extension}"));
    }

    let fail = thumbnail_fail_path_in(cache_dir, path);
    if fail.is_file() {
        if thumbnail_cache_valid_for(&fail, &source_info) {
            return Err(format!(
                "Previously failed to thumbnail {}.",
                path.display()
            ));
        }
        let _ = fs::remove_file(&fail);
    }

    if let Some(cached) = cached_thumbnail_in(cache_dir, path) {
        if thumbnail_source_info(path).ok().as_ref() == Some(&source_info) {
            return Ok(cached);
        }
    }
    for size in THUMBNAIL_CACHE_SIZES {
        let stale = thumbnail_path_in(cache_dir, path, size);
        if stale.is_file() && !thumbnail_cache_valid_for(&stale, &source_info) {
            let _ = fs::remove_file(stale);
        }
    }

    let target = thumbnail_path_in(cache_dir, path, THUMBNAIL_GENERATION_SIZE);
    match generate(path, &target, &extension, &source_info) {
        Ok(()) => {
            enforce_thumbnail_cache_limit(cache_dir);
            Ok(target)
        }
        Err(error) => {
            if thumbnail_source_info(path).ok().as_ref() == Some(&source_info) {
                write_thumbnail_failure(cache_dir, &fail, &source_info);
            }
            Err(error)
        }
    }
}

const THUMBNAIL_CACHE_SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

fn filem_thumbnail_files(cache_dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut files = Vec::new();
    let thumbnails = cache_dir.join("thumbnails");
    for size in THUMBNAIL_CACHE_SIZES {
        let directory = thumbnails.join(size);
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension().is_none_or(|extension| extension != "png") {
                continue;
            }
            let Some(chunks) = read_thumbnail_text_chunks(&path) else {
                continue;
            };
            if !chunks.contains_key("Filem::Device") || !chunks.contains_key("Filem::Inode") {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            files.push((
                path,
                metadata.len(),
                metadata.modified().unwrap_or(UNIX_EPOCH),
            ));
        }
    }
    files
}

pub fn enforce_thumbnail_cache_limit(cache_dir: &Path) {
    trim_thumbnail_cache(cache_dir, false);
}

pub fn trim_thumbnail_cache(cache_dir: &Path, force: bool) {
    static LAST_SWEEP: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    let last_sweep = LAST_SWEEP.get_or_init(|| Mutex::new(None));
    let Ok(mut last) = last_sweep.lock() else {
        return;
    };
    if !force && last.is_some_and(|time| time.elapsed() < THUMBNAIL_CACHE_SWEEP_INTERVAL) {
        return;
    }
    *last = Some(Instant::now());
    drop(last);

    let limit_mb = crate::lifecycle::performance_config().thumbnail_cache_limit_mb;
    let limit_bytes = u64::from(limit_mb.clamp(64, 4096)) * 1024 * 1024;
    enforce_thumbnail_cache_limit_to(cache_dir, limit_bytes);
}

fn enforce_thumbnail_cache_limit_to(cache_dir: &Path, limit_bytes: u64) {
    let mut files = filem_thumbnail_files(cache_dir);
    let mut total = files.iter().map(|(_, size, _)| *size).sum::<u64>();
    if total <= limit_bytes {
        return;
    }
    files.sort_by_key(|(_, _, accessed)| *accessed);
    for (path, size, _) in files {
        if total <= limit_bytes {
            break;
        }
        if fs::remove_file(path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

pub fn clear_filem_thumbnail_cache(cache_dir: &Path) -> Result<(), String> {
    let mut paths = Vec::new();
    for (path, _, _) in filem_thumbnail_files(cache_dir) {
        paths.push(path);
    }
    let failure_dir = cache_dir.join("thumbnails/fail/filem");
    if let Ok(entries) = fs::read_dir(&failure_dir) {
        paths.extend(entries.flatten().map(|entry| entry.path()));
    }
    for path in paths {
        fs::remove_file(&path)
            .map_err(|error| format!("Could not remove {}: {error}", path.display()))?;
    }
    Ok(())
}

fn write_thumbnail_failure(cache_dir: &Path, fail_path: &Path, source_info: &ThumbnailSourceInfo) {
    if let Some(parent) = fail_path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    #[rustfmt::skip]
    let minimal_png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR length + type
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // 8-bit RGB, CRC
        0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, // IDAT length + type
        0x54, 0x78, 0x9C, 0x63, 0x60, 0x60, 0x60, 0x00, // IDAT data
        0x00, 0x00, 0x04, 0x00, 0x01, 0xF6, 0x17, 0x38, // IDAT data + CRC
        0x55, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, // IEND length + type
        0x44, 0xAE, 0x42, 0x60, 0x82,                   // IEND CRC
    ];
    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = cache_dir
        .join("thumbnails/fail/filem")
        .join(format!(".{:x}.{stamp}.tmp.png", std::process::id()));
    if fs::write(&temporary, minimal_png).is_ok() {
        if embed_thumbnail_metadata(&temporary, source_info).is_ok()
            && fs::rename(&temporary, fail_path).is_ok()
        {
            return;
        } else {
            let _ = fs::remove_file(temporary);
        }
    }
}

fn pdf_page_count(path: &Path) -> Result<usize, String> {
    static COUNTS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    let key = thumbnail_cache_key(path);
    let cache = COUNTS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(counts) = cache.lock() {
        if let Some(count) = counts.get(&key) {
            return Ok(*count);
        }
    }
    let output = Command::new("pdfinfo")
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("Could not inspect PDF pages: {error}"))?;
    if !output.status.success() {
        return Err("Could not inspect this PDF.".to_owned());
    }
    let count = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            line.strip_prefix("Pages:")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .filter(|count| *count > 0)
        .ok_or_else(|| "Could not determine the number of PDF pages.".to_owned())?;
    if let Ok(mut counts) = cache.lock() {
        if counts.len() >= 128 {
            counts.clear();
        }
        counts.insert(key, count);
    }
    Ok(count)
}

fn pdf_page_path(path: &Path, page: usize, max_dimension: u32) -> std::path::PathBuf {
    let key = thumbnail_cache_key(path);
    let dimension = pdf_render_dimension(max_dimension);
    crate::paths::cache_dir()
        .join("filem/previews")
        .join(format!("{key}-{page}-{dimension}.png"))
}

fn pdf_render_dimension(max_dimension: u32) -> u32 {
    (max_dimension.clamp(512, 2400).div_ceil(256) * 256).min(2400)
}

pub fn render_pdf_page(
    path: &Path,
    page: usize,
    max_dimension: u32,
) -> Result<(std::path::PathBuf, usize), String> {
    let count = pdf_page_count(path)?;
    if page == 0 || page > count {
        return Err(format!("Page {page} is outside this PDF."));
    }
    let dimension = pdf_render_dimension(max_dimension);
    let target = pdf_page_path(path, page, dimension);
    if target.is_file() {
        return Ok((target, count));
    }
    let parent = target
        .parent()
        .ok_or_else(|| "Could not create the PDF preview cache.".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create PDF preview cache: {error}"))?;

    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_prefix = target.with_extension(format!("{}-{stamp}.tmp", std::process::id()));
    let generated = std::path::PathBuf::from(format!("{}.png", temp_prefix.display()));
    let page = page.to_string();
    let dimension = dimension.to_string();
    let output = Command::new("pdftoppm")
        .args([
            "-f",
            &page,
            "-l",
            &page,
            "-scale-to",
            &dimension,
            "-singlefile",
            "-png",
        ])
        .arg(path)
        .arg(&temp_prefix)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("Could not render PDF page: {error}"))?;
    if !output.success() || !generated.is_file() {
        let _ = fs::remove_file(&generated);
        return Err(format!("Could not render PDF page {page}."));
    }
    if let Err(error) = fs::rename(&generated, &target) {
        if !target.is_file() {
            let _ = fs::remove_file(&generated);
            return Err(format!("Could not cache PDF page: {error}"));
        }
        let _ = fs::remove_file(&generated);
    }
    Ok((target, count))
}

pub fn prefetch_pdf_pages(path: &Path, pages: &[usize], max_dimension: u32) {
    static PREFETCH_ACTIVE: AtomicBool = AtomicBool::new(false);
    let mut pages = pages
        .iter()
        .copied()
        .filter(|page| *page > 0)
        .collect::<Vec<_>>();
    if pages.is_empty()
        || PREFETCH_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }
    pages.sort_unstable();
    pages.dedup();
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        for page in pages {
            let _ = render_pdf_page(&path, page, max_dimension);
        }
        PREFETCH_ACTIVE.store(false, Ordering::Release);
    });
}

fn looks_like_text(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut sample = [0u8; 4096];
    let Ok(read) = file.read(&mut sample) else {
        return false;
    };
    !sample[..read].contains(&0)
}

fn truncate_text(mut text: String) -> (String, bool) {
    const MAX_CHARS: usize = 40_000;
    if text.chars().count() <= MAX_CHARS {
        return (text, false);
    }
    let cut = text
        .char_indices()
        .nth(MAX_CHARS)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    text.truncate(cut);
    (text, true)
}

fn read_text(path: &Path) -> (String, String) {
    let Ok(file) = fs::File::open(path) else {
        return (String::new(), "Could not read this file.".to_owned());
    };
    let mut bytes = Vec::new();
    let mut limited = file.take(MAX_TEXT_BYTES);
    if let Err(error) = limited.read_to_end(&mut bytes) {
        return (String::new(), format!("Could not read this file: {error}"));
    }
    let was_cut = bytes.len() as u64 >= MAX_TEXT_BYTES;
    let (text, chars_cut) = truncate_text(String::from_utf8_lossy(&bytes).into_owned());
    let note = if was_cut || chars_cut {
        format!(
            "Showing the first {} of this file.",
            format_size(MAX_TEXT_BYTES)
        )
    } else {
        String::new()
    };
    (text, note)
}

/// Build preview data for one path. This is meant to run on a worker thread.
fn build_with_pdf_page(path: &Path, pdf_page: usize, max_dimension: u32) -> PreviewData {
    let mut preview = PreviewData {
        path: path.to_string_lossy().into_owned(),
        title: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned()),
        ..Default::default()
    };

    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) => {
            preview.kind = "Unavailable".to_owned();
            preview.note = format!("Could not read this item: {error}");
            return preview;
        }
    };
    let symlink_meta = fs::symlink_metadata(path).ok();
    preview.is_dir = meta.is_dir();
    preview.size_text = if preview.is_dir {
        String::new()
    } else {
        format_size(meta.len())
    };
    preview.modified_ms = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as f64)
        .unwrap_or(0.0);
    preview.key = format!(
        "{}:{}:{}",
        path.to_string_lossy(),
        meta.len(),
        preview.modified_ms as u64
    );

    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();
    preview.kind = crate::icons::human_kind(&preview.title, preview.is_dir, &extension);
    preview.subtitle = crate::ops::display_path(path);

    if preview.is_dir {
        let children = fs::read_dir(path)
            .map(|read| read.flatten().count())
            .unwrap_or(0);
        preview.note = format!(
            "Folder with {children} item{}. Press Enter to open it.",
            if children == 1 { "" } else { "s" }
        );
        return preview;
    }

    if let Some(target) = symlink_meta
        .as_ref()
        .filter(|meta| meta.file_type().is_symlink())
        .and_then(|_| fs::read_link(path).ok())
    {
        preview.note = format!("Symbolic link → {}", target.display());
    }

    if is_image_extension(&extension) {
        preview.image_url = path_to_file_url(path);
        return preview;
    }

    if archive_format(path).is_some() {
        match archive_list(path, MAX_ARCHIVE_ENTRIES + 1) {
            Ok(mut entries) => {
                if entries.len() > MAX_ARCHIVE_ENTRIES {
                    entries.truncate(MAX_ARCHIVE_ENTRIES);
                    preview.note =
                        format!("Showing the first {MAX_ARCHIVE_ENTRIES} archive entries.");
                } else {
                    preview.note = format!(
                        "{} entr{}",
                        entries.len(),
                        if entries.len() == 1 { "y" } else { "ies" }
                    );
                }
                preview.archive = entries;
            }
            Err(error) => preview.note = error,
        }
        return preview;
    }

    if extension == "pdf" {
        match render_pdf_page(path, pdf_page, max_dimension) {
            Ok((thumbnail, page_count)) => {
                preview.image_url = path_to_file_url(&thumbnail);
                preview.page_count = page_count;
                preview.page_number = pdf_page;
            }
            Err(error) => {
                preview.note = format!("Could not preview this PDF: {error}");
            }
        }
        return preview;
    }

    if is_audio_extension(&extension) {
        if let Some(thumbnail) = cached_thumbnail_path(path) {
            preview.image_url = path_to_file_url(&thumbnail);
        }
        preview.note = "Use the playback controls to listen.".to_owned();
        return preview;
    }

    if is_video_extension(&extension) {
        if let Some(thumbnail) = cached_thumbnail_path(path) {
            preview.image_url = path_to_file_url(&thumbnail);
            preview.note = "Video preview frame.".to_owned();
        } else {
            preview.note = "A video preview is being generated.".to_owned();
        }
        return preview;
    }

    if is_text_extension(&extension)
        || (preview.size_text != "0 B" && meta.len() <= MAX_TEXT_BYTES && looks_like_text(path))
    {
        let (text, note) = read_text(path);
        preview.text = text;
        if !note.is_empty() {
            preview.note = note;
        }
        return preview;
    }

    preview.note = "No preview is available for this file type. Press Enter to open it.".to_owned();
    preview
}

pub fn build_pdf_page(path: &Path, page: usize, max_dimension: u32) -> PreviewData {
    build_with_pdf_page(path, page, max_dimension)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const TEST_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x60,
        0x60, 0x60, 0x00, 0x00, 0x00, 0x04, 0x00, 0x01, 0xF6, 0x17, 0x38, 0x55, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn write_cached_png(cache_dir: &Path, source: &Path) -> PathBuf {
        let source_info = thumbnail_source_info(source).unwrap();
        let thumbnail = thumbnail_path_in(cache_dir, source, THUMBNAIL_GENERATION_SIZE);
        fs::create_dir_all(thumbnail.parent().unwrap()).unwrap();
        fs::write(&thumbnail, TEST_PNG).unwrap();
        embed_thumbnail_metadata(&thumbnail, &source_info).unwrap();
        thumbnail
    }

    fn png_dimensions(path: &Path) -> (u32, u32) {
        let bytes = fs::read(path).unwrap();
        assert!(bytes.len() >= 24);
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        (
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
        )
    }

    fn write_embedded_art_audio(source: &Path, artwork: &Path, audio_codec: &str, id3: bool) {
        let mut command = Command::new("ffmpeg");
        command.args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-i",
        ]);
        command.arg(artwork).args([
            "-map",
            "0:a:0",
            "-map",
            "1:v:0",
            "-c:a",
            audio_codec,
            "-c:v",
            "copy",
            "-disposition:v:0",
            "attached_pic",
        ]);
        if id3 {
            command.args([
                "-id3v2_version",
                "3",
                "-metadata:s:v",
                "title=Album cover",
                "-metadata:s:v",
                "comment=Cover (front)",
            ]);
        }
        let output = command
            .arg("-y")
            .arg(source)
            .output()
            .expect("ffmpeg is required in the development shell");
        assert!(
            output.status.success(),
            "Could not create {} artwork fixture: {}",
            source.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn assert_audio_artwork_is_cached(source: &Path, cache: &Path) {
        let thumbnail = request_thumbnail_in(source, cache).unwrap();
        assert!(thumbnail_cache_valid(&thumbnail, source));
        let (width, height) = png_dimensions(&thumbnail);
        assert!(width > 0 && height > 0 && width <= 256 && height <= 256);
    }

    #[test]
    fn freedesktop_thumbnail_key_hashes_the_escaped_file_uri() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("photo with spaces.jpg");
        let uri = thumbnail_uri(&source);
        assert!(uri.contains("%20"));
        assert_eq!(
            thumbnail_cache_key(&source),
            format!("{:x}", md5::compute(uri.as_bytes()))
        );
    }

    #[test]
    fn valid_cached_thumbnail_is_reused_without_running_a_thumbnailer() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("broken.jpg");
        let cache = dir.path().join("cache");
        fs::write(&source, b"not actually a JPEG").unwrap();
        let cached = write_cached_png(&cache, &source);
        assert!(thumbnail_cache_valid(&cached, &source));

        let result = request_thumbnail_in_with(&source, &cache, |_, _, _, _| {
            panic!("valid cache hit must not invoke generation")
        })
        .unwrap();
        assert_eq!(result, cached);
    }

    #[test]
    fn cached_thumbnail_is_invalidated_when_source_metadata_changes() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("photo.jpg");
        let cache = dir.path().join("cache");
        fs::write(&source, b"first").unwrap();
        let cached = write_cached_png(&cache, &source);
        assert!(thumbnail_cache_valid(&cached, &source));

        fs::write(&source, b"changed source").unwrap();
        assert!(!thumbnail_cache_valid(&cached, &source));
        assert!(cached_thumbnail_in(&cache, &source).is_none());
    }

    #[test]
    fn thumbnail_limit_and_clear_only_remove_filem_owned_entries() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("photo.jpg");
        let cache = dir.path().join("cache");
        fs::write(&source, b"source").unwrap();
        let owned = write_cached_png(&cache, &source);
        let unrelated = cache
            .join("thumbnails/normal")
            .join("other-application.png");
        fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
        fs::write(&unrelated, TEST_PNG).unwrap();

        enforce_thumbnail_cache_limit_to(&cache, 1);
        assert!(!owned.exists());
        assert!(unrelated.exists());

        let owned = write_cached_png(&cache, &source);
        clear_filem_thumbnail_cache(&cache).unwrap();
        assert!(!owned.exists());
        assert!(unrelated.exists());
    }

    #[test]
    fn corrupted_cached_thumbnail_is_regenerated() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("photo.jpg");
        let cache = dir.path().join("cache");
        fs::write(&source, b"source").unwrap();
        let cached = write_cached_png(&cache, &source);
        let mut bytes = fs::read(&cached).unwrap();
        *bytes.last_mut().unwrap() ^= 0x01;
        fs::write(&cached, bytes).unwrap();
        assert!(cached_thumbnail_in(&cache, &source).is_none());

        let regenerated =
            request_thumbnail_in_with(&source, &cache, |_, target, _, source_info| {
                fs::write(target, TEST_PNG).map_err(|error| error.to_string())?;
                embed_thumbnail_metadata(target, source_info)
            })
            .unwrap();
        assert_eq!(regenerated, cached);
        assert!(thumbnail_cache_valid(&regenerated, &source));
    }

    #[test]
    fn failed_thumbnail_generation_is_cached_for_unchanged_source() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("no-artwork.mp3");
        let cache = dir.path().join("cache");
        fs::write(&source, b"invalid audio").unwrap();
        let attempts = std::cell::Cell::new(0);

        let first = request_thumbnail_in_with(&source, &cache, |_, _, _, _| {
            attempts.set(attempts.get() + 1);
            Err("no embedded artwork".to_owned())
        });
        assert!(first.is_err());
        assert_eq!(attempts.get(), 1);
        let failure = thumbnail_fail_path_in(&cache, &source);
        assert!(thumbnail_cache_valid(&failure, &source));

        let second = request_thumbnail_in_with(&source, &cache, |_, _, _, _| {
            panic!("unchanged source with a failure marker must not be retried")
        });
        assert!(second.unwrap_err().contains("Previously failed"));
    }

    #[test]
    fn extracts_embedded_id3_album_art() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("album.mp3");
        let artwork = dir.path().join("cover.png");
        let cache = dir.path().join("cache");
        fs::write(&artwork, TEST_PNG).unwrap();
        write_embedded_art_audio(&source, &artwork, "libmp3lame", true);

        assert_audio_artwork_is_cached(&source, &cache);
    }

    #[test]
    fn extracts_embedded_flac_picture() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("album.flac");
        let artwork = dir.path().join("cover.png");
        let cache = dir.path().join("cache");
        fs::write(&artwork, TEST_PNG).unwrap();
        write_embedded_art_audio(&source, &artwork, "flac", false);

        assert_audio_artwork_is_cached(&source, &cache);
    }

    #[test]
    fn audio_thumbnailer_recognizes_common_audio_formats() {
        for extension in ["mp3", "flac", "m4a", "aac", "ogg", "opus", "wav", "oga"] {
            assert!(is_audio_extension(extension), "{extension}");
        }
    }

    fn write_test_pdf(path: &Path, page_count: usize) {
        let kids = (0..page_count)
            .map(|index| format!("{} 0 R", index * 2 + 3))
            .collect::<Vec<_>>()
            .join(" ");
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            format!("<< /Type /Pages /Kids [{kids}] /Count {page_count} >>").into_bytes(),
        ];
        for index in 0..page_count {
            let page_id = index * 2 + 3;
            let content_id = page_id + 1;
            objects.push(
                format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Resources << >> /Contents {content_id} 0 R >>")
                    .into_bytes(),
            );
            let drawing = (0..500)
                .map(|line| {
                    let y = line % 400;
                    format!("0 {} m 300 {} l S\n", y, y)
                })
                .collect::<String>();
            objects.push(
                format!(
                    "<< /Length {} >>\nstream\n{}endstream",
                    drawing.len(),
                    drawing
                )
                .into_bytes(),
            );
        }
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0usize];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", offsets.len()).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                offsets.len()
            )
            .as_bytes(),
        );
        fs::write(path, pdf).unwrap();
    }

    #[test]
    fn text_preview_is_bounded_and_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        fs::write(&path, "hello\nworld\n").unwrap();
        let preview = build_with_pdf_page(&path, 1, 1200);
        assert_eq!(preview.text, "hello\nworld\n");
        assert!(preview.image_url.is_empty());
        assert!(!preview.is_dir);
    }

    #[test]
    fn markdown_source_and_log_files_are_read_as_text() {
        let dir = tempfile::tempdir().unwrap();
        for (name, contents) in [
            ("readme.md", "# Heading\n"),
            ("main.go", "package main\n"),
            ("server.log", "started\n"),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, contents).unwrap();
            let preview = build_with_pdf_page(&path, 1, 1200);
            assert_eq!(preview.text, contents, "{name}");
            assert!(preview.note.is_empty(), "{name}: {}", preview.note);
        }
    }

    #[test]
    fn audio_preview_is_playable_in_quick_look() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("track.oga");
        fs::write(&path, b"audio").unwrap();
        let preview = build_with_pdf_page(&path, 1, 1200);
        assert!(preview.text.is_empty());
        assert!(preview.note.contains("playback controls"));
    }

    #[test]
    fn binary_files_are_not_rendered_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blob.bin");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(&[0u8, 1, 2, 3, 0, 9]).unwrap();
        let preview = build_with_pdf_page(&path, 1, 1200);
        assert!(preview.text.is_empty());
        assert!(preview.note.contains("No preview"));
    }

    #[test]
    fn images_get_a_file_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo one.png");
        fs::write(&path, b"not really a png").unwrap();
        let preview = build_with_pdf_page(&path, 1, 1200);
        assert!(preview.image_url.ends_with("photo%20one.png"));
    }

    #[test]
    fn pdf_preview_renders_only_the_first_page() {
        if Command::new("pdftoppm").arg("-v").output().is_err()
            || Command::new("pdfinfo").arg("-v").output().is_err()
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two-pages.pdf");
        write_test_pdf(&path, 2);

        let preview = build_with_pdf_page(&path, 1, 1200);
        assert!(preview.image_url.ends_with(".png"), "{}", preview.note);
        assert_eq!(preview.page_count, 2);
        assert_eq!(preview.page_number, 1);
        let first = pdf_page_path(&path, 1, 1200);
        assert!(preview.image_url.ends_with(".png"));
        assert_eq!(png_dimensions(&first).1, 1280);
        let (second_page, page_count) = render_pdf_page(&path, 2, 1200).unwrap();
        assert_eq!(page_count, 2);
        assert!(second_page.is_file());
        assert!(render_pdf_page(&path, 3, 1200).is_err());
        let cached = render_pdf_page(&path, 2, 1200).unwrap();
        assert_eq!(cached.0, second_page);
        let _ = fs::remove_file(second_page);
        let _ = fs::remove_file(pdf_page_path(&path, 1, 1200));
    }

    #[test]
    fn large_pdf_renders_only_requested_pages_at_bounded_resolution() {
        if Command::new("pdftoppm").arg("-v").output().is_err()
            || Command::new("pdfinfo").arg("-v").output().is_err()
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("many-pages.pdf");
        write_test_pdf(&path, 48);

        let preview = build_pdf_page(&path, 1, 100_000);
        assert_eq!(preview.page_count, 48);
        assert_eq!(preview.page_number, 1);
        let (last, count) = render_pdf_page(&path, 48, 100_000).unwrap();
        assert_eq!(count, 48);
        assert!(last.is_file());
        assert_eq!(png_dimensions(&last).1, 2400);
        assert_eq!(
            pdf_page_path(&path, 48, 100_000),
            pdf_page_path(&path, 48, 2400)
        );
        let _ = fs::remove_file(last);
        let _ = fs::remove_file(pdf_page_path(&path, 1, 100_000));
    }

    #[test]
    fn folder_preview_counts_direct_children() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("box");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("a"), b"1").unwrap();
        fs::write(folder.join("b"), b"2").unwrap();
        let preview = build_with_pdf_page(&folder, 1, 1200);
        assert!(preview.is_dir);
        assert!(preview.note.contains("2 items"), "{}", preview.note);
    }
}
