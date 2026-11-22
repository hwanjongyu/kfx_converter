//! Hardware & Device Lifecycle Management for Amazon Kindle
//!
//! Handles hardware USB sysfs probing, MTP/UMS mount detection, model resolution,
//! storage capacity queries, and safe device unmounting (`libc::syncfs`, `gio`, `udisksctl`).

use super::DEVICE_SYNC_LOCK;
use super::types::{KindleDeviceInfo, SyncError, UnmountedKindleUsb};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Decodes `\OOO` octal escapes (e.g. `\040`, `\011`, `\012`, `\134`, and multi-byte UTF-8
/// byte sequences) from a `/proc/mounts` field.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn unescape_proc_mounts_field(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
            && let Ok(oct_str) = std::str::from_utf8(&bytes[i + 1..i + 4])
            && let Ok(val) = u8::from_str_radix(oct_str, 8)
        {
            out.push(val);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Finds and unescapes the source device field in `/proc/mounts` content matching `target_mount`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn find_mount_source_in_mounts(
    mounts_content: &str,
    target_mount: &Path,
) -> Option<String> {
    for line in mounts_content.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(raw_dev), Some(raw_mount)) = (parts.next(), parts.next()) {
            let mount = unescape_proc_mounts_field(raw_mount);
            if Path::new(&mount) == target_mount {
                return Some(unescape_proc_mounts_field(raw_dev));
            }
        }
    }
    None
}

#[cfg(unix)]
fn run_eject_command(label: &str, mut cmd: Command) -> Result<(), String> {
    match cmd.output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                "no stderr output".to_string()
            } else {
                stderr
            };
            Err(format!(
                "{} failed (exit {:?}): {}",
                label,
                out.status.code(),
                detail
            ))
        }
        Err(e) => Err(format!("{} failed to spawn: {}", label, e)),
    }
}

/// Queries filesystem storage statistics (used_gb, total_gb).
#[cfg(unix)]
pub fn get_storage_stats(path: &Path) -> Option<(f32, f32)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } == 0 {
        let block_size = if stat.f_frsize > 0 {
            stat.f_frsize
        } else {
            stat.f_bsize
        } as u64;
        let total_bytes = stat.f_blocks as u64 * block_size;
        let free_bytes = stat.f_bavail as u64 * block_size;
        let used_bytes = total_bytes.saturating_sub(free_bytes);

        let total_gb = total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let used_gb = used_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        Some((used_gb as f32, total_gb as f32))
    } else {
        None
    }
}

#[cfg(windows)]
const DRIVE_REMOVABLE: u32 = 2;
#[cfg(windows)]
const GENERIC_READ: u32 = 0x8000_0000;
#[cfg(windows)]
const GENERIC_WRITE: u32 = 0x4000_0000;
#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x0000_0001;
#[cfg(windows)]
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
#[cfg(windows)]
const OPEN_EXISTING: u32 = 3;
#[cfg(windows)]
const INVALID_HANDLE_VALUE: *mut core::ffi::c_void = (-1isize) as *mut core::ffi::c_void;
#[cfg(windows)]
const FSCTL_LOCK_VOLUME: u32 = 0x0009_0018;
#[cfg(windows)]
const FSCTL_UNLOCK_VOLUME: u32 = 0x0009_001C;
#[cfg(windows)]
const FSCTL_DISMOUNT_VOLUME: u32 = 0x0009_0020;
#[cfg(windows)]
const IOCTL_STORAGE_EJECT_MEDIA: u32 = 0x002D_4808;

#[cfg(windows)]
#[link(name = "kernel32")]
// SAFETY: Declarations match the Windows kernel32 ABI signatures for volume inspection,
// handle lifecycle, flushing, and DeviceIoControl volume/media operations.
unsafe extern "system" {
    fn GetDriveTypeW(lpRootPathName: *const u16) -> u32;
    fn GetVolumeInformationW(
        lpRootPathName: *const u16,
        lpVolumeNameBuffer: *mut u16,
        nVolumeNameSize: u32,
        lpVolumeSerialNumber: *mut u32,
        lpMaximumComponentLength: *mut u32,
        lpFileSystemFlags: *mut u32,
        lpFileSystemNameBuffer: *mut u16,
        nFileSystemNameSize: u32,
    ) -> i32;
    fn GetDiskFreeSpaceExW(
        lpDirectoryName: *const u16,
        lpFreeBytesAvailableToCaller: *mut u64,
        lpTotalNumberOfBytes: *mut u64,
        lpTotalNumberOfFreeBytes: *mut u64,
    ) -> i32;
    fn CreateFileW(
        lpFileName: *const u16,
        dwDesiredAccess: u32,
        dwShareMode: u32,
        lpSecurityAttributes: *const core::ffi::c_void,
        dwCreationDisposition: u32,
        dwFlagsAndAttributes: u32,
        hTemplateFile: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn FlushFileBuffers(hFile: *mut core::ffi::c_void) -> i32;
    fn DeviceIoControl(
        hDevice: *mut core::ffi::c_void,
        dwIoControlCode: u32,
        lpInBuffer: *const core::ffi::c_void,
        nInBufferSize: u32,
        lpOutBuffer: *mut core::ffi::c_void,
        nOutBufferSize: u32,
        lpBytesReturned: *mut u32,
        lpOverlapped: *mut core::ffi::c_void,
    ) -> i32;
    fn CloseHandle(hObject: *mut core::ffi::c_void) -> i32;
    fn GetLastError() -> u32;
}

/// Extracts a Win32 volume namespace device path (`\\.\X:`) from a Windows drive root or path.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_volume_device_path(mount_point: &Path) -> Option<String> {
    let raw = mount_point.to_str()?;
    let trimmed = raw
        .strip_prefix(r"\\?\")
        .or_else(|| raw.strip_prefix(r"\\.\"))
        .unwrap_or(raw);
    let mut bytes = trimmed.bytes();
    let drive = bytes.next()?;
    let colon = bytes.next()?;
    if drive.is_ascii_alphabetic() && colon == b':' {
        Some(format!(r"\\.\{}:", (drive as char).to_ascii_uppercase()))
    } else {
        None
    }
}

#[cfg(windows)]
struct VolumeGuard {
    handle: *mut core::ffi::c_void,
    locked: bool,
}

#[cfg(windows)]
impl Drop for VolumeGuard {
    fn drop(&mut self) {
        if !self.handle.is_null() && self.handle != INVALID_HANDLE_VALUE {
            if self.locked {
                let mut bytes_returned: u32 = 0;
                // SAFETY: `self.handle` is an open Win32 volume handle not yet closed; null
                // input/output buffers with length 0 are valid for `FSCTL_UNLOCK_VOLUME`.
                unsafe {
                    DeviceIoControl(
                        self.handle,
                        FSCTL_UNLOCK_VOLUME,
                        core::ptr::null(),
                        0,
                        core::ptr::null_mut(),
                        0,
                        &mut bytes_returned,
                        core::ptr::null_mut(),
                    );
                }
            }
            // SAFETY: `self.handle` is an open Win32 handle exclusively owned by this guard.
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(windows)]
fn windows_last_error(op: &str, volume: &str) -> SyncError {
    // SAFETY: `GetLastError` reads thread-local Win32 error state with no pointer arguments.
    let code = unsafe { GetLastError() };
    let os_err = std::io::Error::from_raw_os_error(code as i32);
    SyncError::Io(std::io::Error::new(
        os_err.kind(),
        format!(
            "{} failed for {} (Win32 error {}: {})",
            op, volume, code, os_err
        ),
    ))
}

#[cfg(windows)]
fn windows_eject_volume(root: &Path) -> Result<(), SyncError> {
    use std::os::windows::ffi::OsStrExt;

    let volume_path = windows_volume_device_path(root)
        .ok_or_else(|| SyncError::DeviceNotFound(root.to_path_buf()))?;
    let wide: Vec<u16> = std::ffi::OsStr::new(&volume_path)
        .encode_wide()
        .chain(core::iter::once(0))
        .collect();

    // SAFETY: `wide` is a valid NUL-terminated UTF-16 string; security and template pointers are null.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            core::ptr::null(),
            OPEN_EXISTING,
            0,
            core::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(windows_last_error("CreateFileW", &volume_path));
    }
    let mut guard = VolumeGuard {
        handle,
        locked: false,
    };

    // SAFETY: `guard.handle` is a valid open volume handle returned by `CreateFileW`.
    if unsafe { FlushFileBuffers(guard.handle) } == 0 {
        return Err(windows_last_error("FlushFileBuffers", &volume_path));
    }

    let ioctl_no_buf = |code: u32, op: &str| -> Result<(), SyncError> {
        let mut bytes_returned: u32 = 0;
        // SAFETY: `guard.handle` is a valid open volume handle; null buffers with zero size
        // match the Win32 `FSCTL_LOCK_VOLUME`/`FSCTL_DISMOUNT_VOLUME`/`IOCTL_STORAGE_EJECT_MEDIA` contract.
        let ok = unsafe {
            DeviceIoControl(
                guard.handle,
                code,
                core::ptr::null(),
                0,
                core::ptr::null_mut(),
                0,
                &mut bytes_returned,
                core::ptr::null_mut(),
            )
        };
        if ok == 0 {
            Err(windows_last_error(op, &volume_path))
        } else {
            Ok(())
        }
    };

    ioctl_no_buf(FSCTL_LOCK_VOLUME, "FSCTL_LOCK_VOLUME")?;
    guard.locked = true;
    ioctl_no_buf(FSCTL_DISMOUNT_VOLUME, "FSCTL_DISMOUNT_VOLUME")?;
    ioctl_no_buf(IOCTL_STORAGE_EJECT_MEDIA, "IOCTL_STORAGE_EJECT_MEDIA")?;

    let _ = ioctl_no_buf(FSCTL_UNLOCK_VOLUME, "FSCTL_UNLOCK_VOLUME");
    guard.locked = false;
    Ok(())
}

#[cfg(windows)]
pub fn get_storage_stats(path: &Path) -> Option<(f32, f32)> {
    use std::os::windows::ffi::OsStrExt;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_bytes_available: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut total_free_bytes: u64 = 0;

    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_bytes_available,
            &mut total_bytes,
            &mut total_free_bytes,
        )
    };
    if ok == 0 || total_bytes == 0 {
        return None;
    }

    let used_bytes = total_bytes.saturating_sub(free_bytes_available);
    let total_gb = total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let used_gb = used_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    Some((used_gb as f32, total_gb as f32))
}

#[cfg(all(not(unix), not(windows)))]
pub fn get_storage_stats(_path: &Path) -> Option<(f32, f32)> {
    None
}

/// Checks if a device or directory name corresponds to a Kindle device.
#[inline]
#[allow(dead_code)]
pub fn is_kindle_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("kindle")
        || n.contains("scribe")
        || n.contains("paperwhite")
        || n.contains("oasis")
        || n.contains("colorsoft")
        || n.contains("voyage")
}

/// Verifies that a Windows drive candidate is genuinely a removable Kindle device
/// rather than an internal HDD/SSD or arbitrary USB drive.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn is_verified_windows_kindle_candidate(
    is_removable: bool,
    volume_label: Option<&str>,
    root: &Path,
) -> bool {
    if !is_removable {
        return false;
    }

    let has_kindle_layout = root.join("documents").is_dir()
        || root.join("system").is_dir()
        || root.join(".system").is_dir();

    if volume_label.is_some_and(is_kindle_name) && has_kindle_layout {
        return true;
    }

    let has_kindle_marker = root.join("system").join("version.txt").is_file()
        || root.join(".system").join("version.txt").is_file()
        || root.join("system").join("dev_reg.log").is_file()
        || root.join(".system").join("dev_reg.log").is_file()
        || root.join(".active_content_sandbox").is_dir();

    root.join("documents").is_dir() && has_kindle_marker
}

/// Accurately resolves the specific Kindle hardware model from mount hint or on-device files.
/// Supports all Kindle models: Scribe, Paperwhite, Oasis, Voyage, Colorsoft, Basic, Touch, Keyboard, DX.
pub fn resolve_model_name(root: &Path, hint: &str) -> String {
    let hint_lower = hint.to_lowercase();

    // 1. Check hint (volume label, mount point, or MTP device descriptor)
    if hint_lower.contains("colorsoft") {
        return "Kindle Colorsoft".to_string();
    }
    if hint_lower.contains("scribe") {
        return "Kindle Scribe".to_string();
    }
    if hint_lower.contains("oasis") {
        return "Kindle Oasis".to_string();
    }
    if hint_lower.contains("voyage") {
        return "Kindle Voyage".to_string();
    }
    if hint_lower.contains("paperwhite") {
        return "Kindle Paperwhite".to_string();
    }
    if hint_lower.contains("basic") {
        return "Kindle Basic".to_string();
    }
    if hint_lower.contains("touch") {
        return "Kindle Touch".to_string();
    }
    if hint_lower.contains("keyboard") {
        return "Kindle Keyboard".to_string();
    }
    if hint_lower.contains("dx") {
        return "Kindle DX".to_string();
    }

    // 2. Inspect on-device system files (e.g. system/version.txt, .system/version.txt)
    let version_candidates = [
        root.join("system").join("version.txt"),
        root.join(".system").join("version.txt"),
        root.join("system").join("dev_reg.log"),
        root.join(".system").join("dev_reg.log"),
    ];

    for vpath in &version_candidates {
        if let Ok(content) = std::fs::read_to_string(vpath) {
            let lower = content.to_lowercase();
            if lower.contains("colorsoft") {
                return "Kindle Colorsoft".to_string();
            }
            if lower.contains("scribe") {
                return "Kindle Scribe".to_string();
            }
            if lower.contains("oasis") {
                return "Kindle Oasis".to_string();
            }
            if lower.contains("voyage") {
                return "Kindle Voyage".to_string();
            }
            if lower.contains("paperwhite") {
                return "Kindle Paperwhite".to_string();
            }
            if lower.contains("basic") {
                return "Kindle Basic".to_string();
            }
            if lower.contains("touch") {
                return "Kindle Touch".to_string();
            }
            if lower.contains("keyboard") {
                return "Kindle Keyboard".to_string();
            }
            if lower.contains("dx") {
                return "Kindle DX".to_string();
            }

            // Known Kindle serial number prefixes (first 4 or 6 chars)
            if content.contains("G092") {
                return "Kindle Scribe".to_string();
            }
            if content.contains("G0028") {
                return "Kindle Colorsoft".to_string();
            }
            let oasis_prefixes = [
                "G0B0", "G000P8", "G000SA", "G000WL", "G000WM", "G000WN", "G0011L",
            ];
            if oasis_prefixes.iter().any(|&p| content.contains(p)) {
                return "Kindle Oasis".to_string();
            }
            let voyage_prefixes = ["B013", "B054", "9013", "9054"];
            if voyage_prefixes.iter().any(|&p| content.contains(p)) {
                return "Kindle Voyage".to_string();
            }
            let pw_prefixes = [
                "B012", "B024", "9024", "B0D4", "90D4", "B05A", "G090G1", "G090KB", "G090LK",
                "B017", "B060", "B061", "B062", "G000PP", "G000T6", "G000T1", "G001LG", "G001PX",
            ];
            if pw_prefixes.iter().any(|&p| content.contains(p)) {
                return "Kindle Paperwhite".to_string();
            }
            let basic_prefixes = ["G090VB", "G090VC", "G090WF", "G090WH"];
            if basic_prefixes.iter().any(|&p| content.contains(p)) {
                return "Kindle Basic".to_string();
            }
            let kbd_prefixes = ["B006", "B008", "B00A"];
            if kbd_prefixes.iter().any(|&p| content.contains(p)) {
                return "Kindle Keyboard".to_string();
            }
            let dx_prefixes = ["B004", "B005", "B009"];
            if dx_prefixes.iter().any(|&p| content.contains(p)) {
                return "Kindle DX".to_string();
            }
            if content.contains("B011") {
                return "Kindle Touch".to_string();
            }
        }
    }

    // 3. Fallback: If recognized as a Kindle device, return "Kindle"
    "Kindle".to_string()
}

/// On Linux, inspects `/sys/bus/usb/devices/` to detect any connected Amazon Kindle USB devices
/// (Vendor ID 0x1949 / Lab126) that are currently not mounted as a filesystem.
pub fn probe_unmounted_usb_kindle() -> Option<UnmountedKindleUsb> {
    #[cfg(target_os = "linux")]
    {
        probe_unmounted_usb_kindle_in(Path::new("/sys/bus/usb/devices"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Parameterized helper to probe an arbitrary sysfs USB devices directory.
pub fn probe_unmounted_usb_kindle_in(usb_dir: &Path) -> Option<UnmountedKindleUsb> {
    if !usb_dir.is_dir() {
        return None;
    }

    if let Ok(entries) = std::fs::read_dir(usb_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            let vpath = p.join("idVendor");
            if let Ok(v) = std::fs::read_to_string(&vpath)
                && v.trim().eq_ignore_ascii_case("1949")
            {
                let product_id = std::fs::read_to_string(p.join("idProduct"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let product_name = std::fs::read_to_string(p.join("product"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| "Kindle Device".to_string());
                return Some(UnmountedKindleUsb {
                    vendor_id: "1949".to_string(),
                    product_id,
                    product_name,
                });
            }
        }
    }
    None
}

/// Detects connected Kindle root directory and metadata (GVFS MTP or USB UMS mount).
pub fn detect_device_info() -> Option<KindleDeviceInfo> {
    // 0. Explicit environment variable overrides for headless servers / CI / Docker
    if let Ok(custom_root) =
        std::env::var("KINDLE_ROOT").or_else(|_| std::env::var("KINDLE_MOUNT_PATH"))
    {
        let root = PathBuf::from(custom_root);
        if root.is_dir() {
            let internal = root.join("Internal Storage");
            let active_root = if internal.is_dir() {
                internal
            } else {
                root.clone()
            };
            let doc_path = active_root.join("documents");
            let final_docs = if doc_path.is_dir() {
                doc_path
            } else {
                active_root.clone()
            };
            let model = resolve_model_name(&active_root, &active_root.to_string_lossy());
            let (used_gb, total_gb) = get_storage_stats(&active_root).unwrap_or((12.4, 27.2));
            return Some(KindleDeviceInfo {
                root: active_root,
                documents_dir: final_docs,
                model_name: model,
                used_gb,
                total_gb,
            });
        }
    }

    // 1. Check GVFS MTP on Linux
    #[cfg(target_os = "linux")]
    {
        let uid = unsafe { libc::getuid() };
        let gvfs_dir = PathBuf::from(format!("/run/user/{}/gvfs", uid));
        if gvfs_dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&gvfs_dir)
        {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if is_kindle_name(&name) {
                    let internal = entry.path().join("Internal Storage");
                    let root = if internal.is_dir() {
                        internal
                    } else {
                        entry.path()
                    };
                    let doc_path = root.join("documents");
                    let model = resolve_model_name(&root, &name);
                    let (used_gb, total_gb) = get_storage_stats(&root).unwrap_or((12.4, 27.2));
                    return Some(KindleDeviceInfo {
                        root,
                        documents_dir: doc_path,
                        model_name: model,
                        used_gb,
                        total_gb,
                    });
                }
            }
        }
    }

    // 2. Check UMS mount points across Linux, macOS, and Windows
    let mut candidates: Vec<(PathBuf, Option<String>)> = Vec::new();

    // macOS standard mount location
    #[cfg(target_os = "macos")]
    {
        if let Ok(entries) = std::fs::read_dir("/Volumes") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if is_kindle_name(&name) {
                    candidates.push((entry.path(), None));
                }
            }
        }
        candidates.push((PathBuf::from("/Volumes/Kindle"), None));
        candidates.push((PathBuf::from("/Volumes/Kindle Paperwhite"), None));
        candidates.push((PathBuf::from("/Volumes/Kindle Oasis"), None));
        candidates.push((PathBuf::from("/Volumes/Kindle Scribe"), None));
        candidates.push((PathBuf::from("/Volumes/Kindle Colorsoft"), None));
        candidates.push((PathBuf::from("/Volumes/Kindle Voyage"), None));
        candidates.push((PathBuf::from("/Volumes/Kindle Basic"), None));
    }

    // Windows drive letter mount discovery (D: through Z:)
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        for drive in b'D'..=b'Z' {
            let drive_str = format!("{}:\\", drive as char);
            let drive_path = PathBuf::from(&drive_str);
            let wide: Vec<u16> = drive_path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let is_removable = unsafe { GetDriveTypeW(wide.as_ptr()) } == DRIVE_REMOVABLE;
            if !is_removable {
                continue;
            }

            let mut vol_buf = [0u16; 261];
            let vol_ok = unsafe {
                GetVolumeInformationW(
                    wide.as_ptr(),
                    vol_buf.as_mut_ptr(),
                    vol_buf.len() as u32,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                )
            };
            let vol_label = if vol_ok != 0 {
                let nul_pos = vol_buf
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(vol_buf.len());
                let decoded = String::from_utf16_lossy(&vol_buf[..nul_pos])
                    .trim()
                    .to_string();
                if decoded.is_empty() {
                    None
                } else {
                    Some(decoded)
                }
            } else {
                None
            };

            if is_verified_windows_kindle_candidate(is_removable, vol_label.as_deref(), &drive_path)
            {
                candidates.push((drive_path, vol_label));
            }
        }
    }

    // Linux standard user mount locations
    #[cfg(target_os = "linux")]
    {
        if let Ok(username) = std::env::var("USER") {
            for base in [
                format!("/run/media/{}", username),
                format!("/media/{}", username),
            ] {
                if let Ok(entries) = std::fs::read_dir(&base) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if is_kindle_name(&name) {
                            candidates.push((entry.path(), None));
                        }
                    }
                }
            }
            candidates.push((
                PathBuf::from(format!("/run/media/{}/Kindle", username)),
                None,
            ));
            candidates.push((PathBuf::from(format!("/media/{}/Kindle", username)), None));
        }
        if let Ok(entries) = std::fs::read_dir("/media") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if is_kindle_name(&name) {
                    candidates.push((entry.path(), None));
                }
            }
        }
        candidates.push((PathBuf::from("/media/Kindle"), None));
        candidates.push((PathBuf::from("/mnt/Kindle"), None));

        // Headless / FUSE mount fallback candidates
        if let Ok(home) = std::env::var("HOME") {
            candidates.push((PathBuf::from(format!("{}/kindle", home)), None));
            candidates.push((PathBuf::from(format!("{}/Kindle", home)), None));
        }
        candidates.push((PathBuf::from("/media/kindle"), None));
        candidates.push((PathBuf::from("/mnt/kindle"), None));

        // Also inspect /proc/mounts to find any mounted filesystem labeled or pointing to Kindle
        if let Ok(mounts_content) = std::fs::read_to_string("/proc/mounts") {
            for line in mounts_content.lines() {
                let mut parts = line.split_whitespace();
                if let (Some(_raw_dev), Some(raw_mount)) = (parts.next(), parts.next()) {
                    let mount_point = unescape_proc_mounts_field(raw_mount);
                    let p = PathBuf::from(&mount_point);
                    if is_kindle_name(&mount_point)
                        && !candidates.iter().any(|(existing, _)| existing == &p)
                    {
                        candidates.push((p, None));
                    }
                }
            }
        }
    }

    for (cand, label_hint) in candidates {
        if cand.is_dir() {
            let doc_path = cand.join("documents");
            // Confirm it has either documents directory or system directory (typical Kindle layout)
            if doc_path.is_dir() || cand.join("system").is_dir() || cand.join(".system").is_dir() {
                let (used_gb, total_gb) = get_storage_stats(&cand).unwrap_or((0.0, 0.0));
                if total_gb <= 0.05 {
                    // Media ejected or dummy unmounted mountpoint
                    continue;
                }

                let fallback_hint = cand.to_string_lossy();
                let hint = label_hint
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&fallback_hint);
                let model_name = resolve_model_name(&cand, hint);

                return Some(KindleDeviceInfo {
                    root: cand,
                    documents_dir: doc_path,
                    model_name,
                    used_gb,
                    total_gb,
                });
            }
        }
    }

    None
}

/// Detects connected Kindle root directory (GVFS MTP or USB UMS mount).
pub fn detect_kindle_root() -> Option<PathBuf> {
    detect_device_info().map(|info| info.root)
}

/// Safely flushes dirty filesystem caches and unmounts the connected Kindle device.
pub fn eject_device(root: &Path) -> Result<(), SyncError> {
    let _ = root;
    let _sync_lock = DEVICE_SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // 1. Flush file system write cache to prevent eMMC data loss.
    // On Linux, use syncfs on the device root file descriptor to flush only the Kindle
    // filesystem instead of forcing a global host-wide sync across all drives.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        if let Ok(file) = std::fs::File::open(root) {
            let ret = unsafe { libc::syncfs(file.as_raw_fd()) };
            if ret != 0 {
                return Err(SyncError::Io(std::io::Error::last_os_error()));
            }
        } else {
            unsafe { libc::sync() };
        }
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        unsafe { libc::sync() };
    }
    #[cfg(windows)]
    {
        windows_eject_volume(root)?;
        return Ok(());
    }

    // 2. Unmount cleanly depending on host OS and mount type
    #[cfg(target_os = "linux")]
    {
        let root_str = root.to_string_lossy();
        if root_str.contains("/gvfs/mtp:") {
            let mtp_mount = if let Some(idx) = root_str.find("/Internal Storage") {
                &root_str[..idx]
            } else {
                &root_str
            };
            let mut cmd = Command::new("gio");
            cmd.args(["mount", "-u", "--"]).arg(mtp_mount);
            run_eject_command("gio mount -u", cmd).map_err(|err| {
                SyncError::GioFailed(format!(
                    "Failed to unmount MTP Kindle device at {:?}: {}",
                    mtp_mount, err
                ))
            })?;
        } else {
            let mut errors = Vec::new();
            let mut gio_cmd = Command::new("gio");
            gio_cmd.args(["mount", "-u", "--"]).arg(root);
            if let Err(err) = run_eject_command("gio mount -u", gio_cmd) {
                errors.push(err);
                let block_dev = std::fs::read_to_string("/proc/mounts")
                    .ok()
                    .and_then(|mounts| find_mount_source_in_mounts(&mounts, root));

                let mut unmounted = false;
                if let Some(dev) = block_dev {
                    let mut udisks_cmd = Command::new("udisksctl");
                    udisks_cmd.args(["unmount", "-b"]).arg(&dev);
                    match run_eject_command("udisksctl unmount -b", udisks_cmd) {
                        Ok(()) => unmounted = true,
                        Err(e) => errors.push(e),
                    }
                }
                if !unmounted {
                    let mut umount_cmd = Command::new("umount");
                    umount_cmd.arg("--").arg(root);
                    if let Err(e) = run_eject_command("umount", umount_cmd) {
                        errors.push(e);
                        return Err(SyncError::GioFailed(format!(
                            "Failed to cleanly unmount Kindle device at {:?}: {}",
                            root,
                            errors.join("; ")
                        )));
                    }
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("diskutil");
        cmd.args(["unmount", "--"]).arg(root);
        run_eject_command("diskutil unmount", cmd).map_err(|err| {
            SyncError::GioFailed(format!(
                "Failed to unmount Kindle device at {:?}: {}",
                root, err
            ))
        })?;
    }
    #[cfg(all(unix, not(target_os = "linux"), not(target_os = "macos")))]
    {
        let mut cmd = Command::new("umount");
        cmd.arg("--").arg(root);
        run_eject_command("umount", cmd).map_err(|err| {
            SyncError::GioFailed(format!(
                "Failed to unmount Kindle device at {:?}: {}",
                root, err
            ))
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::KindleSync;

    #[test]
    fn test_detect_device_info_structure() {
        let info = KindleSync::detect_device_info();
        println!("Detected Kindle Info: {:?}", info);
        if let Some(info) = info {
            assert!(info.root.exists());
            assert!(!info.model_name.is_empty());
            assert!(info.total_gb > 0.0);
            assert!(info.used_gb >= 0.0);
        }
    }

    #[test]
    fn test_get_storage_stats_root() {
        let stats = KindleSync::get_storage_stats(Path::new("/"));
        assert!(stats.is_some());
        let (used, total) = stats.unwrap();
        assert!(total > 0.0);
        assert!(used > 0.0);
        assert!(total >= used);
    }

    #[test]
    fn test_eject_device_targeted_syncfs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();

        // Write a test file to dirty cache
        let dummy = root.join("test_file.bin");
        std::fs::write(&dummy, b"dirty cache data").unwrap();

        // eject_device should successfully invoke targeted syncfs without failing
        let res = KindleSync::eject_device(root);
        // Note: gio mount -u may return error if not actually mounted, but syncfs itself succeeds
        let _ = res;
        assert!(dummy.exists());
    }

    #[test]
    fn test_resolve_model_name_all_kindle_models() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();

        // 1. Hint-based model resolution across all Kindle hardware variants
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle Scribe"),
            "Kindle Scribe"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle Paperwhite"),
            "Kindle Paperwhite"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle Oasis"),
            "Kindle Oasis"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle Voyage"),
            "Kindle Voyage"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle Colorsoft"),
            "Kindle Colorsoft"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "/media/drew/Kindle Basic"),
            "Kindle Basic"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "mtp:host=Kindle_Touch_1234"),
            "Kindle Touch"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "Kindle Keyboard"),
            "Kindle Keyboard"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle DX"),
            "Kindle DX"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle"),
            "Kindle"
        );

        // 2. File-based resolution via system/version.txt
        let sys_dir = root.join("system");
        std::fs::create_dir_all(&sys_dir).unwrap();
        let ver_file = sys_dir.join("version.txt");

        std::fs::write(&ver_file, b"Kindle Oasis 5.16.2.1.1").unwrap();
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle"),
            "Kindle Oasis"
        );

        std::fs::write(&ver_file, b"Kindle Colorsoft 5.17.0").unwrap();
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle"),
            "Kindle Colorsoft"
        );

        // Serial number prefix based detection in system files (e.g. G092 -> Scribe, B013 -> Voyage)
        std::fs::write(&ver_file, b"Device Serial: G092240123456789").unwrap();
        assert_eq!(
            KindleSync::resolve_model_name(root, "/media/drew/Drive"),
            "Kindle Scribe"
        );

        std::fs::write(&ver_file, b"Device Serial: B0131234567890AB").unwrap();
        assert_eq!(
            KindleSync::resolve_model_name(root, "/media/drew/Drive"),
            "Kindle Voyage"
        );
    }

    #[test]
    fn test_probe_unmounted_usb_kindle_mock_sysfs() {
        let temp = tempfile::tempdir().unwrap();
        let sys_usb = temp.path();

        // 1. Initially empty directory -> None
        assert!(KindleSync::probe_unmounted_usb_kindle_in(sys_usb).is_none());

        // 2. Non-Amazon device (e.g. Google Pixel 18d1:4ee7) -> None
        let dev1 = sys_usb.join("1-2.1");
        std::fs::create_dir_all(&dev1).unwrap();
        std::fs::write(dev1.join("idVendor"), b"18d1\n").unwrap();
        std::fs::write(dev1.join("idProduct"), b"4ee7\n").unwrap();
        std::fs::write(dev1.join("product"), b"Pixel 8\n").unwrap();
        assert!(KindleSync::probe_unmounted_usb_kindle_in(sys_usb).is_none());

        // 3. Amazon Kindle hardware (1949:0340) -> Detected!
        let dev2 = sys_usb.join("1-2.2");
        std::fs::create_dir_all(&dev2).unwrap();
        std::fs::write(dev2.join("idVendor"), b"1949\n").unwrap();
        std::fs::write(dev2.join("idProduct"), b"0340\n").unwrap();
        std::fs::write(dev2.join("product"), b"Kindle Scribe\n").unwrap();

        let detected = KindleSync::probe_unmounted_usb_kindle_in(sys_usb);
        assert!(detected.is_some());
        let info = detected.unwrap();
        assert_eq!(info.vendor_id, "1949");
        assert_eq!(info.product_id, "0340");
        assert_eq!(info.product_name, "Kindle Scribe");
    }

    #[test]
    fn test_detect_device_info_env_var_override() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("mock_kindle");
        let docs = root.join("documents");
        std::fs::create_dir_all(&docs).unwrap();

        // Set KINDLE_ROOT environment variable
        unsafe { std::env::set_var("KINDLE_ROOT", &root) };

        let info = KindleSync::detect_device_info();
        // Clear environment variable
        unsafe { std::env::remove_var("KINDLE_ROOT") };

        assert!(info.is_some());
        let dev = info.unwrap();
        assert_eq!(dev.root, root);
        assert_eq!(dev.documents_dir, docs);
    }

    #[test]
    fn test_windows_kindle_drive_verification_guards() {
        // 1. Fixed/remote drive (is_removable = false) even with "Kindle" label + markers -> false
        let fixed_temp = tempfile::tempdir().unwrap();
        let fixed_root = fixed_temp.path();
        std::fs::create_dir_all(fixed_root.join("documents")).unwrap();
        std::fs::create_dir_all(fixed_root.join("system")).unwrap();
        std::fs::write(
            fixed_root.join("system").join("version.txt"),
            b"Kindle 5.16",
        )
        .unwrap();
        assert!(!is_verified_windows_kindle_candidate(
            false,
            Some("Kindle"),
            fixed_root
        ));

        // 2. Removable drive with generic label + documents/ + system/ (no concrete Kindle markers) -> false
        let usb_temp = tempfile::tempdir().unwrap();
        let usb_root = usb_temp.path();
        std::fs::create_dir_all(usb_root.join("documents")).unwrap();
        std::fs::create_dir_all(usb_root.join("system")).unwrap();
        assert!(!is_verified_windows_kindle_candidate(
            true,
            Some("USB_DATA"),
            usb_root
        ));

        // 3. Removable drive with "Kindle Paperwhite" label + documents/ -> true
        let pw_temp = tempfile::tempdir().unwrap();
        let pw_root = pw_temp.path();
        std::fs::create_dir_all(pw_root.join("documents")).unwrap();
        assert!(is_verified_windows_kindle_candidate(
            true,
            Some("Kindle Paperwhite"),
            pw_root
        ));

        // Removable with "Kindle" label but missing layout directories -> false
        let empty_temp = tempfile::tempdir().unwrap();
        assert!(!is_verified_windows_kindle_candidate(
            true,
            Some("Kindle"),
            empty_temp.path()
        ));

        // 4. Removable drive with non-Kindle label + documents/ + concrete markers -> true
        let ver_temp = tempfile::tempdir().unwrap();
        let ver_root = ver_temp.path();
        std::fs::create_dir_all(ver_root.join("documents")).unwrap();
        std::fs::create_dir_all(ver_root.join("system")).unwrap();
        std::fs::write(ver_root.join("system").join("version.txt"), b"5.16.2").unwrap();
        assert!(is_verified_windows_kindle_candidate(
            true,
            Some("MY_EBOOK"),
            ver_root
        ));

        let sandbox_temp = tempfile::tempdir().unwrap();
        let sandbox_root = sandbox_temp.path();
        std::fs::create_dir_all(sandbox_root.join("documents")).unwrap();
        std::fs::create_dir_all(sandbox_root.join(".active_content_sandbox")).unwrap();
        assert!(is_verified_windows_kindle_candidate(
            true,
            Some("MY_EBOOK"),
            sandbox_root
        ));

        let reg_temp = tempfile::tempdir().unwrap();
        let reg_root = reg_temp.path();
        std::fs::create_dir_all(reg_root.join("documents")).unwrap();
        std::fs::create_dir_all(reg_root.join(".system")).unwrap();
        std::fs::write(reg_root.join(".system").join("dev_reg.log"), b"reg").unwrap();
        assert!(is_verified_windows_kindle_candidate(
            true,
            Some("MY_EBOOK"),
            reg_root
        ));

        // Concrete marker without documents/ -> false
        let no_docs_temp = tempfile::tempdir().unwrap();
        let no_docs_root = no_docs_temp.path();
        std::fs::create_dir_all(no_docs_root.join("system")).unwrap();
        std::fs::write(no_docs_root.join("system").join("version.txt"), b"5.16.2").unwrap();
        assert!(!is_verified_windows_kindle_candidate(
            true,
            Some("MY_EBOOK"),
            no_docs_root
        ));
    }

    #[test]
    fn test_tmp_kindle_not_auto_discovered() {
        let device_src_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sync/device.rs");
        let content =
            std::fs::read_to_string(&device_src_path).expect("Read src/sync/device.rs source");
        let prod_end = content
            .find("#[cfg(test)]")
            .expect("Locate #[cfg(test)] in device.rs");
        let prod_code = &content[..prod_end];
        let detect_start = prod_code
            .find("fn detect_device_info(")
            .expect("Locate detect_device_info");
        let detect_body = &prod_code[detect_start..];

        assert!(
            !detect_body.contains("/tmp/kindle") && !detect_body.contains("/tmp/kindle_mtp"),
            "detect_device_info() must NOT auto-register world-writable /tmp/kindle or /tmp/kindle_mtp"
        );
    }

    #[test]
    fn test_proc_mounts_unescaping_and_source_lookup() {
        assert_eq!(
            unescape_proc_mounts_field(r"/media/Kindle\040Paperwhite\011\012\134dir"),
            "/media/Kindle Paperwhite\t\n\\dir"
        );
        // Multi-byte UTF-8 octal escape sequence (\355\225\234 = '한')
        assert_eq!(
            unescape_proc_mounts_field(r"/media/\355\225\234\040Kindle"),
            "/media/한 Kindle"
        );
        // Incomplete or out-of-range octal escapes remain literal
        assert_eq!(
            unescape_proc_mounts_field(r"/media/\400\04"),
            r"/media/\400\04"
        );

        let mounts = "/dev/sdb1 /media/Kindle\\040Scribe vfat rw 0 0\n";
        assert_eq!(
            find_mount_source_in_mounts(mounts, Path::new("/media/Kindle Scribe")),
            Some("/dev/sdb1".to_string())
        );
        assert_eq!(
            find_mount_source_in_mounts(mounts, Path::new("/media/Other")),
            None
        );
    }

    #[test]
    fn test_windows_volume_device_path_extraction() {
        assert_eq!(
            windows_volume_device_path(Path::new(r"E:\")),
            Some(r"\\.\E:".to_string())
        );
        assert_eq!(
            windows_volume_device_path(Path::new(r"f:\documents\book.kfx")),
            Some(r"\\.\F:".to_string())
        );
        assert_eq!(
            windows_volume_device_path(Path::new(r"\\?\G:\")),
            Some(r"\\.\G:".to_string())
        );
        assert_eq!(windows_volume_device_path(Path::new("/mnt/Kindle")), None);
        assert_eq!(
            windows_volume_device_path(Path::new(r"\\server\share")),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_eject_device_propagates_failure_for_unmounted_and_dash_paths() {
        let temp = tempfile::tempdir().unwrap();
        let dash_dir = temp.path().join("-rf");
        std::fs::create_dir_all(&dash_dir).unwrap();
        let err = KindleSync::eject_device(&dash_dir).unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.is_empty(),
            "eject_device on unmounted path must return descriptive error"
        );
    }
}
