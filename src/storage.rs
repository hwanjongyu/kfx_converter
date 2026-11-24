//! Level-0 filesystem safety, bounded subprocess execution, MTP path detection,
//! and FAT32/sidecar sanitization primitives shared across `enhancements`, `sync`, `packager`, and `pipeline`.
//!
//! IMPORTANT: Local atomic write helpers in this module deliberately avoid per-file fsync/fdatasync
//! calls so USB mass-storage Kindle devices (fsg gadget driver) never hit SCSI SYNCHRONIZE CACHE stalls.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Global mutual exclusion lock ensuring all write and deletion operations targeting
/// Kindle storage (VFAT FAT table allocations, companion .sdr writes, and MTP commands)
/// are strictly serialized to prevent filesystem corruption, USB FIFO stalls, and MTP deadlocks.
pub(crate) static DEVICE_SYNC_LOCK: Mutex<()> = Mutex::new(());

/// Returns `true` when `path` targets a GVFS/MTP virtual filesystem mount or URI.
#[allow(dead_code)]
pub(crate) fn is_mtp_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("/gvfs/mtp:") || s.starts_with("mtp://") || s.starts_with("gio://")
}

/// Runs an external command with concurrent stdout/stderr draining and an explicit watchdog timeout.
/// Prevents >64 KiB kernel pipe deadlocks and hung child processes from wedging callers or locks.
#[allow(dead_code)]
pub(crate) fn run_command_with_timeout(
    label: &str,
    cmd: &mut Command,
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", label, e))?;

    let stdout_thread = child.stdout.take().map(|mut stdout| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            buf
        })
    });

    let stderr_thread = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf);
            buf
        })
    });

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_thread
                    .and_then(|t| t.join().ok())
                    .unwrap_or_default();
                let stderr = stderr_thread
                    .and_then(|t| t.join().ok())
                    .unwrap_or_default();
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                let elapsed = start.elapsed();
                if elapsed >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Detach reader threads instead of joining: a grandchild process may still
                    // hold inherited stdout/stderr pipe FDs open after the direct child is killed.
                    drop(stdout_thread);
                    drop(stderr_thread);
                    return Err(format!(
                        "{} timed out after {} seconds",
                        label,
                        timeout.as_secs()
                    ));
                }
                let remaining = timeout.saturating_sub(elapsed);
                std::thread::sleep(Duration::from_millis(50).min(remaining));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout_thread);
                drop(stderr_thread);
                return Err(format!("Failed waiting for {}: {}", label, e));
            }
        }
    }
}

/// Executes `gio copy -- <src> <dst>` under a bounded watchdog timeout.
#[allow(dead_code)]
pub(crate) fn execute_gio_copy_with_timeout(
    src: &Path,
    dst: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let mut cmd = Command::new("gio");
    cmd.arg("copy").arg("--").arg(src).arg(dst);
    let output = run_command_with_timeout("gio copy", &mut cmd, timeout)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

/// Executes `gio copy -- <src> <dst>` under a bounded watchdog timeout, returning `io::Result<()>`.
#[allow(dead_code)]
pub(crate) fn execute_gio_copy_with_timeout_io(
    src: &Path,
    dst: &Path,
    timeout: Duration,
) -> io::Result<()> {
    execute_gio_copy_with_timeout(src, dst, timeout).map_err(io::Error::other)
}

/// Executes `gio mkdir -p -- <dir>` under a bounded watchdog timeout.
#[allow(dead_code)]
pub(crate) fn run_gio_mkdir_with_timeout(dir: &Path, timeout: Duration) -> io::Result<()> {
    let mut cmd = Command::new("gio");
    cmd.arg("mkdir").arg("-p").arg("--").arg(dir);
    let output =
        run_command_with_timeout("gio mkdir", &mut cmd, timeout).map_err(io::Error::other)?;
    if output.status.success() || dir.is_dir() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(io::Error::other(format!(
            "gio mkdir -p failed for {:?} (status {:?}): {}",
            dir,
            output.status.code(),
            stderr.trim()
        )))
    }
}

fn is_os_root_symlink(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        if path == Path::new("/tmp") || path == Path::new("/var") || path == Path::new("/etc") {
            return true;
        }
        let tmp = std::env::temp_dir();
        let tmp_str = tmp.to_string_lossy();
        let tmp_trimmed = Path::new(tmp_str.trim_end_matches('/'));
        if path == tmp || path == tmp_trimmed {
            return true;
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
    }
    false
}

/// Rejects a path if it already exists as a symbolic link (including dangling symlinks).
#[allow(dead_code)]
pub(crate) fn reject_symlink_path(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Refusing symlink path: {:?}", path),
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Rejects any symbolic link component strictly below `root` along the path to `target`.
/// Does NOT inspect OS path components above `root` (safe on macOS `/var` -> `/private/var`).
#[allow(dead_code)]
pub(crate) fn reject_symlink_ancestors_under(root: &Path, target: &Path) -> io::Result<()> {
    let rel = target.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Target {:?} is outside trusted root {:?}", target, root),
        )
    })?;
    let mut cur = root.to_path_buf();
    for comp in rel.components() {
        match comp {
            Component::Normal(part) => {
                cur.push(part);
                reject_symlink_path(&cur)?;
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("Unsafe path component in {:?} under {:?}", target, root),
                ));
            }
        }
    }
    Ok(())
}

/// Ensures `dir` exists as a real (non-symlink) directory, creating missing subdirectories one by one
/// while rejecting symlink targets and symlinked ancestors.
#[allow(dead_code)]
pub(crate) fn ensure_non_symlink_dir(dir: &Path) -> io::Result<()> {
    if dir.as_os_str().is_empty() || dir == Path::new(".") {
        return Ok(());
    }

    match std::fs::symlink_metadata(dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("Refusing symlink directory: {:?}", dir),
                ));
            }
            if !meta.file_type().is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("Path exists and is not a directory: {:?}", dir),
                ));
            }
            return Ok(());
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    let mut missing: Vec<OsString> = Vec::new();
    let mut cur = dir.to_path_buf();

    while !cur.as_os_str().is_empty() {
        match std::fs::symlink_metadata(&cur) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    if !is_os_root_symlink(&cur) {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            format!("Refusing symlink ancestor directory: {:?}", cur),
                        ));
                    }
                    if !std::fs::metadata(&cur).is_ok_and(|m| m.is_dir()) {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            format!("Ancestor path is not a directory: {:?}", cur),
                        ));
                    }
                } else if !meta.file_type().is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("Ancestor path is not a directory: {:?}", cur),
                    ));
                }
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let Some(name) = cur.file_name() else {
                    break;
                };
                missing.push(name.to_os_string());
                match cur.parent() {
                    Some(parent) => cur = parent.to_path_buf(),
                    None => {
                        cur = PathBuf::new();
                        break;
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }

    for part in missing.into_iter().rev() {
        cur.push(part);
        match std::fs::create_dir(&cur) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let meta = std::fs::symlink_metadata(&cur)?;
                if meta.file_type().is_symlink() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("Refusing symlink directory created concurrently: {:?}", cur),
                    ));
                }
                if !meta.file_type().is_dir() {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }

    let final_meta = std::fs::symlink_metadata(dir)?;
    if final_meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Refusing symlink directory: {:?}", dir),
        ));
    }
    if !final_meta.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("Path exists and is not a directory: {:?}", dir),
        ));
    }
    Ok(())
}

/// Sets the read-only attribute on an existing non-symlink file without following symlinks.
/// On Unix, uses `O_NOFOLLOW` + fd-based `set_permissions` (`fchmod`) to close the path TOCTOU window.
#[allow(dead_code)]
#[allow(clippy::permissions_set_readonly_false)]
pub(crate) fn set_readonly_non_symlink(path: &Path, readonly: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let file = match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(f) => f,
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("Refusing symlink path: {:?}", path),
                ));
            }
            Err(e) => return Err(e),
        };
        let meta = file.metadata()?;
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("Refusing symlink path: {:?}", path),
            ));
        }
        let mut perms = meta.permissions();
        if perms.readonly() != readonly {
            perms.set_readonly(readonly);
            file.set_permissions(perms)?;
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("Refusing symlink path: {:?}", path),
            ));
        }
        let mut perms = meta.permissions();
        if perms.readonly() != readonly {
            perms.set_readonly(readonly);
            std::fs::set_permissions(path, perms)?;
        }
        Ok(())
    }
}

/// Clears the read-only attribute on an existing non-symlink destination so atomic overwrite succeeds.
/// Returns `Ok(())` when `path` does not exist; returns `PermissionDenied` if `path` is a symlink.
#[allow(dead_code)]
pub(crate) fn prepare_writable_non_symlink(path: &Path) -> io::Result<()> {
    match set_readonly_non_symlink(path, false) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Ensures `dir` exists safely on both local filesystems (rejecting symlinks) and GVFS/MTP URIs
/// (bounded `gio mkdir -p`).
#[allow(dead_code)]
pub(crate) fn ensure_dir_exists_safe(dir: &Path) -> io::Result<()> {
    let s = dir.to_string_lossy();
    if s.starts_with("gio://") || s.starts_with("mtp://") {
        return run_gio_mkdir_with_timeout(dir, Duration::from_secs(15));
    }
    match ensure_non_symlink_dir(dir) {
        Ok(()) => Ok(()),
        Err(err) if is_mtp_path(dir) && err.kind() != io::ErrorKind::PermissionDenied => {
            run_gio_mkdir_with_timeout(dir, Duration::from_secs(15))
        }
        Err(err) => Err(err),
    }
}

/// Atomically writes `data` to a local non-symlink destination `dst` via a staged tempfile.
/// Rejects symlink leaf and symlink parent; clears read-only via `O_NOFOLLOW` before persist.
/// Deliberately omits per-file fsync/fdatasync for USB mass-storage Kindle gadget compatibility.
#[allow(dead_code)]
pub(crate) fn write_bytes_local_safe(dst: &Path, data: &[u8]) -> io::Result<()> {
    reject_symlink_path(dst)?;
    let parent = match dst.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    if parent != Path::new(".") && !is_os_root_symlink(parent) {
        reject_symlink_path(parent)?;
    }

    let mut tmp = tempfile::Builder::new()
        .prefix(".kfx_tmp.")
        .suffix(".part")
        .tempfile_in(parent)?;
    tmp.write_all(data)?;
    tmp.flush()?;

    prepare_writable_non_symlink(dst)?;
    reject_symlink_path(dst)?;
    tmp.persist(dst).map_err(|e| e.error)?;
    Ok(())
}

/// Writes `data` to `dst`, using a bounded `gio copy` on MTP/GVFS paths (optionally acquiring
/// `DEVICE_SYNC_LOCK`) or `write_bytes_local_safe` on local filesystems.
#[allow(dead_code)]
pub(crate) fn write_bytes_with_mtp(dst: &Path, data: &[u8], lock_mtp: bool) -> io::Result<()> {
    if is_mtp_path(dst) {
        let _guard = if lock_mtp {
            Some(DEVICE_SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner()))
        } else {
            None
        };
        let mut tmp = tempfile::NamedTempFile::new()?;
        tmp.write_all(data)?;
        tmp.flush()?;
        execute_gio_copy_with_timeout_io(tmp.path(), dst, Duration::from_secs(60))
    } else {
        write_bytes_local_safe(dst, data)
    }
}

// ============================================================================
// FAT32 / VFAT Filename & Sidecar Identifier Sanitization
// ============================================================================

/// Returns `true` if `name` has a Windows/FAT32 DOS reserved device stem
/// (`CON`, `PRN`, `AUX`, `NUL`, `COM1..=COM9`, `LPT1..=LPT9`) before the first `.`.
fn is_dos_reserved(name: &str) -> bool {
    const DOS_DEVICES: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(|c: char| c.is_ascii_whitespace() || c == '.');
    DOS_DEVICES.iter().any(|dev| stem.eq_ignore_ascii_case(dev))
}

/// Truncates `s` on UTF-8 char boundaries to at most `max_bytes` and trims
/// trailing FAT32-unsafe characters (`'.'`, `'_'`, `' '`).
fn clamp_and_trim_filename(s: &mut String, max_bytes: usize) {
    while s.len() > max_bytes {
        s.pop();
    }
    let trimmed_len = s.trim_end_matches(['.', '_', ' ']).len();
    s.truncate(trimmed_len);
}

/// Sanitizes an e-book title to be completely safe for FAT32 / VFAT storage
/// and prevents directory traversal attacks (CWE-22 / CWE-73).
pub fn sanitize_filename(name: &str) -> String {
    const MAX_FILENAME_BYTES: usize = 128;
    let forbidden = ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0'];
    let mut clean: String = name
        .chars()
        .map(|c| {
            if forbidden.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();

    // Disallow relative directory traversal sequences
    while clean.contains("..") {
        clean = clean.replace("..", "_");
    }

    let mut clean = clean.trim().trim_end_matches(['.', '_', ' ']).to_string();
    if clean.is_empty() {
        clean = "book".to_string();
    }

    // Guard against FAT32 DOS device names (CON, PRN, AUX, NUL, COM1-9, LPT1-9)
    if is_dos_reserved(&clean) {
        clean = format!("book_{}", clean);
    }

    // Clamp UTF-8 byte length to <= 128 bytes, re-trim tail, and re-verify invariants
    clamp_and_trim_filename(&mut clean, MAX_FILENAME_BYTES);
    if clean.is_empty() {
        clean = "book".to_string();
    }
    if is_dos_reserved(&clean) {
        clean = format!("book_{}", clean);
        clamp_and_trim_filename(&mut clean, MAX_FILENAME_BYTES);
    }
    clean
}

/// Sanitizes an untrusted sidecar stem (`asin` or `content_id`) by trimming whitespace,
/// removing path separators, dots, NULs, and control characters, and capping to 64 chars.
/// Returns `None` if nothing remains after sanitization.
pub fn sanitize_sidecar_stem(raw: &str) -> Option<String> {
    let forbidden = ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0', ' ', '.'];
    let mut clean: String = raw
        .trim()
        .chars()
        .filter(|c| !forbidden.contains(c) && !c.is_control())
        .collect();
    if clean.is_empty() {
        return None;
    }
    if let Some((idx, _)) = clean.char_indices().nth(64) {
        clean.truncate(idx);
    }
    Some(clean)
}

/// Sanitizes an ASIN identifier against path traversal and special characters.
pub fn sanitize_asin(asin: &str) -> String {
    sanitize_sidecar_stem(asin).unwrap_or_else(|| "UNKNOWN_ASIN".to_string())
}

/// Normalizes and validates a Kindle CDE content type (`"EBOK"` or `"PDOC"`).
///
/// Accepts `"EBOK"` and `"PDOC"` (ASCII case-insensitive), defaulting any invalid
/// or unrecognized input to `"EBOK"`.
pub fn sanitize_cde_content_type(cde: &str) -> &'static str {
    if cde.trim().eq_ignore_ascii_case("PDOC") {
        "PDOC"
    } else {
        "EBOK"
    }
}

/// Computes the normalized 10-character Kindle ASIN alias (`B` + 9 ASCII alphanumeric chars)
/// from `asin` when distinct (case-insensitively) from `asin`.
pub fn normalize_asin_10(asin: &str) -> Option<String> {
    let clean = sanitize_sidecar_stem(asin)?;
    if clean.len() == 10
        && clean.as_bytes()[0].eq_ignore_ascii_case(&b'B')
        && clean.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return None;
    }
    let prefix: String = clean.chars().take(9).collect();
    if prefix.chars().count() == 9 && prefix.chars().all(|c| c.is_ascii_alphanumeric()) {
        let candidate = format!("B{}", prefix.to_ascii_uppercase());
        if !candidate.eq_ignore_ascii_case(&clean) {
            return Some(candidate);
        }
    }
    None
}

fn push_unique_stem(stems: &mut Vec<String>, seen_lower: &mut HashSet<String>, stem: String) {
    if seen_lower.insert(stem.to_lowercase()) {
        stems.push(stem);
    }
}

/// Collects case-insensitively deduplicated sanitized stems for sidecars
/// (`(asin, asin_10, content_id)`).
pub fn collect_sidecar_stems(asin: Option<&str>, content_id: Option<&str>) -> Vec<String> {
    let mut stems = Vec::new();
    let mut seen_lower = HashSet::new();
    if let Some(safe_asin) = asin.and_then(sanitize_sidecar_stem) {
        let asin_10 = normalize_asin_10(&safe_asin);
        push_unique_stem(&mut stems, &mut seen_lower, safe_asin);
        if let Some(alias) = asin_10 {
            push_unique_stem(&mut stems, &mut seen_lower, alias);
        }
    }
    if let Some(safe_cid) = content_id.and_then(sanitize_sidecar_stem) {
        push_unique_stem(&mut stems, &mut seen_lower, safe_cid);
    }
    stems
}

/// Collects case-insensitively deduplicated sanitized stems for thumbnail files
/// (`(asin, content_id)`).
pub fn collect_thumbnail_stems(asin: Option<&str>, content_id: Option<&str>) -> Vec<String> {
    let mut stems = Vec::new();
    let mut seen_lower = HashSet::new();
    if let Some(safe_asin) = asin.and_then(sanitize_sidecar_stem) {
        push_unique_stem(&mut stems, &mut seen_lower, safe_asin);
    }
    if let Some(safe_cid) = content_id.and_then(sanitize_sidecar_stem) {
        push_unique_stem(&mut stems, &mut seen_lower, safe_cid);
    }
    stems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_mtp_path_unified_schemes() {
        assert!(is_mtp_path(Path::new(
            "/run/user/1000/gvfs/mtp:host=Amazon_Kindle/Internal Storage/documents"
        )));
        assert!(is_mtp_path(Path::new("mtp://Amazon_Kindle/documents")));
        assert!(is_mtp_path(Path::new("gio://mtp/kindle/documents")));
        assert!(!is_mtp_path(Path::new("/media/user/Kindle/documents")));
    }

    #[cfg(unix)]
    #[test]
    fn test_run_command_with_timeout_kills_hung_child() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 2");
        let start = Instant::now();
        let err = run_command_with_timeout("test sleep", &mut cmd, Duration::from_millis(100))
            .expect_err("hung child must time out");
        assert!(err.contains("timed out"), "unexpected error: {}", err);
        assert!(
            start.elapsed() < Duration::from_millis(800),
            "timeout took too long (blocked on grandchild pipe?): {:?}",
            start.elapsed()
        );

        // Happy-path fast child captures both stdout and stderr cleanly
        let mut ok_cmd = Command::new("sh");
        ok_cmd.arg("-c").arg("printf 'out_ok'; printf 'err_ok' >&2");
        let output =
            run_command_with_timeout("test ok", &mut ok_cmd, Duration::from_secs(2)).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"out_ok");
        assert_eq!(output.stderr, b"err_ok");
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_helpers_and_atomic_write_safe() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let temp = tempfile::tempdir().unwrap();
        let real_dir = temp.path().join("real_dir");
        std::fs::create_dir_all(&real_dir).unwrap();
        let link_dir = temp.path().join("link_dir");
        symlink(&real_dir, &link_dir).unwrap();

        // Reject symlink dir & symlink ancestor
        assert_eq!(
            ensure_non_symlink_dir(&link_dir).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            ensure_non_symlink_dir(&link_dir.join("child"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            reject_symlink_ancestors_under(temp.path(), &link_dir.join("file.kfx"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );

        // Reject writing through symlink parent or symlink leaf
        let victim = temp.path().join("victim.txt");
        std::fs::write(&victim, b"SAFE").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o444)).unwrap();
        let leaf_link = real_dir.join("leaf.txt");
        symlink(&victim, &leaf_link).unwrap();

        assert_eq!(
            write_bytes_local_safe(&leaf_link, b"PWNED")
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            write_bytes_local_safe(&link_dir.join("out.txt"), b"PWNED")
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(std::fs::read(&victim).unwrap(), b"SAFE");

        // Writable atomic overwrite on real read-only file succeeds via O_NOFOLLOW
        let ro_file = real_dir.join("ro.txt");
        write_bytes_local_safe(&ro_file, b"v1").unwrap();
        set_readonly_non_symlink(&ro_file, true).unwrap();
        assert!(
            std::fs::metadata(&ro_file)
                .unwrap()
                .permissions()
                .readonly()
        );
        write_bytes_local_safe(&ro_file, b"v2").unwrap();
        assert_eq!(std::fs::read(&ro_file).unwrap(), b"v2");
    }
}
