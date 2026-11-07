//! User-session storage discovery and explicit UDisks/GIO actions.

use gio::prelude::*;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use crate::paths::format_size;

#[derive(Debug, Clone)]
pub struct Volume {
    pub path: Option<PathBuf>,
    pub device: String,
    pub eject_device: String,
    pub filesystem: String,
    pub label: String,
    pub total: u64,
    pub free: u64,
    pub removable: bool,
    pub network: bool,
    pub encrypted: bool,
    pub locked: bool,
    /// True for removable media such as a USB flash drive, as opposed to a
    /// fixed disk in a USB enclosure. Only used to pick the sidebar glyph.
    pub usb_flash: bool,
    /// The running system's root or another system partition such as `/boot`.
    /// These are navigable but must never offer unmount/eject actions.
    pub system: bool,
}

impl Volume {
    pub fn can_unmount(&self) -> bool {
        !self.system && self.path.is_some()
    }

    pub fn can_eject(&self) -> bool {
        !self.system && self.removable && !self.eject_device.is_empty()
    }
}

#[derive(Deserialize)]
struct Lsblk {
    blockdevices: Vec<BlockDevice>,
}

#[derive(Deserialize)]
struct BlockDevice {
    name: String,
    path: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    rm: Option<bool>,
    tran: Option<String>,
    size: Option<u64>,
    fstype: Option<String>,
    label: Option<String>,
    mountpoints: Option<Vec<Option<String>>>,
    children: Option<Vec<BlockDevice>>,
}

fn unescape_mount(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 4], 8) {
                out.push(byte);
                index += 4;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const NETWORK_FS: [&str; 9] = [
    "cifs",
    "smb3",
    "nfs",
    "nfs4",
    "sshfs",
    "davfs",
    "fuse.sshfs",
    "fuse.rclone",
    "9p",
];

/// Filesystems that only expose kernel or session state. They are never
/// ordinary storage devices, even when mounted at `/` or `/boot`.
const PSEUDO_FS: &[&str] = &[
    "proc",
    "sysfs",
    "tmpfs",
    "devtmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "pstore",
    "securityfs",
    "debugfs",
    "tracefs",
    "configfs",
    "fusectl",
    "mqueue",
    "hugetlbfs",
    "bpf",
    "autofs",
    "rpc_pipefs",
    "ramfs",
    "efivarfs",
    "binfmt_misc",
    "nsfs",
    "fuse.portal",
    "fuse.gvfsd-fuse",
    "fuse.lxcfs",
    "fuse.snapfuse",
];

fn is_pseudo_filesystem(filesystem: &str) -> bool {
    PSEUDO_FS.contains(&filesystem)
}

fn capacity(path: &Path) -> (u64, u64) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return (0, 0);
    };
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path.as_ptr(), &mut stat) != 0 {
            return (0, 0);
        }
        let block = stat.f_frsize.max(1) as u64;
        (
            (stat.f_blocks as u64).saturating_mul(block),
            (stat.f_bavail as u64).saturating_mul(block),
        )
    }
}

/// User-visible mounts below common media directories or on network
/// filesystems. The running system and `/boot` are handled separately by
/// `system_volumes()` so they can be marked as protected system volumes.
fn is_relevant_mount(mount: &str, fs: &str) -> bool {
    !is_pseudo_filesystem(fs)
        && mount != "/"
        && (mount.starts_with("/run/media/")
            || mount.starts_with("/media/")
            || mount.starts_with("/mnt/")
            || NETWORK_FS.contains(&fs))
}

fn mounted_volumes() -> Vec<Volume> {
    let Ok(text) = fs::read_to_string("/proc/self/mounts") else {
        return Vec::new();
    };
    let mut volumes = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let (Some(device), Some(mount), Some(filesystem)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let device = unescape_mount(device);
        let mount = unescape_mount(mount);
        if !is_relevant_mount(&mount, filesystem) || !seen.insert(mount.clone()) {
            continue;
        }
        let path = PathBuf::from(&mount);
        let (total, free) = capacity(&path);
        let network = NETWORK_FS.contains(&filesystem);
        let label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| device.clone());
        volumes.push(Volume {
            path: Some(path),
            device,
            eject_device: String::new(),
            filesystem: filesystem.to_owned(),
            label,
            total,
            free,
            removable: mount.starts_with("/run/media/") || mount.starts_with("/media/"),
            network,
            encrypted: false,
            locked: false,
            usb_flash: false,
            system: false,
        });
    }

    // GVfs exposes MTP, AFC and camera mounts below one FUSE mount. Enumerate
    // its child directories so phones and cameras appear as navigable devices.
    if let Some(root) = gvfs_root() {
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() || !seen.insert(path.to_string_lossy().into_owned()) {
                    continue;
                }
                let source = entry.file_name().to_string_lossy().into_owned();
                let label = gvfs_label(&source);
                let (total, free) = capacity(&path);
                let kind = source.split(':').next().unwrap_or("");
                let network = matches!(kind, "smb-share" | "nfs" | "sftp" | "dav" | "davs" | "ftp");
                volumes.push(Volume {
                    path: Some(path.clone()),
                    device: format!("gio:{source}"),
                    eject_device: format!("gio:{source}"),
                    filesystem: "GVfs".to_owned(),
                    label,
                    total,
                    free,
                    removable: !network,
                    network,
                    encrypted: false,
                    locked: false,
                    usb_flash: false,
                    system: false,
                });
            }
        }
    }
    volumes
}

/// Pick the human-readable OS name from os-release. `NAME` is preferred over
/// `PRETTY_NAME` so the root device reads "NixOS" rather than the full release
/// title; distributions without `NAME` still get a useful label.
fn os_release_name(text: &str) -> Option<String> {
    let mut name = None;
    let mut pretty = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches(['"', '\'']).trim();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "NAME" => name = Some(value.to_owned()),
            "PRETTY_NAME" => pretty = Some(value.to_owned()),
            _ => {}
        }
    }
    name.or(pretty)
}

fn system_label() -> String {
    fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| os_release_name(&text))
        .unwrap_or_else(|| "System".to_owned())
}

fn system_volume(device: String, mount: &str, filesystem: &str, label: String) -> Volume {
    let path = PathBuf::from(mount);
    let (total, free) = capacity(&path);
    Volume {
        path: Some(path),
        device,
        eject_device: String::new(),
        filesystem: filesystem.to_owned(),
        label,
        total,
        free,
        removable: false,
        network: false,
        encrypted: false,
        locked: false,
        usb_flash: false,
        system: true,
    }
}

/// Discover the running root filesystem and, when it is a separate real mount,
/// `/boot` from the kernel's mount table. Non-storage filesystems are ignored,
/// and the last mount recorded for a path wins so overmounts are respected.
fn parse_system_volumes(text: &str, root_label: &str) -> Vec<Volume> {
    let mut root = None;
    let mut boot = None;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let (Some(device), Some(mount), Some(filesystem)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if is_pseudo_filesystem(filesystem) {
            continue;
        }
        let device = unescape_mount(device);
        let mount = unescape_mount(mount);
        match mount.as_str() {
            "/" => root = Some((device, filesystem.to_owned())),
            "/boot" => boot = Some((device, filesystem.to_owned())),
            _ => {}
        }
    }

    let mut volumes = Vec::new();
    if let Some((device, filesystem)) = root {
        volumes.push(system_volume(
            device,
            "/",
            &filesystem,
            root_label.to_owned(),
        ));
    }
    if let Some((device, filesystem)) = boot {
        volumes.push(system_volume(
            device,
            "/boot",
            &filesystem,
            "Boot".to_owned(),
        ));
    }
    volumes
}

fn system_volumes() -> Vec<Volume> {
    let Ok(text) = fs::read_to_string("/proc/self/mounts") else {
        return Vec::new();
    };
    parse_system_volumes(&text, &system_label())
}

/// Structured snapshot of one GIO volume.
///
/// Keeping this independent of the GObject lets the GIO queries run on their
/// own thread and makes the mapping testable without a live GVfs session.
#[derive(Debug, Clone, Default)]
struct GioVolumeInfo {
    name: String,
    uuid: Option<String>,
    unix_device: Option<String>,
    activation_uri: Option<String>,
    /// Directory name inside the GVfs FUSE mount, when this volume is mounted
    /// through GVfs (for example `mtp:host=Pixel`).
    gvfs_source: Option<String>,
    path: Option<PathBuf>,
    can_mount: bool,
    can_eject: bool,
}

fn gio_info_to_volume(info: GioVolumeInfo) -> Option<Volume> {
    let activation_source = info.activation_uri.as_deref().and_then(gvfs_source_for_uri);

    // Prefer the actual GVfs directory name so the device identifier matches
    // the one produced by mounted_volumes(). Fall back through the activation
    // URI, Unix device, and finally the UUID so missing optional properties do
    // not hide a usable device.
    let source = info
        .gvfs_source
        .clone()
        .or_else(|| activation_source.clone())
        .or_else(|| info.unix_device.clone())
        .or_else(|| info.uuid.clone().map(|uuid| format!("uuid={uuid}")))?;

    let (filesystem, removable) = info
        .gvfs_source
        .as_deref()
        .and_then(gio_volume_kind)
        .or_else(|| info.activation_uri.as_deref().and_then(gio_volume_kind))
        .or_else(|| activation_source.as_deref().and_then(gio_volume_kind))
        .or_else(|| info.unix_device.as_deref().and_then(gio_volume_kind))?;

    // A volume that can neither be mounted nor is currently mounted has no
    // useful action. Devices with missing optional metadata (UUID, Unix device,
    // label) are still kept above.
    if !info.can_mount && info.path.is_none() {
        return None;
    }

    let label = if info.name.trim().is_empty() {
        gvfs_label(&source)
    } else {
        info.name
    };
    let device = format!("gio:{source}");
    let (total, free) = info.path.as_deref().map(capacity).unwrap_or((0, 0));
    Some(Volume {
        path: info.path,
        eject_device: if info.can_eject {
            device.clone()
        } else {
            String::new()
        },
        device,
        filesystem: filesystem.to_owned(),
        label,
        total,
        free,
        removable,
        network: !removable,
        encrypted: false,
        locked: false,
        usb_flash: false,
        system: false,
    })
}

fn gio_volume_info(volume: &gio::Volume, gvfs_root: Option<&Path>) -> GioVolumeInfo {
    let mount = volume.get_mount();
    let mount_path = mount.as_ref().and_then(|mount| mount.root().path());
    let gvfs_source = mount_path.as_deref().and_then(|path| {
        (path.parent() == gvfs_root)
            .then(|| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .flatten()
    });
    let activation_uri = volume.activation_root().map(|file| file.uri().to_string());
    let path = mount_path.or_else(|| activation_uri.as_deref().and_then(gvfs_path_for_uri));

    GioVolumeInfo {
        name: volume.name().to_string(),
        uuid: volume.uuid().map(|value| value.to_string()),
        unix_device: volume
            .identifier("unix-device")
            .map(|value| value.to_string()),
        activation_uri,
        gvfs_source,
        path,
        can_mount: volume.can_mount(),
        can_eject: volume.can_eject(),
    }
}

fn collect_gio_volumes(monitor: &gio::VolumeMonitor) -> Vec<Volume> {
    let gvfs_root = gvfs_root();
    let volumes = monitor
        .volumes()
        .iter()
        .map(|volume| gio_volume_info(volume, gvfs_root.as_deref()))
        .filter_map(gio_info_to_volume)
        .collect();
    dedup_gio_volumes(volumes)
}

fn dedup_gio_volumes(volumes: Vec<Volume>) -> Vec<Volume> {
    let mut indexes: HashMap<String, usize> = HashMap::new();
    let mut result: Vec<Volume> = Vec::new();
    for volume in volumes {
        if let Some(&index) = indexes.get(&volume.device) {
            // Keep a mounted entry over an unmounted duplicate of the same device.
            if result[index].path.is_none() && volume.path.is_some() {
                result[index] = volume;
            }
            continue;
        }
        indexes.insert(volume.device.clone(), result.len());
        result.push(volume);
    }
    result
}

struct GioDiscovery {
    requests: Sender<Sender<Vec<Volume>>>,
}

fn gio_discovery() -> Option<&'static GioDiscovery> {
    static WORKER: OnceLock<Option<GioDiscovery>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (requests, receiver) = mpsc::channel();
            std::thread::Builder::new()
                .name("filem-gio-monitor".to_owned())
                .spawn(move || gio_monitor_loop(receiver))
                .ok()
                .map(|_| GioDiscovery { requests })
        })
        .as_ref()
}

fn gio_monitor_loop(requests: Receiver<Sender<Vec<Volume>>>) {
    // GVolumeMonitor is not thread-default-context aware and only notices
    // hotplug events while its main context is being iterated. Keep one
    // long-lived worker and pump that context so later discoveries stay current.
    let context = gio::glib::MainContext::default();
    if context
        .with_thread_default(|| gio_serve(&context, &requests))
        .is_err()
    {
        let context = gio::glib::MainContext::new();
        let _ = context.with_thread_default(|| gio_serve(&context, &requests));
    }
}

fn gio_serve(context: &gio::glib::MainContext, requests: &Receiver<Sender<Vec<Volume>>>) {
    let monitor = gio::VolumeMonitor::get();
    loop {
        while context.pending() {
            context.iteration(false);
        }
        match requests.recv_timeout(Duration::from_millis(200)) {
            Ok(reply) => {
                while context.pending() {
                    context.iteration(false);
                }
                let _ = reply.send(collect_gio_volumes(&monitor));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn gio_volumes() -> Vec<Volume> {
    gio_volumes_from(gio_discovery())
}

fn gio_volumes_from(worker: Option<&GioDiscovery>) -> Vec<Volume> {
    let Some(worker) = worker else {
        return Vec::new();
    };
    let (reply, response) = mpsc::channel();
    if worker.requests.send(reply).is_err() {
        return Vec::new();
    }
    response
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default()
}

fn gio_volume_kind(device: &str) -> Option<(&'static str, bool)> {
    let device = device.to_lowercase();
    if device.starts_with("mtp:") || device.starts_with("mtp://") {
        Some(("Phone", true))
    } else if device.starts_with("gphoto2:") || device.starts_with("gphoto2://") {
        Some(("Camera", true))
    } else if device.starts_with("afc:") || device.starts_with("afc://") {
        Some(("Apple device", true))
    } else if ["smb-share:", "sftp:", "nfs:", "dav:", "davs:", "ftp:"]
        .iter()
        .any(|prefix| device.starts_with(prefix))
    {
        Some(("GVfs", false))
    } else {
        None
    }
}

fn gvfs_source_for_uri(uri: &str) -> Option<String> {
    let (scheme, authority) = uri.split_once("://")?;
    let host = authority.split('/').next()?.trim();
    if host.is_empty() || host.contains('@') {
        return None;
    }
    let host = percent_encoding::percent_decode_str(host)
        .decode_utf8()
        .ok()?;
    Some(format!("{scheme}:host={host}"))
}

fn gvfs_path_for_uri(uri: &str) -> Option<PathBuf> {
    let expected = gvfs_source_for_uri(uri)?;
    let root = gvfs_root()?;
    fs::read_dir(root)
        .ok()?
        .flatten()
        .find(|entry| entry.file_name().to_string_lossy() == expected)
        .map(|entry| entry.path())
}

fn gvfs_root() -> Option<PathBuf> {
    let text = fs::read_to_string("/proc/self/mounts").ok()?;
    text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        fields.next()?;
        let mount = unescape_mount(fields.next()?);
        let filesystem = fields.next()?;
        (filesystem == "fuse.gvfsd-fuse").then(|| PathBuf::from(mount))
    })
}

fn gvfs_label(source: &str) -> String {
    let (kind, details) = source.split_once(':').unwrap_or((source, ""));
    let device = details
        .split(',')
        .find_map(|part| part.strip_prefix("host="))
        .unwrap_or(details)
        .replace("%20", " ");
    match kind {
        "mtp" => format!(
            "Phone{}",
            if device.is_empty() {
                String::new()
            } else {
                format!(" — {device}")
            }
        ),
        "gphoto2" => format!(
            "Camera{}",
            if device.is_empty() {
                String::new()
            } else {
                format!(" — {device}")
            }
        ),
        "afc" => format!(
            "Apple device{}",
            if device.is_empty() {
                String::new()
            } else {
                format!(" — {device}")
            }
        ),
        _ => source.to_owned(),
    }
}

fn add_block_devices(
    nodes: &[BlockDevice],
    parent_usb: bool,
    parent_path: &str,
    parent_encrypted: bool,
    parent_removable_media: bool,
    output: &mut Vec<Volume>,
    seen: &mut HashSet<String>,
    seen_paths: &mut HashSet<PathBuf>,
) {
    for node in nodes {
        let path = node
            .path
            .clone()
            .unwrap_or_else(|| format!("/dev/{}", node.name));
        let removable_media = node.rm.unwrap_or(false) || parent_removable_media;
        let usb = parent_usb || node.rm.unwrap_or(false) || node.tran.as_deref() == Some("usb");
        let usb_flash = usb && removable_media;
        let filesystem = node.fstype.clone().unwrap_or_default();
        let encrypted = filesystem.eq_ignore_ascii_case("crypto_LUKS");
        let swap = filesystem.eq_ignore_ascii_case("swap");
        let physical_device = if node.kind.as_deref() == Some("disk") && usb {
            path.as_str()
        } else if usb {
            parent_path
        } else {
            ""
        };
        if !physical_device.is_empty() {
            for volume in output
                .iter_mut()
                .filter(|volume| volume.device == path && volume.removable)
            {
                if volume.eject_device.is_empty() {
                    volume.eject_device = physical_device.to_owned();
                }
            }
        }
        let unlocked_child = if encrypted {
            node.children.as_ref().and_then(|children| {
                children
                    .iter()
                    .find(|child| child.kind.as_deref() == Some("crypt"))
            })
        } else {
            None
        };
        let mountpoints = unlocked_child
            .and_then(|child| child.mountpoints.as_ref())
            .or(node.mountpoints.as_ref());
        let mounted_at_root = mountpoints.is_some_and(|points| {
            points
                .iter()
                .flatten()
                .any(|point| Path::new(point) == Path::new("/"))
        });
        let mount = mountpoints
            .and_then(|points| points.iter().flatten().next())
            .cloned();
        let mounted_path = unlocked_child
            .and_then(|child| child.mountpoints.as_ref())
            .and_then(|points| points.iter().flatten().next())
            .or(mount.as_ref())
            .map(PathBuf::from);
        // A USB flash drive can already have been discovered from the mount
        // table or GIO. Carry the removable-media bit over to that entry so the
        // sidebar can distinguish a flash drive from an external disk.
        if usb_flash {
            if let Some(volume) = output.iter_mut().find(|volume| volume.device == path) {
                volume.usb_flash = true;
            } else if let Some(mounted) = mounted_path.as_deref() {
                if let Some(volume) = output
                    .iter_mut()
                    .find(|volume| volume.path.as_deref() == Some(mounted))
                {
                    volume.usb_flash = true;
                }
            }
        }
        let duplicate_path = mounted_path
            .as_ref()
            .is_some_and(|path| seen_paths.contains(path));
        let eligible = !swap
            && !parent_encrypted
            && !mounted_at_root
            && !duplicate_path
            && (encrypted
                || (node.kind.as_deref() == Some("part") && usb && !filesystem.is_empty())
                || (node.kind.as_deref() == Some("part")
                    && !filesystem.is_empty()
                    && mount.as_deref().is_some_and(|point| point != "/")))
            && seen.insert(path.clone());
        if eligible {
            if mounted_path
                .as_deref()
                .is_some_and(|path| path == Path::new("/"))
            {
                continue;
            }
            if let Some(path) = &mounted_path {
                seen_paths.insert(path.clone());
            }
            let locked = encrypted && unlocked_child.is_none();
            let (total, free) = mounted_path.as_deref().map(capacity).unwrap_or((
                unlocked_child
                    .and_then(|child| child.size)
                    .or(node.size)
                    .unwrap_or(0),
                0,
            ));
            output.push(Volume {
                path: mounted_path,
                device: unlocked_child
                    .and_then(|child| child.path.clone())
                    .unwrap_or_else(|| path.clone()),
                eject_device: if !usb {
                    String::new()
                } else if parent_path.is_empty() {
                    path.clone()
                } else {
                    parent_path.to_owned()
                },
                filesystem: if let Some(child) = unlocked_child {
                    child.fstype.clone().unwrap_or_else(|| "LUKS".to_owned())
                } else if encrypted {
                    "LUKS".to_owned()
                } else {
                    filesystem
                },
                label: node
                    .label
                    .clone()
                    .filter(|label| !label.is_empty())
                    .unwrap_or_else(|| node.name.clone()),
                total,
                free,
                removable: usb,
                network: false,
                encrypted,
                locked,
                usb_flash,
                system: false,
            });
        }
        let next_parent = if node.kind.as_deref() == Some("disk") && usb {
            path.as_str()
        } else if parent_usb {
            parent_path
        } else {
            ""
        };
        if let Some(children) = &node.children {
            add_block_devices(
                children,
                usb,
                next_parent,
                parent_encrypted || encrypted,
                removable_media,
                output,
                seen,
                seen_paths,
            );
        }
    }
}

fn merge_volumes(volumes: &mut Vec<Volume>, incoming: Vec<Volume>) {
    let mut seen: HashSet<String> = volumes.iter().map(|volume| volume.device.clone()).collect();
    let mut seen_paths: HashSet<PathBuf> = volumes
        .iter()
        .filter_map(|volume| volume.path.clone())
        .collect();
    for volume in incoming {
        if seen.contains(&volume.device) {
            continue;
        }
        if volume
            .path
            .as_ref()
            .is_some_and(|path| seen_paths.contains(path))
        {
            continue;
        }
        seen.insert(volume.device.clone());
        if let Some(path) = &volume.path {
            seen_paths.insert(path.clone());
        }
        volumes.push(volume);
    }
}

fn add_lsblk_volumes(volumes: &mut Vec<Volume>) {
    let output = match Command::new("lsblk")
        .args([
            "--json",
            "--bytes",
            "--output",
            "NAME,PATH,TYPE,RM,TRAN,SIZE,FSTYPE,LABEL,MOUNTPOINTS",
        ])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprintln!(
                "filem: cannot inspect removable storage: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return;
        }
        Err(error) => {
            eprintln!("filem: cannot inspect removable storage: lsblk is unavailable ({error}).");
            return;
        }
    };
    let parsed: Lsblk = match serde_json::from_slice(&output.stdout) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("filem: could not read lsblk device information: {error}");
            return;
        }
    };
    let mut seen: HashSet<String> = volumes.iter().map(|volume| volume.device.clone()).collect();
    let mut seen_paths: HashSet<PathBuf> = volumes
        .iter()
        .filter_map(|volume| volume.path.clone())
        .collect();
    add_block_devices(
        &parsed.blockdevices,
        false,
        "",
        false,
        false,
        volumes,
        &mut seen,
        &mut seen_paths,
    );
}

/// Enumerate system volumes, mounted user volumes, GVfs devices, and removable
/// UDisks block devices. Nothing is mounted or unlocked during discovery.
pub fn discover() -> Result<Vec<Volume>, String> {
    let mut volumes = system_volumes();
    merge_volumes(&mut volumes, mounted_volumes());
    merge_volumes(&mut volumes, gio_volumes());
    add_lsblk_volumes(&mut volumes);
    volumes.sort_by(|a, b| {
        b.system
            .cmp(&a.system)
            .then_with(|| {
                let a_root = a.path.as_deref() == Some(Path::new("/"));
                let b_root = b.path.as_deref() == Some(Path::new("/"));
                b_root.cmp(&a_root)
            })
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    Ok(volumes)
}

pub fn mounted_path(device: &str) -> Option<PathBuf> {
    for _ in 0..20 {
        if let Ok(volumes) = discover() {
            if let Some(path) = volumes
                .into_iter()
                .find(|volume| volume.device == device)
                .and_then(|volume| volume.path)
            {
                return Some(path);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    None
}

pub fn capacity_text(volume: &Volume) -> String {
    if volume.total == 0 {
        return String::new();
    }
    if volume.path.is_none() {
        return format_size(volume.total);
    }
    format!(
        "{} free of {}",
        format_size(volume.free),
        format_size(volume.total)
    )
}

fn mount_entry_for(path: &Path) -> Option<(String, String)> {
    let text = fs::read_to_string("/proc/self/mounts").ok()?;
    let mut best: Option<(usize, String, String)> = None;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let device = fields.next()?;
        let mount = unescape_mount(fields.next()?);
        if path.starts_with(&mount) {
            let length = mount.trim_end_matches('/').len();
            if best
                .as_ref()
                .map(|(len, _, _)| length > *len)
                .unwrap_or(true)
            {
                best = Some((length, unescape_mount(device), mount));
            }
        }
    }
    best.map(|(_, device, mount)| (device, mount))
}

fn helper(binary: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .map_err(|error| format!("Could not run {binary}: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

pub fn mount(device: &str) -> Result<String, String> {
    if let Some(device) = device.strip_prefix("gio:") {
        if !which("gio") {
            return Err("GIO/GVfs is unavailable; this device cannot be mounted.".to_owned());
        }
        if let Some(uri) = device.strip_prefix("mtp:host=") {
            return helper("gio", &["mount", &format!("mtp://{uri}/")]);
        }
        if let Some(uri) = device.strip_prefix("gphoto2:host=") {
            return helper("gio", &["mount", &format!("gphoto2://{uri}/")]);
        }
        if let Some(uri) = device.strip_prefix("afc:host=") {
            return helper("gio", &["mount", &format!("afc://{uri}/")]);
        }
        return helper("gio", &["mount", "--device", device]);
    }
    if !device.starts_with("/dev/") {
        return Err("Refusing to mount an invalid device path.".to_owned());
    }
    if !which("udisksctl") {
        return Err(
            "UDisks2 is unavailable; install/enable udisks2 to mount removable drives.".to_owned(),
        );
    }
    helper("udisksctl", &["mount", "--block-device", device])
}

pub fn unlock(device: &str, passphrase: &str) -> Result<String, String> {
    if !device.starts_with("/dev/") {
        return Err("Refusing to unlock an invalid device path.".to_owned());
    }
    if passphrase.is_empty() {
        return Err("Enter the encrypted volume's passphrase.".to_owned());
    }
    if !which("udisksctl") {
        return Err(
            "UDisks2 is unavailable; install/enable udisks2 to unlock encrypted drives.".to_owned(),
        );
    }
    let mut key = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    key.push(format!(
        "filem-unlock-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = options
        .open(&key)
        .map_err(|error| format!("Could not securely prepare unlock request: {error}"))?;
    use std::io::Write;
    let write_result = file.write_all(passphrase.as_bytes());
    drop(file);
    let result = match write_result {
        Ok(()) => helper(
            "udisksctl",
            &[
                "unlock",
                "--block-device",
                device,
                "--key-file",
                key.to_string_lossy().as_ref(),
            ],
        ),
        Err(error) => Err(format!(
            "Could not securely prepare unlock request: {error}"
        )),
    };
    let cleanup = fs::remove_file(&key);
    if let Err(error) = cleanup {
        if result.is_ok() {
            return Err(format!(
                "Unlocked device, but could not remove temporary key file: {error}"
            ));
        }
    }
    result
}

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

pub fn unmount(path: &Path) -> Result<String, String> {
    if gvfs_root().is_some_and(|root| path.starts_with(root)) {
        if !which("gio") {
            return Err("GIO/GVfs is unavailable; cannot safely unmount this device.".to_owned());
        }
        let path = path.to_string_lossy().into_owned();
        return helper("gio", &["mount", "--unmount", &path]);
    }
    let (device, mount) = mount_entry_for(path)
        .ok_or_else(|| format!("{} is not a mounted volume.", path.display()))?;
    if mount == "/" || mount == crate::paths::home_dir().to_string_lossy() {
        return Err("Refusing to unmount the running system or the home directory.".to_owned());
    }
    if device.starts_with("/dev/") {
        if !which("udisksctl") {
            return Err("UDisks2 is unavailable; cannot safely unmount this device.".to_owned());
        }
        helper("udisksctl", &["unmount", "--block-device", &device])
    } else if which("gio") {
        helper("gio", &["mount", "--unmount", &mount])
    } else {
        Err("GIO is unavailable; cannot unmount this volume.".to_owned())
    }
}

pub fn eject(device: &str) -> Result<String, String> {
    if let Some(device) = device.strip_prefix("gio:") {
        if !which("gio") {
            return Err("GIO/GVfs is unavailable; this device cannot be ejected.".to_owned());
        }
        return helper("gio", &["mount", "--eject", "--device", device]);
    }
    if !device.starts_with("/dev/") {
        if !Path::new(device).is_absolute() || !which("gio") {
            return Err("GIO/GVfs is unavailable or the eject target is invalid.".to_owned());
        }
        return helper("gio", &["mount", "--eject", device]);
    }
    if !which("udisksctl") {
        return Err("UDisks2 is unavailable; cannot safely eject this device.".to_owned());
    }
    let output = Command::new("lsblk")
        .args(["--json", "--output", "NAME,PATH,TYPE,RM,TRAN,MOUNTPOINTS"])
        .output()
        .map_err(|error| format!("Cannot verify mounted partitions before ejecting: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Cannot verify mounted partitions before ejecting: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let parsed: Lsblk = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Could not inspect partitions before ejecting: {error}"))?;
    let root = parsed
        .blockdevices
        .iter()
        .find(|entry| entry.path.as_deref() == Some(device))
        .ok_or_else(|| format!("{device} is no longer available."))?;
    if !root.rm.unwrap_or(false) && root.tran.as_deref() != Some("usb") {
        return Err("Refusing to eject a device that is not identified as removable.".to_owned());
    }
    unmount_tree(root)?;
    helper("udisksctl", &["power-off", "--block-device", device])
}

fn unmount_tree(device: &BlockDevice) -> Result<(), String> {
    if let Some(children) = &device.children {
        for child in children {
            unmount_tree(child)?;
        }
    }
    let Some(path) = device.path.as_deref() else {
        return Ok(());
    };
    if device
        .mountpoints
        .as_ref()
        .is_some_and(|points| points.iter().any(Option::is_some))
    {
        helper("udisksctl", &["unmount", "--block-device", path])?;
    }
    Ok(())
}

fn validate_network_uri(uri: &str) -> Result<(), String> {
    let scheme = uri.split_once(':').map(|(scheme, _)| scheme).unwrap_or("");
    if !matches!(scheme, "smb" | "sftp" | "dav" | "davs" | "ftp" | "nfs") {
        return Err(
            "Use a supported network URI such as smb://, sftp://, dav://, or nfs://.".to_owned(),
        );
    }
    if uri.chars().any(char::is_control)
        || uri
            .split_once("://")
            .and_then(|(_, authority)| authority.split('/').next())
            .is_some_and(|authority| authority.contains('@'))
    {
        return Err("Enter the server address without embedded credentials or control characters; use the system authentication prompt.".to_owned());
    }
    Ok(())
}

pub fn mount_network(uri: &str) -> Result<String, String> {
    validate_network_uri(uri)?;
    if !which("gio") {
        return Err("GIO/GVfs is unavailable; network locations cannot be mounted.".to_owned());
    }
    helper("gio", &["mount", uri])
}

fn which(binary: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(binary).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_escapes_are_decoded() {
        assert_eq!(
            unescape_mount("/run/media/me/My\\040Disk"),
            "/run/media/me/My Disk"
        );
        assert_eq!(unescape_mount("/tmp/plain"), "/tmp/plain");
    }

    #[test]
    fn root_and_irrelevant_mounts_are_hidden() {
        assert!(!is_relevant_mount("/", "ext4"));
        assert!(!is_relevant_mount("/nix/store", "ext4"));
        assert!(!is_relevant_mount("/run/user/1000", "tmpfs"));
        assert!(!is_relevant_mount("/run/user/1000/doc", "fuse.portal"));
        assert!(!is_relevant_mount("/run/user/1000/gvfs", "fuse.gvfsd-fuse"));
        assert!(!is_relevant_mount("/mnt/scratch", "tmpfs"));
        assert!(!is_relevant_mount("/mnt/scratch", "devtmpfs"));
        assert!(is_relevant_mount("/run/media/me/usb", "vfat"));
        assert!(is_relevant_mount("/mnt/nas", "nfs4"));
    }

    #[test]
    fn system_root_and_boot_are_detected_from_mounts() {
        let mounts = "\
proc /proc proc rw 0 0
sysfs /sys sysfs rw 0 0
tmpfs /run tmpfs rw 0 0
/dev/nvme0n1p2 / ext4 rw 0 0
/dev/nvme0n1p1 /boot vfat rw 0 0
/dev/sdb1 /run/media/me/USB vfat rw 0 0
";
        let volumes = parse_system_volumes(mounts, "NixOS");
        assert_eq!(volumes.len(), 2);

        let root = &volumes[0];
        assert_eq!(root.path.as_deref(), Some(Path::new("/")));
        assert_eq!(root.device, "/dev/nvme0n1p2");
        assert_eq!(root.label, "NixOS");
        assert_eq!(root.filesystem, "ext4");
        assert!(root.system && !root.removable && !root.network);
        assert!(!root.can_unmount() && !root.can_eject());

        let boot = &volumes[1];
        assert_eq!(boot.path.as_deref(), Some(Path::new("/boot")));
        assert_eq!(boot.label, "Boot");
        assert!(boot.system && !boot.removable);
        assert!(!boot.can_unmount() && !boot.can_eject());
    }

    #[test]
    fn pseudo_filesystems_are_never_system_volumes() {
        let mounts = "\
tmpfs / tmpfs rw 0 0
proc /proc proc rw 0 0
devtmpfs /dev devtmpfs rw 0 0
cgroup2 /sys/fs/cgroup cgroup2 rw 0 0
";
        assert!(parse_system_volumes(mounts, "NixOS").is_empty());
        assert!(is_pseudo_filesystem("tmpfs"));
        assert!(is_pseudo_filesystem("cgroup2"));
        assert!(!is_pseudo_filesystem("ext4"));
        assert!(!is_pseudo_filesystem("btrfs"));
    }

    #[test]
    fn overmounted_root_uses_the_last_mount_entry() {
        let mounts = "/dev/sda2 / ext4 rw 0 0\noverlay / overlay rw 0 0\n";
        let volumes = parse_system_volumes(mounts, "NixOS");
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].device, "overlay");
    }

    #[test]
    fn os_release_name_prefers_the_short_name() {
        let text = "\
# comment
PRETTY_NAME=\"NixOS 24.11 (Vicuna)\"
NAME=\"NixOS\"
";
        assert_eq!(os_release_name(text).as_deref(), Some("NixOS"));
        assert_eq!(
            os_release_name("PRETTY_NAME=\"Example Linux\"\n").as_deref(),
            Some("Example Linux")
        );
        assert_eq!(os_release_name(""), None);
    }

    #[test]
    fn lsblk_does_not_duplicate_a_system_mount_path() {
        let parsed: Lsblk = serde_json::from_str(
            r#"{"blockdevices":[{"name":"nvme0n1p1","path":"/dev/nvme0n1p1","type":"part","size":1000,"fstype":"vfat","label":"EFI","mountpoints":["/boot"]}]}"#,
        )
        .unwrap();
        // The mount table identifies /boot through a by-uuid path, so device
        // matching alone would not catch the lsblk duplicate.
        let mut volumes = parse_system_volumes(
            "/dev/nvme0n1p2 / ext4 rw 0 0\n/dev/disk/by-uuid/BOOT /boot vfat rw 0 0\n",
            "NixOS",
        );
        let mut seen = volumes.iter().map(|volume| volume.device.clone()).collect();
        let mut seen_paths = volumes
            .iter()
            .filter_map(|volume| volume.path.clone())
            .collect();
        add_block_devices(
            &parsed.blockdevices,
            false,
            "",
            false,
            false,
            &mut volumes,
            &mut seen,
            &mut seen_paths,
        );
        let boot_entries: Vec<_> = volumes
            .iter()
            .filter(|volume| volume.path == Some(PathBuf::from("/boot")))
            .collect();
        assert_eq!(boot_entries.len(), 1);
        assert!(boot_entries[0].system);
    }

    #[test]
    fn gvfs_mount_labels_identify_devices() {
        assert_eq!(gvfs_label("mtp:host=Pixel_7"), "Phone — Pixel_7");
        assert_eq!(gvfs_label("gphoto2:host=usb:001,002"), "Camera — usb:001");
    }

    fn mtp_info() -> GioVolumeInfo {
        GioVolumeInfo {
            name: "CPH2387".to_owned(),
            unix_device: Some("/dev/bus/usb/003/016".to_owned()),
            activation_uri: Some("mtp://OPPO_CPH2387_SERIAL/".to_owned()),
            can_mount: true,
            ..GioVolumeInfo::default()
        }
    }

    #[test]
    fn gio_maps_unmounted_mtp_devices_for_user_activated_mounting() {
        let volume = gio_info_to_volume(mtp_info()).expect("MTP volume");
        assert_eq!(volume.device, "gio:mtp:host=OPPO_CPH2387_SERIAL");
        assert_eq!(volume.filesystem, "Phone");
        assert!(volume.removable && !volume.network);
        assert!(volume.path.is_none());
        assert!(volume.eject_device.is_empty());
    }

    #[test]
    fn gio_maps_camera_apple_and_network_devices() {
        let camera = gio_info_to_volume(GioVolumeInfo {
            name: "Camera".to_owned(),
            activation_uri: Some("gphoto2://usb:001,002/".to_owned()),
            can_mount: true,
            ..GioVolumeInfo::default()
        })
        .expect("camera volume");
        assert_eq!(camera.filesystem, "Camera");
        assert_eq!(camera.device, "gio:gphoto2:host=usb:001,002");

        let apple = gio_info_to_volume(GioVolumeInfo {
            name: "Phone".to_owned(),
            activation_uri: Some("afc://SERIAL/".to_owned()),
            can_mount: true,
            ..GioVolumeInfo::default()
        })
        .expect("apple volume");
        assert_eq!(apple.filesystem, "Apple device");

        let network = gio_info_to_volume(GioVolumeInfo {
            name: "media on nas".to_owned(),
            activation_uri: Some("smb://nas/media".to_owned()),
            gvfs_source: Some("smb-share:server=nas,share=media".to_owned()),
            path: Some(PathBuf::from(
                "/run/user/1000/gvfs/smb-share:server=nas,share=media",
            )),
            can_mount: false,
            ..GioVolumeInfo::default()
        })
        .expect("network volume");
        assert_eq!(network.device, "gio:smb-share:server=nas,share=media");
        assert!(network.network && !network.removable);
    }

    #[test]
    fn gio_keeps_devices_when_optional_properties_are_missing() {
        // No UUID, Unix device, or label: identity comes from the activation root.
        let volume = gio_info_to_volume(GioVolumeInfo {
            activation_uri: Some("mtp://SERIAL/".to_owned()),
            can_mount: true,
            ..GioVolumeInfo::default()
        })
        .expect("volume without optional properties");
        assert_eq!(volume.device, "gio:mtp:host=SERIAL");
        assert_eq!(volume.label, "Phone — SERIAL");

        // No activation root either, but a mounted GVfs directory still yields a
        // stable identity and label.
        let mounted = gio_info_to_volume(GioVolumeInfo {
            gvfs_source: Some("mtp:host=SERIAL".to_owned()),
            path: Some(PathBuf::from("/run/user/1000/gvfs/mtp:host=SERIAL")),
            ..GioVolumeInfo::default()
        })
        .expect("mounted volume without activation root");
        assert_eq!(mounted.device, "gio:mtp:host=SERIAL");
        assert_eq!(mounted.label, "Phone — SERIAL");
    }

    #[test]
    fn gio_ignores_block_devices_and_unusable_volumes() {
        // Local block devices come from lsblk, not GIO, to avoid duplicates.
        assert!(gio_info_to_volume(GioVolumeInfo {
            name: "disk".to_owned(),
            unix_device: Some("/dev/sdb1".to_owned()),
            can_mount: true,
            ..GioVolumeInfo::default()
        })
        .is_none());

        // Recognised but neither mounted nor mountable: nothing useful to show.
        assert!(gio_info_to_volume(GioVolumeInfo {
            activation_uri: Some("mtp://SERIAL/".to_owned()),
            can_mount: false,
            ..GioVolumeInfo::default()
        })
        .is_none());
    }

    fn test_volume(device: &str, path: Option<&str>) -> Volume {
        Volume {
            path: path.map(PathBuf::from),
            device: device.to_owned(),
            eject_device: String::new(),
            filesystem: "GVfs".to_owned(),
            label: device.to_owned(),
            total: 0,
            free: 0,
            removable: true,
            network: false,
            encrypted: false,
            locked: false,
            usb_flash: false,
            system: false,
        }
    }

    #[test]
    fn system_volumes_never_offer_unmount_or_eject_actions() {
        let volumes = parse_system_volumes("/dev/nvme0n1p2 / ext4 rw 0 0\n", "NixOS");
        let root = volumes.first().expect("root volume");
        assert!(root.system);
        assert!(!root.can_unmount());
        assert!(!root.can_eject());

        let mut removable = test_volume(
            "gio:mtp:host=SERIAL",
            Some("/run/user/1000/gvfs/mtp:host=SERIAL"),
        );
        removable.eject_device = "gio:mtp:host=SERIAL".to_owned();
        assert!(removable.can_unmount());
        assert!(removable.can_eject());

        removable.removable = false;
        assert!(!removable.can_eject());
    }

    #[test]
    fn gio_duplicates_collapse_and_mounted_entries_win() {
        let deduped = dedup_gio_volumes(vec![
            test_volume("gio:mtp:host=SERIAL", None),
            test_volume(
                "gio:mtp:host=SERIAL",
                Some("/run/user/1000/gvfs/mtp:host=SERIAL"),
            ),
        ]);
        assert_eq!(deduped.len(), 1);
        assert_eq!(
            deduped[0].path,
            Some(PathBuf::from("/run/user/1000/gvfs/mtp:host=SERIAL"))
        );

        // The mounted copy also wins when it is seen first.
        let deduped = dedup_gio_volumes(vec![
            test_volume(
                "gio:mtp:host=SERIAL",
                Some("/run/user/1000/gvfs/mtp:host=SERIAL"),
            ),
            test_volume("gio:mtp:host=SERIAL", None),
        ]);
        assert_eq!(deduped.len(), 1);
        assert!(deduped[0].path.is_some());
    }

    #[test]
    fn discovery_merges_sources_without_duplicate_devices_or_paths() {
        let mut volumes = vec![test_volume(
            "gio:smb-share:server=nas,share=media",
            Some("/run/user/1000/gvfs/smb-share:server=nas,share=media"),
        )];
        merge_volumes(
            &mut volumes,
            vec![
                // Same device as the /proc mount: skipped.
                test_volume(
                    "gio:smb-share:server=nas,share=media",
                    Some("/run/user/1000/gvfs/smb-share:server=nas,share=media"),
                ),
                // Different identity but the same mount path: skipped.
                test_volume(
                    "gio:smb:host=nas",
                    Some("/run/user/1000/gvfs/smb-share:server=nas,share=media"),
                ),
                // A genuinely new device is kept.
                test_volume("gio:mtp:host=SERIAL", None),
            ],
        );
        assert_eq!(volumes.len(), 2);
        assert!(volumes
            .iter()
            .any(|volume| volume.device == "gio:mtp:host=SERIAL"));
    }

    #[test]
    fn unavailable_gio_worker_yields_no_devices() {
        assert!(gio_volumes_from(None).is_empty());

        let (requests, receiver) = mpsc::channel();
        drop(receiver);
        let worker = GioDiscovery { requests };
        assert!(gio_volumes_from(Some(&worker)).is_empty());
    }

    #[test]
    fn parses_lsblk_shape_and_encrypted_device_metadata() {
        let parsed: Lsblk = serde_json::from_str(
            r#"{"blockdevices":[{"name":"sdb","path":"/dev/sdb","type":"disk","rm":true,"tran":"usb","size":8000000,"children":[{"name":"sdb1","path":"/dev/sdb1","type":"part","rm":true,"size":4000000,"fstype":"crypto_LUKS","label":"vault","mountpoints":[null]}]}]}"#,
        )
        .unwrap();
        let mut volumes = Vec::new();
        add_block_devices(
            &parsed.blockdevices,
            false,
            "",
            false,
            false,
            &mut volumes,
            &mut HashSet::new(),
            &mut HashSet::new(),
        );
        assert_eq!(volumes.len(), 1);
        assert!(volumes[0].encrypted && volumes[0].locked);
        assert_eq!(volumes[0].label, "vault");
        assert!(volumes[0].removable);
        assert_eq!(volumes[0].eject_device, "/dev/sdb");
    }

    #[test]
    fn distinguishes_usb_flash_drives_from_usb_disk_enclosures() {
        let parsed: Lsblk = serde_json::from_str(
            r#"{"blockdevices":[
                {"name":"sdb","path":"/dev/sdb","type":"disk","rm":true,"tran":"usb","children":[
                    {"name":"sdb1","path":"/dev/sdb1","type":"part","size":4000000,"fstype":"exfat","label":"stick","mountpoints":["/run/media/user/stick"]}
                ]},
                {"name":"sdc","path":"/dev/sdc","type":"disk","rm":false,"tran":"usb","children":[
                    {"name":"sdc1","path":"/dev/sdc1","type":"part","size":4000000000,"fstype":"ext4","label":"archive","mountpoints":["/run/media/user/archive"]}
                ]}
            ]}"#,
        )
        .unwrap();
        let mut volumes = Vec::new();
        add_block_devices(
            &parsed.blockdevices,
            false,
            "",
            false,
            false,
            &mut volumes,
            &mut HashSet::new(),
            &mut HashSet::new(),
        );
        let stick = volumes
            .iter()
            .find(|volume| volume.label == "stick")
            .expect("flash drive");
        let archive = volumes
            .iter()
            .find(|volume| volume.label == "archive")
            .expect("external disk");
        assert!(stick.removable && stick.usb_flash);
        assert!(archive.removable && !archive.usb_flash);
    }

    #[test]
    fn unlocked_luks_uses_mapper_for_mount_and_never_ejects_internal_storage() {
        let parsed: Lsblk = serde_json::from_str(
            r#"{"blockdevices":[{"name":"nvme0n1p3","path":"/dev/nvme0n1p3","type":"part","rm":false,"size":4000000,"fstype":"crypto_LUKS","label":"system","children":[{"name":"dm-0","path":"/dev/dm-0","type":"crypt","size":3900000,"fstype":"ext4","mountpoints":[null]}]}]}"#,
        )
        .unwrap();
        let mut volumes = Vec::new();
        add_block_devices(
            &parsed.blockdevices,
            false,
            "",
            false,
            false,
            &mut volumes,
            &mut HashSet::new(),
            &mut HashSet::new(),
        );
        assert_eq!(volumes.len(), 1);
        assert!(volumes[0].encrypted && !volumes[0].locked);
        assert_eq!(volumes[0].device, "/dev/dm-0");
        assert!(!volumes[0].removable);
        assert!(volumes[0].eject_device.is_empty());
    }

    #[test]
    fn internal_mounted_partitions_are_listed_but_root_is_hidden() {
        let parsed: Lsblk = serde_json::from_str(
            r#"{"blockdevices":[{"name":"nvme0n1","path":"/dev/nvme0n1","type":"disk","rm":false,"children":[{"name":"nvme0n1p1","path":"/dev/nvme0n1p1","type":"part","size":1000,"fstype":"vfat","label":"EFI","mountpoints":["/boot/efi"]},{"name":"nvme0n1p2","path":"/dev/nvme0n1p2","type":"part","size":2000,"fstype":"ext4","label":"system","mountpoints":["/nix/store","/"]},{"name":"nvme0n1p3","path":"/dev/nvme0n1p3","type":"part","size":3000,"fstype":"ext4","mountpoints":["/data"]}]}]}"#,
        )
        .unwrap();
        let mut volumes = Vec::new();
        add_block_devices(
            &parsed.blockdevices,
            false,
            "",
            false,
            false,
            &mut volumes,
            &mut HashSet::new(),
            &mut HashSet::new(),
        );
        assert_eq!(volumes.len(), 2);
        assert!(volumes.iter().any(|volume| volume.label == "EFI"));
        assert!(volumes
            .iter()
            .any(|volume| volume.path == Some(PathBuf::from("/data"))));
        assert!(!volumes
            .iter()
            .any(|volume| volume.path == Some(PathBuf::from("/"))));
    }

    #[test]
    fn network_uri_validation_rejects_embedded_credentials() {
        assert!(validate_network_uri("smb://server/share").is_ok());
        assert!(validate_network_uri("smb://user:secret@server/share").is_err());
        assert!(validate_network_uri("file:///etc").is_err());
    }

    #[test]
    fn unlock_requires_a_passphrase() {
        assert!(unlock("/dev/example", "")
            .unwrap_err()
            .contains("passphrase"));
    }

    #[test]
    fn listing_is_stable_and_capacity_is_readable() {
        let volumes = mounted_volumes();
        for volume in &volumes {
            assert!(!volume.label.is_empty());
            let text = capacity_text(volume);
            assert!(volume.total == 0 || !text.is_empty());
        }
    }
}
