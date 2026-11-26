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
const EJECT_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[cfg(unix)]
fn run_eject_command(label: &str, mut cmd: Command) -> Result<(), String> {
    let out = crate::storage::run_command_with_timeout(label, &mut cmd, EJECT_COMMAND_TIMEOUT)?;
    if out.status.success() {
        Ok(())
    } else {
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
}

/// Constructs `gio mount -u -- <target>` with POSIX `--` option termination so dash-prefixed
/// mount paths cannot be interpreted as CLI flags.
#[cfg(any(target_os = "linux", test))]
fn build_linux_gio_unmount_command(target: &Path) -> Command {
    let mut cmd = Command::new("gio");
    cmd.args(["mount", "-u", "--"]).arg(target);
    cmd
}

/// Constructs `umount -- <root>` with POSIX `--` option termination so dash-prefixed
/// mount paths cannot be interpreted as CLI flags.
#[cfg(any(target_os = "linux", test))]
fn build_linux_umount_command(root: &Path) -> Command {
    let mut cmd = Command::new("umount");
    cmd.arg("--").arg(root);
    cmd
}

/// Flushes dirty filesystem caches targeting `root` (using `libc::syncfs` on Linux to avoid
/// stalling unrelated host mounts, with fallback to `libc::sync`).
#[cfg(not(windows))]
fn flush_device_root_syncfs(root: &Path) -> Result<(), SyncError> {
    // 1. Flush file system write cache to prevent eMMC data loss.
    // On Linux, use syncfs on the device root file descriptor to flush only the Kindle
    // filesystem instead of forcing a global host-wide sync across all drives.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        if let Ok(file) = std::fs::File::open(root) {
            // SAFETY: `file` is an open `File` whose raw fd remains valid for the `syncfs` call.
            let ret = unsafe { libc::syncfs(file.as_raw_fd()) };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                if matches!(
                    err.raw_os_error(),
                    Some(libc::EINVAL | libc::ENOTSUP | libc::ENOSYS)
                ) {
                    eprintln!(
                        "Warning: syncfs unsupported on {:?} ({}); continuing with unmount.",
                        root, err
                    );
                } else {
                    return Err(SyncError::Io(err));
                }
            }
        } else {
            // SAFETY: `libc::sync` takes no arguments and flushes kernel buffers without pointer access.
            unsafe { libc::sync() };
        }
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let _ = root;
        // SAFETY: `libc::sync` takes no arguments and flushes kernel buffers without pointer access.
        unsafe { libc::sync() };
    }
    #[cfg(not(unix))]
    {
        let _ = root;
    }
    Ok(())
}

/// Validates a Linux `/dev/...` block device path and constructs `udisksctl unmount --block-device=<dev>`.
///
/// GLib's `GOptionContext` treats `-b` / `--block-device` as an option taking a required argument,
/// so passing `["unmount", "-b", "--", dev]` would bind `"--"` as the device path and fail.
/// Using `--block-device=<dev>` after validating `/dev/...` prevents option injection while
/// cleanly skipping non-block FUSE sources (e.g., `jmtpfs`, `gvfsd-fuse`).
#[cfg(any(target_os = "linux", test))]
fn build_linux_udisks_unmount_command(dev: &str) -> Result<Command, String> {
    use std::ffi::OsString;
    use std::path::Component;

    if dev.is_empty() {
        return Err("Refusing empty block device path for udisksctl".to_string());
    }
    if dev.as_bytes().contains(&b'\0') {
        return Err(format!(
            "Refusing NUL-containing block device path for udisksctl: {:?}",
            dev
        ));
    }
    if !dev.starts_with("/dev/") {
        return Err(format!(
            "Skipping udisksctl for non-/dev block source: {:?}",
            dev
        ));
    }
    let dev_path = Path::new(dev);
    if dev_path
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(format!(
            "Refusing block device path containing parent traversal ('..'): {:?}",
            dev
        ));
    }
    let has_device_component = dev_path
        .strip_prefix("/dev")
        .ok()
        .is_some_and(|rel| rel.components().any(|c| matches!(c, Component::Normal(_))));
    if !has_device_component {
        return Err(format!(
            "Refusing incomplete /dev block device path for udisksctl: {:?}",
            dev
        ));
    }

    let mut arg = OsString::from("--block-device=");
    arg.push(dev);
    let mut cmd = Command::new("udisksctl");
    cmd.arg("unmount").arg(arg);
    Ok(cmd)
}

/// Validates a macOS mount-point path and constructs the `diskutil unmount` command.
///
/// Apple's `diskutil` does not accept POSIX `--` option termination (`diskutil unmount -- <path>`
/// fails unconditionally with `Unable to find disk for --`). Instead, argument-injection safety
/// is enforced by requiring `root` to be a non-empty, NUL-free, POSIX-absolute path starting with
/// `'/'` that contains at least one normal component and no `..` traversal segments, ensuring
/// `diskutil` can never interpret the target argument as a flag (`-...`) or keyword (`force`).
#[cfg(any(target_os = "macos", test))]
fn build_macos_eject_command(root: &Path) -> Result<Command, SyncError> {
    use std::path::Component;

    let raw_bytes = root.as_os_str().as_encoded_bytes();
    if raw_bytes.is_empty() {
        return Err(SyncError::SecurityViolation(
            "Refusing to eject empty macOS mount path".to_string(),
        ));
    }
    if raw_bytes.contains(&b'\0') {
        return Err(SyncError::SecurityViolation(format!(
            "Refusing to eject macOS mount path containing NUL byte: {:?}",
            root
        )));
    }
    if !raw_bytes.starts_with(b"/") {
        return Err(SyncError::SecurityViolation(format!(
            "Refusing to eject non-absolute macOS mount path: {:?}",
            root
        )));
    }
    if root.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(SyncError::SecurityViolation(format!(
            "Refusing to eject macOS mount path containing parent traversal ('..'): {:?}",
            root
        )));
    }
    if !root.components().any(|c| matches!(c, Component::Normal(_))) {
        return Err(SyncError::SecurityViolation(format!(
            "Refusing to eject POSIX root path: {:?}",
            root
        )));
    }

    let mut cmd = Command::new("diskutil");
    cmd.arg("unmount").arg(root);
    Ok(cmd)
}

/// Computes `(used_gb, total_gb)` from `statvfs` block metrics using overflow-safe `u128` arithmetic.
#[cfg(any(unix, test))]
fn compute_storage_gb(block_size: u64, total_blocks: u64, avail_blocks: u64) -> (f32, f32) {
    let bs = u128::from(block_size);
    let total_bytes = u128::from(total_blocks).saturating_mul(bs);
    let free_bytes = u128::from(avail_blocks).saturating_mul(bs);
    let used_bytes = total_bytes.saturating_sub(free_bytes);

    let gib = 1024.0_f64 * 1024.0_f64 * 1024.0_f64;
    let total_gb = total_bytes as f64 / gib;
    let used_gb = used_bytes as f64 / gib;
    (used_gb as f32, total_gb as f32)
}

/// Queries filesystem storage statistics (used_gb, total_gb).
#[cfg(unix)]
pub fn get_storage_stats(path: &Path) -> Option<(f32, f32)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `libc::statvfs` is a C POD struct where all-zero bits is a valid initial state.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a valid NUL-terminated `CString` and `stat` is a valid output buffer.
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } == 0 {
        let block_size = if stat.f_frsize > 0 {
            stat.f_frsize as u64
        } else {
            stat.f_bsize as u64
        };
        Some(compute_storage_gb(
            block_size,
            stat.f_blocks as u64,
            stat.f_bavail as u64,
        ))
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

    // SAFETY: `wide` is a NUL-terminated UTF-16 path and the three output pointers refer to initialized locals.
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

const MAX_VERSION_FILE_BYTES: u64 = 64 * 1024;

/// Matches `token_lower` as an ASCII alphanumeric token delimited by non-alphanumeric boundaries.
fn contains_ascii_token(haystack: &str, token_lower: &str) -> bool {
    haystack
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|seg| seg.eq_ignore_ascii_case(token_lower))
}

/// Resolves a Kindle model name from a device hint or version log string without substring false-positives.
fn match_model_keyword(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    const MODELS: &[(&str, &str)] = &[
        ("colorsoft", "Kindle Colorsoft"),
        ("scribe", "Kindle Scribe"),
        ("oasis", "Kindle Oasis"),
        ("voyage", "Kindle Voyage"),
        ("paperwhite", "Kindle Paperwhite"),
        ("basic", "Kindle Basic"),
        ("touch", "Kindle Touch"),
        ("keyboard", "Kindle Keyboard"),
    ];
    for &(needle, model) in MODELS {
        if lower.contains(needle) {
            return Some(model);
        }
    }
    if contains_ascii_token(text, "dx") {
        return Some("Kindle DX");
    }
    None
}

/// Reads at most `max_bytes` from a non-symlink regular file whose immediate parent directory is also not a symlink.
fn read_bounded_regular_file(path: &Path, max_bytes: u64) -> Option<String> {
    use std::io::Read;

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let parent_meta = std::fs::symlink_metadata(parent).ok()?;
        if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
            return None;
        }
    }
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::with_capacity(meta.len().min(max_bytes) as usize);
    file.take(max_bytes).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Accurately resolves the specific Kindle hardware model from mount hint or on-device files.
/// Supports all Kindle models: Scribe, Paperwhite, Oasis, Voyage, Colorsoft, Basic, Touch, Keyboard, DX.
pub fn resolve_model_name(root: &Path, hint: &str) -> String {
    // 1. Check hint (volume label, mount point, or MTP device descriptor)
    if let Some(model) = match_model_keyword(hint) {
        return model.to_string();
    }

    // 2. Inspect on-device system files (e.g. system/version.txt, .system/version.txt)
    let version_candidates = [
        root.join("system").join("version.txt"),
        root.join(".system").join("version.txt"),
        root.join("system").join("dev_reg.log"),
        root.join(".system").join("dev_reg.log"),
    ];

    for vpath in &version_candidates {
        if let Some(content) = read_bounded_regular_file(vpath, MAX_VERSION_FILE_BYTES) {
            if let Some(model) = match_model_keyword(&content) {
                return model.to_string();
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

/// Builds `KindleDeviceInfo` with honest `(0.0, 0.0)` fallback if filesystem capacity stats fail.
fn build_device_info(root: PathBuf, documents_dir: PathBuf, model_hint: &str) -> KindleDeviceInfo {
    let model_name = resolve_model_name(&root, model_hint);
    let (used_gb, total_gb) = get_storage_stats(&root).unwrap_or((0.0, 0.0));
    KindleDeviceInfo {
        root,
        documents_dir,
        model_name,
        used_gb,
        total_gb,
    }
}

/// Detects connected Kindle root directory and metadata (GVFS MTP or USB UMS mount).
pub fn detect_device_info() -> Option<KindleDeviceInfo> {
    let env_override = std::env::var_os("KINDLE_ROOT")
        .or_else(|| std::env::var_os("KINDLE_MOUNT_PATH"))
        .map(PathBuf::from);
    detect_device_info_with_override(env_override.as_deref())
}

/// Detects connected Kindle root directory and metadata with an optional explicit mount override.
pub(crate) fn detect_device_info_with_override(
    override_root: Option<&Path>,
) -> Option<KindleDeviceInfo> {
    // 0. Explicit override for headless servers / CI / Docker / deterministic tests
    if let Some(root) = override_root
        && root.is_dir()
    {
        let internal = root.join("Internal Storage");
        let active_root = if internal.is_dir() {
            internal
        } else {
            root.to_path_buf()
        };
        let doc_path = active_root.join("documents");
        let final_docs = if doc_path.is_dir() {
            doc_path
        } else {
            active_root.clone()
        };
        let hint = active_root.to_string_lossy().into_owned();
        return Some(build_device_info(active_root, final_docs, &hint));
    }

    // 1. Check GVFS MTP on Linux
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `libc::getuid` is a pure POSIX query with no arguments or pointer dereferences.
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
                    return Some(build_device_info(root, doc_path, &name));
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

            // SAFETY: `wide` is a valid NUL-terminated UTF-16 drive root string.
            let is_removable = unsafe { GetDriveTypeW(wide.as_ptr()) } == DRIVE_REMOVABLE;
            if !is_removable {
                continue;
            }

            let mut vol_buf = [0u16; 261];
            // SAFETY: `wide` is a valid NUL-terminated UTF-16 string and `vol_buf` has `vol_buf.len()` UTF-16 elements.
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
    let _sync_lock = DEVICE_SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    #[cfg(windows)]
    {
        windows_eject_volume(root)
    }

    #[cfg(not(windows))]
    {
        let _ = root;

        #[cfg(target_os = "macos")]
        let macos_eject_cmd = build_macos_eject_command(root)?;

        // 1. Flush file system write cache to prevent eMMC data loss.
        flush_device_root_syncfs(root)?;

        // 2. Unmount cleanly depending on host OS and mount type
        #[cfg(target_os = "linux")]
        {
            let root_str = root.to_string_lossy();
            if crate::storage::is_mtp_path(root) {
                let mtp_mount = if let Some(idx) = root_str.find("/Internal Storage") {
                    Path::new(&root_str[..idx])
                } else {
                    root
                };
                let cmd = build_linux_gio_unmount_command(mtp_mount);
                run_eject_command("gio mount -u", cmd).map_err(|err| {
                    SyncError::GioFailed(format!(
                        "Failed to unmount MTP Kindle device at {:?}: {}",
                        mtp_mount, err
                    ))
                })?;
            } else {
                let mut errors = Vec::new();
                let gio_cmd = build_linux_gio_unmount_command(root);
                if let Err(err) = run_eject_command("gio mount -u", gio_cmd) {
                    errors.push(err);
                    let block_dev = std::fs::read_to_string("/proc/mounts")
                        .ok()
                        .and_then(|mounts| find_mount_source_in_mounts(&mounts, root));

                    let mut unmounted = false;
                    if let Some(dev) = block_dev
                        && let Ok(udisks_cmd) = build_linux_udisks_unmount_command(&dev)
                    {
                        match run_eject_command("udisksctl unmount", udisks_cmd) {
                            Ok(()) => unmounted = true,
                            Err(e) => errors.push(e),
                        }
                    }
                    if !unmounted {
                        let umount_cmd = build_linux_umount_command(root);
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
            run_eject_command("diskutil unmount", macos_eject_cmd).map_err(|err| {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::KindleSync;

    #[test]
    fn test_detect_device_info_structure() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("Kindle");
        let docs = root.join("documents");
        let sys = root.join("system");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(&sys).unwrap();
        std::fs::write(sys.join("version.txt"), b"Kindle Paperwhite 5.16.2").unwrap();

        let info = detect_device_info_with_override(Some(&root))
            .expect("Deterministic TempDir Kindle override must resolve device info");
        assert_eq!(info.root, root);
        assert_eq!(info.documents_dir, docs);
        assert_eq!(info.model_name, "Kindle Paperwhite");
        assert!(info.total_gb > 0.0);
        assert!(info.used_gb >= 0.0);
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

        // Exercise targeted syncfs flush deterministically without invoking host unmount tools
        #[cfg(not(windows))]
        flush_device_root_syncfs(root).expect("flush_device_root_syncfs must succeed on tempdir");
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

        let dev = detect_device_info_with_override(Some(&root))
            .expect("Override root with documents/ must be detected");
        assert_eq!(dev.root, root);
        assert_eq!(dev.documents_dir, docs);
        let expected_stats = get_storage_stats(&root).unwrap_or((0.0, 0.0));
        assert_eq!(dev.total_gb, expected_stats.1);
        assert!(
            dev.used_gb >= 0.0 && dev.used_gb <= dev.total_gb,
            "used_gb ({}) must be within [0.0, total_gb ({})]",
            dev.used_gb,
            dev.total_gb
        );
        assert!(
            (dev.used_gb - expected_stats.0).abs() < 1.0,
            "used_gb ({}) drifted unexpectedly from statvfs ({})",
            dev.used_gb,
            expected_stats.0
        );

        // MTP-style "Internal Storage" subdirectory resolution inside override root
        let mtp_root = temp.path().join("mtp_override");
        let internal = mtp_root.join("Internal Storage");
        let internal_docs = internal.join("documents");
        std::fs::create_dir_all(&internal_docs).unwrap();

        let mtp_dev = detect_device_info_with_override(Some(&mtp_root))
            .expect("Internal Storage override must be detected");
        assert_eq!(mtp_dev.root, internal);
        assert_eq!(mtp_dev.documents_dir, internal_docs);

        // Fallback when override root has no documents/ subdirectory
        let bare_root = temp.path().join("bare_override");
        std::fs::create_dir_all(&bare_root).unwrap();
        let bare_dev = detect_device_info_with_override(Some(&bare_root))
            .expect("Bare override root must fall back to root as documents_dir");
        assert_eq!(bare_dev.root, bare_root);
        assert_eq!(bare_dev.documents_dir, bare_root);

        // Non-fabricated (0.0, 0.0) storage fallback when stats cannot be queried
        let missing_mount = temp.path().join("missing_mount_point");
        let fallback_dev = build_device_info(
            missing_mount.clone(),
            missing_mount.join("documents"),
            "Kindle",
        );
        assert_eq!(fallback_dev.used_gb, 0.0);
        assert_eq!(fallback_dev.total_gb, 0.0);
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

    #[test]
    fn test_eject_command_builders_and_error_propagation() {
        use std::ffi::OsStr;

        // 1. Linux gio mount -u must include POSIX `--` before dash-prefixed and MTP paths
        let dash_path = Path::new("/media/user/-rf");
        let gio_cmd = build_linux_gio_unmount_command(dash_path);
        assert_eq!(gio_cmd.get_program(), OsStr::new("gio"));
        let gio_args: Vec<&OsStr> = gio_cmd.get_args().collect();
        assert_eq!(
            gio_args,
            vec![
                OsStr::new("mount"),
                OsStr::new("-u"),
                OsStr::new("--"),
                OsStr::new("/media/user/-rf"),
            ]
        );

        let mtp_path = Path::new("/run/user/1000/gvfs/mtp:host=Amazon_Kindle");
        let mtp_cmd = build_linux_gio_unmount_command(mtp_path);
        assert_eq!(mtp_cmd.get_program(), OsStr::new("gio"));
        assert_eq!(
            mtp_cmd.get_args().collect::<Vec<&OsStr>>(),
            vec![
                OsStr::new("mount"),
                OsStr::new("-u"),
                OsStr::new("--"),
                OsStr::new("/run/user/1000/gvfs/mtp:host=Amazon_Kindle"),
            ]
        );

        // 2. Linux umount must include POSIX `--` before dash-prefixed paths
        let umount_cmd = build_linux_umount_command(dash_path);
        assert_eq!(umount_cmd.get_program(), OsStr::new("umount"));
        assert_eq!(
            umount_cmd.get_args().collect::<Vec<&OsStr>>(),
            vec![OsStr::new("--"), OsStr::new("/media/user/-rf")]
        );

        // 3. Deterministic run_eject_command error propagation (never invokes live host unmount)
        #[cfg(unix)]
        {
            let mut fail_cmd = Command::new("sh");
            fail_cmd
                .arg("-c")
                .arg("printf 'target is not mounted' >&2; exit 1");
            let err = run_eject_command("mock unmount", fail_cmd)
                .expect_err("non-zero eject command must return Err");
            assert!(err.contains("mock unmount failed"));
            assert!(err.contains("target is not mounted"));

            let mut ok_cmd = Command::new("sh");
            ok_cmd.arg("-c").arg("exit 0");
            run_eject_command("mock unmount", ok_cmd)
                .expect("zero-exit eject command must return Ok");
        }
    }

    #[test]
    fn test_resolve_model_name_dx_tokens_symlink_ignore_and_honest_storage_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();

        // Substring "dx" inside unrelated words/usernames must NOT trigger "Kindle DX"
        assert_eq!(
            KindleSync::resolve_model_name(root, "/media/dxb/Kindle"),
            "Kindle"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Index/Kindle"),
            "Kindle"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Sandbox/Kindle"),
            "Kindle"
        );
        // Delimited DX tokens still match
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle DX"),
            "Kindle DX"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "Kindle_DX_2"),
            "Kindle DX"
        );
        // Glued/underscored vendor tokens remain intact
        assert_eq!(
            KindleSync::resolve_model_name(root, "KindlePaperwhite"),
            "Kindle Paperwhite"
        );
        assert_eq!(
            KindleSync::resolve_model_name(root, "mtp:host=Kindle_Touch_1234"),
            "Kindle Touch"
        );

        // Hex/words containing "dx" inside system/version.txt must NOT trigger "Kindle DX"
        let sys_dir = root.join("system");
        std::fs::create_dir_all(&sys_dir).unwrap();
        let ver_path = sys_dir.join("version.txt");
        std::fs::write(&ver_path, b"0x12dx34 index sandbox").unwrap();
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle"),
            "Kindle"
        );

        // Symlinked version.txt and symlinked .system directory must be ignored
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let outside = temp.path().join("outside_secret.txt");
            std::fs::write(&outside, b"Kindle Scribe").unwrap();
            std::fs::remove_file(&ver_path).unwrap();
            symlink(&outside, &ver_path).unwrap();
            assert_eq!(
                KindleSync::resolve_model_name(root, "/Volumes/Kindle"),
                "Kindle"
            );

            let outside_dir = temp.path().join("outside_sys");
            std::fs::create_dir_all(&outside_dir).unwrap();
            std::fs::write(outside_dir.join("version.txt"), b"Kindle Oasis").unwrap();
            symlink(&outside_dir, root.join(".system")).unwrap();
            assert_eq!(
                KindleSync::resolve_model_name(root, "/Volumes/Kindle"),
                "Kindle"
            );
        }

        // Honest (0.0, 0.0) storage fallback when get_storage_stats fails
        let missing_mount = root.join("nonexistent_mount_point");
        let info = build_device_info(
            missing_mount.clone(),
            missing_mount.join("documents"),
            "Kindle",
        );
        assert_eq!(info.used_gb, 0.0);
        assert_eq!(info.total_gb, 0.0);
    }

    #[test]
    fn test_resolve_model_name_caps_version_file_at_64k() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let sys_dir = root.join("system");
        std::fs::create_dir_all(&sys_dir).unwrap();
        let ver_path = sys_dir.join("version.txt");

        // Bytes past MAX_VERSION_FILE_BYTES (64 KiB) must be ignored by read_bounded_regular_file
        let mut oversized = vec![b' '; MAX_VERSION_FILE_BYTES as usize + 64];
        oversized[MAX_VERSION_FILE_BYTES as usize..MAX_VERSION_FILE_BYTES as usize + 12]
            .copy_from_slice(b"Kindle Oasis");
        std::fs::write(&ver_path, &oversized).unwrap();
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle"),
            "Kindle"
        );

        // Model keyword within the 64 KiB cap is detected cleanly
        let mut within_cap = vec![b' '; MAX_VERSION_FILE_BYTES as usize];
        within_cap[MAX_VERSION_FILE_BYTES as usize - 12..].copy_from_slice(b"Kindle Oasis");
        std::fs::write(&ver_path, &within_cap).unwrap();
        assert_eq!(
            KindleSync::resolve_model_name(root, "/Volumes/Kindle"),
            "Kindle Oasis"
        );
    }

    #[test]
    fn test_build_macos_eject_command_constructs_valid_diskutil_argv_without_double_dash() {
        use std::ffi::OsStr;

        let valid_paths = [
            "/Volumes/Kindle",
            "/Volumes/My Kindle",
            "/Volumes/Kindle Paperwhite",
            "/Volumes/Kindle/",
            "/Volumes/-rf",
            "/var/folders/tmp/-rf",
            "/var/folders/ab/cd/-rf",
        ];

        for raw in valid_paths {
            let cmd = build_macos_eject_command(Path::new(raw)).unwrap_or_else(|e| {
                panic!("Expected valid macOS mount path {:?}, got {:?}", raw, e)
            });
            assert_eq!(cmd.get_program(), OsStr::new("diskutil"));
            let args: Vec<&OsStr> = cmd.get_args().collect();
            assert_eq!(
                args,
                vec![OsStr::new("unmount"), OsStr::new(raw)],
                "Unexpected diskutil argv for {:?}",
                raw
            );
            assert!(
                !args.contains(&OsStr::new("--")),
                "diskutil unmount argv must NOT contain POSIX '--' for {:?}",
                raw
            );
        }

        // Preserve raw non-UTF-8 Unix mount paths without lossy conversion
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;

            let non_utf8 = OsStr::from_bytes(b"/Volumes/Kindle_\xff_Vol");
            let cmd = build_macos_eject_command(Path::new(non_utf8))
                .expect("Anchored non-UTF-8 Unix path must be accepted");
            assert_eq!(cmd.get_program(), OsStr::new("diskutil"));
            let args: Vec<&OsStr> = cmd.get_args().collect();
            assert_eq!(args, vec![OsStr::new("unmount"), non_utf8]);
            assert!(!args.contains(&OsStr::new("--")));
        }
    }

    #[test]
    fn test_build_macos_eject_command_rejects_unsafe_paths() {
        let unsafe_paths = [
            "",
            "Kindle",
            "Volumes/Kindle",
            "./Kindle",
            "-rf",
            "--",
            "--help",
            "force",
            "/",
            "///",
            "/.",
            "/Volumes/../etc",
            "/Volumes/Kindle/..",
            "/Volumes/Kindle/../Other",
            "/Volumes/Kindle\0evil",
        ];

        for raw in unsafe_paths {
            let res = build_macos_eject_command(Path::new(raw));
            assert!(
                matches!(res, Err(SyncError::SecurityViolation(_))),
                "Expected SyncError::SecurityViolation for unsafe path {:?}, got {:?}",
                raw,
                res
            );
        }

        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;

            let nul_path = Path::new(OsStr::from_bytes(b"/Volumes/Kindle\0bad"));
            let res = build_macos_eject_command(nul_path);
            assert!(
                matches!(res, Err(SyncError::SecurityViolation(_))),
                "Expected SyncError::SecurityViolation for Unix NUL path, got {:?}",
                res
            );
        }
    }

    #[test]
    fn test_build_linux_udisks_unmount_command_validates_block_device() {
        use std::ffi::OsStr;

        for valid in ["/dev/sdb1", "/dev/disk/by-id/usb-Amazon_Kindle-0:0"] {
            let cmd = build_linux_udisks_unmount_command(valid)
                .unwrap_or_else(|e| panic!("Expected valid block dev {:?}, got {}", valid, e));
            assert_eq!(cmd.get_program(), OsStr::new("udisksctl"));
            let args: Vec<&OsStr> = cmd.get_args().collect();
            let expected_flag = format!("--block-device={}", valid);
            assert_eq!(
                args,
                vec![OsStr::new("unmount"), OsStr::new(&expected_flag)]
            );
        }

        for invalid in [
            "",
            "-f",
            "--force",
            "../dev/sda",
            "/dev/../etc/passwd",
            "/dev/",
            "/dev/.",
            "gvfsd-fuse",
            "jmtpfs",
            "/dev/sdb1\0bad",
        ] {
            assert!(
                build_linux_udisks_unmount_command(invalid).is_err(),
                "Expected invalid block device {:?} to be rejected",
                invalid
            );
        }
    }

    #[test]
    fn test_compute_storage_gb_handles_u64_overflow_without_panic() {
        let (used_gb, total_gb) = compute_storage_gb(4096, u64::MAX, u64::MAX / 2);
        assert!(total_gb.is_finite() && total_gb > 0.0);
        assert!(used_gb.is_finite() && used_gb >= 0.0 && used_gb <= total_gb);

        // avail > total saturates used_gb cleanly to 0.0
        let (used_sat, total_sat) = compute_storage_gb(4096, 100, 200);
        assert_eq!(used_sat, 0.0);
        assert!(total_sat > 0.0);
    }
}
