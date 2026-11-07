//! Single-process coordination and background-window lifecycle.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use qmetaobject::prelude::*;
use qmetaobject::QVariantList;

use crate::config::{Config, PerformanceConfig};

const MAX_PENDING_OPEN_REQUESTS: usize = 32;

fn pending_open_requests() -> &'static Mutex<VecDeque<String>> {
    static REQUESTS: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
    REQUESTS.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn cache_clear_message() -> &'static Mutex<String> {
    static MESSAGE: OnceLock<Mutex<String>> = OnceLock::new();
    MESSAGE.get_or_init(|| Mutex::new(String::new()))
}

fn performance_state() -> &'static Mutex<PerformanceConfig> {
    static PERFORMANCE: OnceLock<Mutex<PerformanceConfig>> = OnceLock::new();
    PERFORMANCE.get_or_init(|| Mutex::new(normalize_performance(Config::load().0.performance)))
}

fn normalize_performance(mut settings: PerformanceConfig) -> PerformanceConfig {
    if !matches!(
        settings.background_idle_timeout_seconds,
        0 | 5 | 10 | 30 | 60 | 300
    ) {
        settings.background_idle_timeout_seconds = 30;
    }
    if !matches!(
        settings.startup_behavior.as_str(),
        "balanced" | "fast_reopen" | "low_memory"
    ) {
        settings.startup_behavior = "balanced".to_owned();
    }
    if !matches!(
        settings.thumbnail_cache_limit_mb,
        128 | 256 | 512 | 1024 | 2048
    ) {
        settings.thumbnail_cache_limit_mb = 512;
    }
    settings
}

pub fn performance_config() -> PerformanceConfig {
    performance_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn update_performance(
    update: impl FnOnce(&mut PerformanceConfig) -> Result<(), String>,
) -> Result<PerformanceConfig, String> {
    let mut shared = performance_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut next = shared.clone();
    update(&mut next)?;
    let (mut config, warning) = Config::load();
    if let Some(warning) = warning {
        return Err(warning);
    }
    config.performance = next.clone();
    config.save()?;
    *shared = next.clone();
    Ok(next)
}

#[derive(Default)]
struct WindowState {
    visible_windows: usize,
    idle_since: Option<Instant>,
}

fn window_state() -> &'static Mutex<WindowState> {
    static STATE: OnceLock<Mutex<WindowState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(WindowState::default()))
}

impl WindowState {
    fn opened(&mut self) {
        self.visible_windows = self.visible_windows.saturating_add(1);
        self.idle_since = None;
    }

    fn closed(&mut self, now: Instant) {
        self.visible_windows = self.visible_windows.saturating_sub(1);
        if self.visible_windows == 0 {
            self.idle_since = Some(now);
        }
    }

    fn should_exit(&mut self, performance: &PerformanceConfig, now: Instant) -> bool {
        if self.visible_windows > 0 {
            return false;
        }
        if !performance.keep_running_background {
            return true;
        }
        if performance.background_idle_timeout_seconds == 0 {
            return false;
        }
        let idle_since = *self.idle_since.get_or_insert(now);
        now.saturating_duration_since(idle_since)
            >= Duration::from_secs(u64::from(performance.background_idle_timeout_seconds))
    }
}

fn ipc_directory() -> PathBuf {
    PathBuf::from(format!("/tmp/filem-{}", unsafe { libc::geteuid() }))
}

fn ipc_socket_path() -> PathBuf {
    ipc_directory().join("instance.sock")
}

fn ipc_lock_path() -> PathBuf {
    ipc_directory().join("instance.lock")
}

fn prepare_ipc_directory(directory: &Path) -> Result<(), String> {
    match fs::create_dir(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(format!("Could not create {}: {error}", directory.display()));
        }
    }
    let metadata = fs::symlink_metadata(directory)
        .map_err(|error| format!("Could not inspect {}: {error}", directory.display()))?;
    if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(format!(
            "Refusing to use unsafe IPC directory {}",
            directory.display()
        ));
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Could not secure {}: {error}", directory.display()))
}

/// Holds the per-user advisory lock for as long as this process owns IPC.
pub struct InstanceGuard {
    _lock: File,
    socket_path: PathBuf,
}

impl InstanceGuard {
    /// Returns `Ok(None)` when an existing process accepted the launch request.
    pub fn acquire(initial_path: &str) -> Result<Option<Self>, String> {
        prepare_ipc_directory(&ipc_directory())?;
        Self::acquire_at(initial_path, ipc_lock_path(), ipc_socket_path())
    }

    fn acquire_at(
        initial_path: &str,
        lock_path: PathBuf,
        socket_path: PathBuf,
    ) -> Result<Option<Self>, String> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&lock_path)
            .map_err(|error| format!("Could not open {}: {error}", lock_path.display()))?;
        if lock
            .metadata()
            .map_err(|error| format!("Could not inspect {}: {error}", lock_path.display()))?
            .uid()
            != unsafe { libc::geteuid() }
        {
            return Err(format!(
                "IPC lock {} is not owned by this user",
                lock_path.display()
            ));
        }
        fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Could not secure {}: {error}", lock_path.display()))?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
                forward_open_request(initial_path, &socket_path)?;
                return Ok(None);
            }
            return Err(format!(
                "Could not lock {}: {}",
                lock_path.display(),
                std::io::Error::last_os_error()
            ));
        }

        match fs::symlink_metadata(&socket_path) {
            Ok(metadata)
                if metadata.file_type().is_socket()
                    && metadata.uid() == unsafe { libc::geteuid() } =>
            {
                fs::remove_file(&socket_path).map_err(|error| {
                    format!(
                        "Could not remove stale socket {}: {error}",
                        socket_path.display()
                    )
                })?;
            }
            Ok(_) => {
                return Err(format!(
                    "Refusing to replace non-socket IPC path {}",
                    socket_path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Could not inspect {}: {error}",
                    socket_path.display()
                ));
            }
        }
        let listener = UnixListener::bind(&socket_path)
            .map_err(|error| format!("Could not create {}: {error}", socket_path.display()))?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Could not secure {}: {error}", socket_path.display()))?;
        thread::Builder::new()
            .name("filem-ipc".to_owned())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else {
                        continue;
                    };
                    receive_open_request(stream);
                }
            })
            .map_err(|error| format!("Could not start IPC listener: {error}"))?;

        Ok(Some(Self {
            _lock: lock,
            socket_path,
        }))
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::symlink_metadata(&self.socket_path) {
            if metadata.file_type().is_socket() && metadata.uid() == unsafe { libc::geteuid() } {
                let _ = fs::remove_file(&self.socket_path);
            }
        }
    }
}

fn forward_open_request(path: &str, socket_path: &PathBuf) -> Result<(), String> {
    let payload = serde_json::to_string(path)
        .map_err(|error| format!("Could not encode open request: {error}"))?;
    let mut last_error = None;
    for _ in 0..50 {
        match UnixStream::connect(socket_path) {
            Ok(mut stream) => {
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .map_err(|error| format!("Could not configure IPC: {error}"))?;
                stream
                    .write_all(payload.as_bytes())
                    .and_then(|_| stream.write_all(b"\n"))
                    .map_err(|error| format!("Could not send open request: {error}"))?;
                return Ok(());
            }
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
    Err(format!(
        "The running Filem process did not accept the open request: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "IPC endpoint unavailable".to_owned())
    ))
}

fn receive_open_request(stream: UnixStream) {
    let mut line = String::new();
    let result = BufReader::new(stream).take(64 * 1024).read_line(&mut line);
    if result.is_err() || line.len() >= 64 * 1024 {
        return;
    }
    let Ok(path) = serde_json::from_str::<String>(line.trim_end()) else {
        return;
    };
    let mut requests = pending_open_requests()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if requests.len() == MAX_PENDING_OPEN_REQUESTS {
        requests.pop_front();
    }
    requests.push_back(path);
}

#[derive(QObject)]
pub struct Lifecycle {
    base: qt_base_class!(trait QObject),

    keep_running_background: qt_property!(bool; NOTIFY settings_changed),
    background_idle_timeout_seconds: qt_property!(i32; NOTIFY settings_changed),
    startup_behavior: qt_property!(QString; NOTIFY settings_changed),
    thumbnail_cache_limit_mb: qt_property!(i32; NOTIFY settings_changed),
    settings_error: qt_property!(QString; NOTIFY settings_changed),
    cache_clear_status: qt_property!(QString; NOTIFY cache_clear_changed),
    settings_changed: qt_signal!(),
    cache_clear_changed: qt_signal!(),

    poll_open_requests: qt_method!(fn(&mut self) -> QVariantList),
    visible_window_count: qt_method!(fn(&self) -> i32),
    window_opened: qt_method!(fn(&mut self)),
    window_closed: qt_method!(fn(&mut self)),
    should_exit: qt_method!(fn(&self) -> bool),
    set_keep_running_background: qt_method!(fn(&mut self, enabled: bool)),
    set_background_idle_timeout: qt_method!(fn(&mut self, seconds: i32)),
    set_startup_behavior: qt_method!(fn(&mut self, behavior: QString)),
    set_thumbnail_cache_limit: qt_method!(fn(&mut self, megabytes: i32)),
    clear_thumbnail_cache: qt_method!(fn(&mut self)),
    poll_cache_clear_status: qt_method!(fn(&mut self)),
}

impl Default for Lifecycle {
    fn default() -> Self {
        let performance = performance_config();
        Self {
            base: Default::default(),
            keep_running_background: performance.keep_running_background,
            background_idle_timeout_seconds: performance.background_idle_timeout_seconds as i32,
            startup_behavior: QString::from(performance.startup_behavior),
            thumbnail_cache_limit_mb: performance.thumbnail_cache_limit_mb as i32,
            settings_error: QString::default(),
            cache_clear_status: QString::default(),
            settings_changed: Default::default(),
            cache_clear_changed: Default::default(),
            poll_open_requests: Default::default(),
            visible_window_count: Default::default(),
            window_opened: Default::default(),
            window_closed: Default::default(),
            should_exit: Default::default(),
            set_keep_running_background: Default::default(),
            set_background_idle_timeout: Default::default(),
            set_startup_behavior: Default::default(),
            set_thumbnail_cache_limit: Default::default(),
            clear_thumbnail_cache: Default::default(),
            poll_cache_clear_status: Default::default(),
        }
    }
}

impl Lifecycle {
    fn poll_open_requests(&mut self) -> QVariantList {
        let mut requests = pending_open_requests()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut result = QVariantList::default();
        while let Some(path) = requests.pop_front() {
            result.push(QVariant::from(QString::from(path)));
        }
        result
    }

    fn visible_window_count(&self) -> i32 {
        window_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .visible_windows
            .min(i32::MAX as usize) as i32
    }

    fn window_opened(&mut self) {
        let mut state = window_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.opened();
    }

    fn window_closed(&mut self) {
        let low_memory = performance_config().startup_behavior == "low_memory";
        let mut state = window_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.closed(Instant::now());
        if state.visible_windows == 0 {
            if low_memory {
                crate::metadata::clear_cache();
                crate::icons::clear_shared_cache();
            }
        }
    }

    fn should_exit(&self) -> bool {
        let performance = performance_config();
        let mut state = window_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.should_exit(&performance, Instant::now())
    }

    fn set_keep_running_background(&mut self, enabled: bool) {
        self.update_setting(|settings| settings.keep_running_background = enabled);
    }

    fn set_background_idle_timeout(&mut self, seconds: i32) {
        if !matches!(seconds, 5 | 10 | 30 | 60 | 300 | 0) {
            self.settings_error = QString::from("Choose a supported background idle timeout.");
            self.settings_changed();
            return;
        }
        self.update_setting(|settings| {
            settings.background_idle_timeout_seconds = seconds as u32;
        });
    }

    fn set_startup_behavior(&mut self, behavior: QString) {
        let behavior = behavior.to_string();
        if !matches!(behavior.as_str(), "balanced" | "fast_reopen" | "low_memory") {
            self.settings_error = QString::from("Choose a supported startup behavior.");
            self.settings_changed();
            return;
        }
        self.update_setting(|settings| settings.startup_behavior = behavior);
    }

    fn set_thumbnail_cache_limit(&mut self, megabytes: i32) {
        if !matches!(megabytes, 128 | 256 | 512 | 1024 | 2048) {
            self.settings_error = QString::from("Choose a supported thumbnail cache limit.");
            self.settings_changed();
            return;
        }
        self.update_setting(|settings| settings.thumbnail_cache_limit_mb = megabytes as u32);
    }

    fn clear_thumbnail_cache(&mut self) {
        self.cache_clear_status = QString::from("Clearing Filem thumbnails…");
        self.cache_clear_changed();
        let cache_dir = crate::paths::cache_dir();
        thread::spawn(move || {
            let result = crate::preview::clear_filem_thumbnail_cache(&cache_dir);
            let message = result
                .map(|()| "Filem thumbnail cache cleared.".to_owned())
                .unwrap_or_else(|error| error);
            *cache_clear_message()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = message;
        });
    }

    fn poll_cache_clear_status(&mut self) {
        let message = cache_clear_message()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if self.cache_clear_status.to_string() != message && !message.is_empty() {
            self.cache_clear_status = QString::from(message);
            self.cache_clear_changed();
        }
    }

    fn update_setting(&mut self, update: impl FnOnce(&mut PerformanceConfig)) {
        match update_performance(|settings| {
            update(settings);
            Ok(())
        }) {
            Ok(settings) => {
                if settings.thumbnail_cache_limit_mb != self.thumbnail_cache_limit_mb as u32 {
                    thread::spawn(|| {
                        crate::preview::trim_thumbnail_cache(&crate::paths::cache_dir(), true)
                    });
                }
                self.keep_running_background = settings.keep_running_background;
                self.background_idle_timeout_seconds =
                    settings.background_idle_timeout_seconds as i32;
                self.startup_behavior = QString::from(settings.startup_behavior);
                self.thumbnail_cache_limit_mb = settings.thumbnail_cache_limit_mb as i32;
                self.settings_error = QString::default();
            }
            Err(error) => self.settings_error = QString::from(error),
        }
        self.settings_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_payload_round_trips_paths_with_spaces_and_unicode() {
        let path = "/home/user/資料/Work Files".to_owned();
        let payload = serde_json::to_string(&path).unwrap();
        assert_eq!(serde_json::from_str::<String>(&payload).unwrap(), path);
    }

    #[test]
    fn timeout_values_have_explicit_until_logout_representation() {
        assert_eq!(u64::from(0_u32), 0);
        assert_eq!(Duration::from_secs(5).as_secs(), 5);
        assert_eq!(Duration::from_secs(300).as_secs(), 300);
    }

    #[test]
    fn window_counts_keep_idle_from_starting_until_the_last_close() {
        let now = Instant::now();
        let mut state = WindowState::default();
        state.opened();
        state.opened();
        state.closed(now);
        assert_eq!(state.visible_windows, 1);
        assert!(state.idle_since.is_none());
        state.closed(now);
        assert_eq!(state.visible_windows, 0);
        assert_eq!(state.idle_since, Some(now));
    }

    #[test]
    fn reopening_during_keepalive_cancels_the_idle_deadline() {
        let closed_at = Instant::now();
        let mut state = WindowState {
            visible_windows: 0,
            idle_since: Some(closed_at),
        };
        let mut settings = PerformanceConfig::default();
        settings.background_idle_timeout_seconds = 5;
        state.opened();
        assert!(!state.should_exit(&settings, closed_at + Duration::from_secs(10)));
        state.closed(closed_at + Duration::from_secs(10));
        assert!(!state.should_exit(&settings, closed_at + Duration::from_secs(14)));
        assert!(state.should_exit(&settings, closed_at + Duration::from_secs(15)));
    }

    #[test]
    fn idle_timeout_and_until_logout_follow_settings() {
        let start = Instant::now();
        let mut state = WindowState {
            visible_windows: 0,
            idle_since: Some(start),
        };
        let mut settings = PerformanceConfig::default();
        settings.background_idle_timeout_seconds = 5;
        assert!(!state.should_exit(&settings, start + Duration::from_secs(4)));
        assert!(state.should_exit(&settings, start + Duration::from_secs(5)));
        settings.background_idle_timeout_seconds = 0;
        assert!(!state.should_exit(&settings, start + Duration::from_secs(600)));
        settings.keep_running_background = false;
        assert!(state.should_exit(&settings, start));
        state.visible_windows = 1;
        assert!(!state.should_exit(&settings, start));
    }

    #[test]
    fn second_launch_is_forwarded_to_the_locked_instance() {
        let directory = tempfile::tempdir().unwrap();
        let lock_path = directory.path().join("filem.lock");
        let socket_path = directory.path().join("filem.sock");
        pending_open_requests().lock().unwrap().clear();
        let primary = InstanceGuard::acquire_at("", lock_path.clone(), socket_path.clone())
            .unwrap()
            .unwrap();
        assert!(
            InstanceGuard::acquire_at("/tmp/Folder With Spaces", lock_path, socket_path,)
                .unwrap()
                .is_none()
        );
        for _ in 0..50 {
            if !pending_open_requests().lock().unwrap().is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            pending_open_requests()
                .lock()
                .unwrap()
                .pop_front()
                .as_deref(),
            Some("/tmp/Folder With Spaces")
        );
        drop(primary);
    }

    #[test]
    fn stale_owned_socket_is_replaced_but_regular_files_are_not() {
        let directory = tempfile::tempdir().unwrap();
        let lock_path = directory.path().join("filem.lock");
        let socket_path = directory.path().join("filem.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        drop(listener);
        assert!(fs::symlink_metadata(&socket_path)
            .unwrap()
            .file_type()
            .is_socket());
        let primary = InstanceGuard::acquire_at("", lock_path.clone(), socket_path.clone())
            .unwrap()
            .unwrap();
        assert!(fs::symlink_metadata(&socket_path)
            .unwrap()
            .file_type()
            .is_socket());
        drop(primary);

        fs::write(&socket_path, b"preserve").unwrap();
        assert!(InstanceGuard::acquire_at("", lock_path, socket_path.clone()).is_err());
        assert_eq!(fs::read(socket_path).unwrap(), b"preserve");
    }
}
