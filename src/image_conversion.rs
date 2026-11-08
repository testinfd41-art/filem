use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::OnceLock;
use std::time::Duration;

static OUTPUT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static AVAILABLE_FORMATS: OnceLock<Result<Vec<String>, String>> = OnceLock::new();

pub(crate) fn available_formats() -> Result<Vec<String>, String> {
    AVAILABLE_FORMATS
        .get_or_init(load_available_formats)
        .clone()
}

pub(crate) fn is_image_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .is_some_and(|extension| crate::preview::is_image_extension(&extension))
}

pub(crate) fn convert_images(
    sources: &[PathBuf],
    format: &str,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(String, f64),
) -> Result<String, String> {
    if sources.is_empty() {
        return Err("Select at least one image to convert.".to_owned());
    }

    let formats = available_formats()?;
    let format = formats
        .iter()
        .find(|available| available.eq_ignore_ascii_case(format))
        .ok_or_else(|| format!("ImageMagick cannot write the {format} format."))?;
    let extension = format.to_ascii_lowercase();
    let total = sources.len();
    let mut converted = 0usize;

    for (index, source) in sources.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(canceled_message(converted));
        }
        if !is_image_file(source) {
            return Err(format!("Not a supported image file: {}", source.display()));
        }

        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.display().to_string());
        let base_progress = index as f64 / total as f64;
        let progress_span = 1.0 / total as f64;
        progress(format!("Converting {name}"), base_progress);
        let temporary = temporary_path(source, &extension)?;
        let mut image_progress = 0.0f64;
        let result = run_magick(source, &temporary, cancel, &mut |stage, fraction| {
            image_progress = image_progress.max(fraction);
            progress(
                format!("{name} - {stage}"),
                base_progress + progress_span * image_progress,
            );
        });
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(if converted == 0 {
                format!("Could not convert {}: {error}", source.display())
            } else {
                format!(
                    "Converted {converted} image(s) before {} failed: {error}",
                    source.display()
                )
            });
        }
        if !temporary.is_file()
            || temporary
                .metadata()
                .map_or(true, |metadata| metadata.len() == 0)
        {
            let _ = fs::remove_file(&temporary);
            return Err(format!(
                "ImageMagick produced no output for {}.",
                source.display()
            ));
        }

        let output = match publish_output(source, &temporary, &extension) {
            Ok(output) => output,
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(format!("Could not save the converted image: {error}"));
            }
        };
        if let Err(error) = fs::remove_file(&temporary) {
            let _ = fs::remove_file(&output);
            return Err(format!("Could not clean up the temporary image: {error}"));
        }
        converted += 1;
        progress(
            format!("Converted {name}"),
            converted as f64 / total as f64,
        );
        if cancel.load(Ordering::Relaxed) {
            return Err(canceled_message(converted));
        }
        if !output.is_file() {
            return Err(format!(
                "The converted image was not created: {}",
                output.display()
            ));
        }
    }

    Ok(format!("Converted {converted} image(s) to {format}."))
}

fn load_available_formats() -> Result<Vec<String>, String> {
    let output = Command::new("magick")
        .args(["-list", "format"])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Image conversion requires ImageMagick (magick): {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            "ImageMagick could not list its supported formats.".to_owned()
        } else {
            format!("ImageMagick could not list its supported formats: {detail}")
        });
    }

    let formats = parse_format_list(&String::from_utf8_lossy(&output.stdout));
    if formats.is_empty() {
        return Err("ImageMagick reports no writable image formats.".to_owned());
    }
    Ok(formats)
}

fn parse_format_list(output: &str) -> Vec<String> {
    let mut formats = output
        .lines()
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let name = columns.next()?.trim_end_matches('*');
            let mode = columns.next()?;
            if mode.len() != 3 || mode.as_bytes()[1] != b'w' {
                return None;
            }
            if name.is_empty()
                || !name
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
            {
                return None;
            }
            Some(name.to_ascii_lowercase())
        })
        .collect::<Vec<_>>();
    formats.sort_unstable();
    formats.dedup();
    formats
}

fn temporary_path(source: &Path, extension: &str) -> Result<PathBuf, String> {
    let parent = source.parent().ok_or_else(|| {
        format!(
            "Could not find a destination folder for {}.",
            source.display()
        )
    })?;
    for _ in 0..100 {
        let sequence = OUTPUT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".filem-convert-{}-{sequence}.{extension}",
            std::process::id()
        ));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err("Could not allocate a temporary image file name.".to_owned())
}

fn publish_output(source: &Path, temporary: &Path, extension: &str) -> io::Result<PathBuf> {
    let parent = source.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "source has no parent directory",
        )
    })?;
    let stem = source
        .file_stem()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "source has no file name"))?;

    for suffix in 1u32..=100_000 {
        let mut name = stem.to_os_string();
        if suffix > 1 {
            name.push(format!(" ({suffix})"));
        }
        name.push(".");
        name.push(extension);
        let output = parent.join(name);

        match fs::hard_link(temporary, &output) {
            Ok(()) => return Ok(output),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => match copy_without_overwrite(temporary, &output) {
                Ok(()) => return Ok(output),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            },
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not find an unused output name",
    ))
}

fn copy_without_overwrite(source: &Path, destination: &Path) -> io::Result<()> {
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let result = (|| {
        let mut input = File::open(source)?;
        io::copy(&mut input, &mut output)?;
        output.flush()
    })();
    if result.is_err() {
        let _ = fs::remove_file(destination);
    }
    result
}

struct MonitorProgress {
    stage: String,
    fraction: f64,
}

fn parse_monitor_progress(line: &str) -> Option<MonitorProgress> {
    let (operation, detail) = line.split_once(':')?;
    let stage = operation.split('/').next()?.trim();
    let percent = detail
        .rsplit_once(',')?
        .1
        .trim()
        .split_whitespace()
        .next()?
        .strip_suffix('%')?
        .parse::<f64>()
        .ok()?
        .clamp(0.0, 100.0)
        / 100.0;
    let (stage, fraction) = match stage {
        "Load" | "Read" => ("Reading", 0.02 + percent * 0.43),
        "Save" | "Write" => ("Writing", 0.5 + percent * 0.45),
        other => (other, 0.02 + percent * 0.93),
    };
    Some(MonitorProgress {
        stage: stage.to_owned(),
        fraction,
    })
}

fn run_magick(
    source: &Path,
    temporary: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&str, f64),
) -> Result<(), String> {
    let mut child = Command::new("magick")
        .arg("-monitor")
        .arg(source)
        .arg(temporary)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start ImageMagick: {error}"))?;

    let stderr = child.stderr.take().expect("stderr was piped");
    let latest_progress = Arc::new(Mutex::new(None));
    let reader_progress = Arc::clone(&latest_progress);
    let stderr_reader = std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut captured = Vec::new();
        let mut pending = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            match stderr.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    for byte in &buffer[..read] {
                        if matches!(*byte, b'\r' | b'\n') {
                            process_monitor_line(&pending, &mut captured, &reader_progress);
                            pending.clear();
                        } else if pending.len() < 8192 {
                            pending.push(*byte);
                        }
                    }
                }
            }
        }
        process_monitor_line(&pending, &mut captured, &reader_progress);
        String::from_utf8_lossy(&captured).trim().to_owned()
    });

    let mut last_fraction = 0.0f64;
    let status = loop {
        if let Some(update) = take_monitor_progress(&latest_progress) {
            last_fraction = last_fraction.max(update.fraction);
            progress(&update.stage, last_fraction);
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_reader.join();
            return Err("conversion canceled".to_owned());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stderr_reader.join();
                return Err(format!("could not wait for ImageMagick: {error}"));
            }
        }
    };

    let detail = stderr_reader.join().unwrap_or_default();
    if let Some(update) = take_monitor_progress(&latest_progress) {
        last_fraction = last_fraction.max(update.fraction);
        progress(&update.stage, last_fraction);
    }
    if status.success() {
        Ok(())
    } else if detail.is_empty() {
        Err(format!("ImageMagick exited with status {status}"))
    } else {
        Err(detail)
    }
}

fn process_monitor_line(
    line: &[u8],
    captured: &mut Vec<u8>,
    latest_progress: &Mutex<Option<MonitorProgress>>,
) {
    let line = String::from_utf8_lossy(line);
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if let Some(update) = parse_monitor_progress(line) {
        *latest_progress
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(update);
    } else if captured.len() < 8192 {
        captured.extend_from_slice(line.as_bytes());
        captured.push(b'\n');
    }
}

fn take_monitor_progress(
    latest_progress: &Mutex<Option<MonitorProgress>>,
) -> Option<MonitorProgress> {
    latest_progress
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
}

fn canceled_message(converted: usize) -> String {
    if converted == 0 {
        "Image conversion canceled.".to_owned()
    } else {
        format!("Image conversion canceled after saving {converted} image(s).")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_list_includes_only_unique_writable_formats() {
        let output = "\
      PNG* rw- Portable Network Graphics
      JPG* rw- JPEG
      PNG  rw+ Portable Network Graphics
      SVG  r-- Scalable Vector Graphics
      PDF  -w+ PDF output only
      odd! rw+ Invalid name
";

        assert_eq!(
            parse_format_list(output),
            ["jpg".to_owned(), "pdf".to_owned(), "png".to_owned()]
        );
    }

    #[test]
    fn monitor_progress_maps_load_and_save_stages() {
        let loading =
            parse_monitor_progress("Load/Image//tmp/photo.png: 1 of 2, 100% complete").unwrap();
        let saving =
            parse_monitor_progress("Save/Image//tmp/photo.jpeg: 1 of 2, 100% complete").unwrap();

        assert_eq!(loading.stage, "Reading");
        assert_eq!(saving.stage, "Writing");
        assert!(loading.fraction < saving.fraction);
        assert!((saving.fraction - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn conversion_keeps_source_and_never_overwrites_existing_output() {
        if !available_formats()
            .map(|formats| formats.iter().any(|format| format == "jpeg"))
            .unwrap_or(false)
        {
            return;
        }

        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        let first_output = directory.path().join("photo.jpeg");
        let generated = Command::new("magick")
            .args(["-size", "1x1", "xc:red"])
            .arg(&source)
            .status()
            .unwrap();
        assert!(generated.success());
        fs::write(&first_output, b"preserve existing file").unwrap();

        let mut progress = Vec::new();
        let result = convert_images(
            &[source.clone()],
            "jpeg",
            &AtomicBool::new(false),
            &mut |detail, fraction| progress.push((detail, fraction)),
        )
        .unwrap();

        assert!(source.is_file());
        assert_eq!(fs::read(first_output).unwrap(), b"preserve existing file");
        assert!(directory.path().join("photo (2).jpeg").is_file());
        assert!(result.contains("Converted 1 image"));
        assert!(progress.iter().any(|(detail, _)| detail.contains("photo.png")));
        assert_eq!(progress.last().unwrap().1, 1.0);

        if available_formats()
            .map(|formats| formats.iter().any(|format| format == "heic"))
            .unwrap_or(false)
        {
            convert_images(&[source], "heic", &AtomicBool::new(false), &mut |_, _| {}).unwrap();
            assert!(directory.path().join("photo.heic").is_file());
        }
    }
}
