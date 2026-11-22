//! Kindle File Transfer Engine & Storage Synchronization
//!
//! Handles safe transfer of KFX books, lockscreen cover thumbnails, and Word Wise sidecars
//! over UMS and Linux GVFS MTP (`gio copy`), with atomic `.part` renames and streaming copy.

use super::DEVICE_SYNC_LOCK;
use super::catalog::clean_stale_sdr_files;
use super::sanitize::{sanitize_asin, sanitize_filename};
use super::types::SyncError;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Synchronizes a converted KFX file, multi-identifier cover thumbnails, and optional sidecars to the Kindle.
#[allow(clippy::too_many_arguments)]
pub fn sync_to_device(
    device_root: &Path,
    kfx_source: &Path,
    book_title: &str,
    asin: &str,
    content_id: Option<&str>,
    cde_content_type: Option<&str>,
    book_language: Option<&str>,
    cover_jpeg: Option<&[u8]>,
    kll_bytes: Option<&[u8]>,
) -> Result<PathBuf, SyncError> {
    let _sync_lock = DEVICE_SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let docs_dir = device_root.join("documents");
    if !docs_dir.exists() {
        return Err(SyncError::DeviceNotFound(docs_dir));
    }

    // Pre-flight check: ensure storage is not mounted read-only from previous unclean unmounts
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        if let Ok(c_path) = CString::new(docs_dir.as_os_str().as_bytes()) {
            let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
            if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } == 0
                && (stat.f_flag & libc::ST_RDONLY as libc::c_ulong) != 0
            {
                return Err(SyncError::SecurityViolation(
                    "Kindle storage is mounted read-only (EROFS, likely due to an unclean USB disconnect). Please reconnect the Kindle USB cable.".to_string(),
                ));
            }
        }
    }
    if let Ok(metadata) = docs_dir.metadata()
        && metadata.permissions().readonly()
    {
        return Err(SyncError::SecurityViolation(
                "Kindle storage is mounted read-only (likely due to an unclean USB disconnect). Please reconnect the Kindle USB cable.".to_string(),
            ));
    }

    let safe_title = kfx_source
        .file_stem()
        .and_then(|s| s.to_str())
        .map(sanitize_filename)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| sanitize_filename(book_title));
    let safe_asin = sanitize_asin(asin);

    let kfx_filename = format!("{}.kfx", safe_title);
    let dest_kfx_path = docs_dir.join(&kfx_filename);

    // Security assertion: Destination path must reside strictly within docs_dir
    if !dest_kfx_path.starts_with(&docs_dir) {
        return Err(SyncError::SecurityViolation(format!(
            "Path traversal detected in KFX target: {:?}",
            kfx_filename
        )));
    }

    // 1. Copy KFX file if source and destination differ
    if kfx_source != dest_kfx_path {
        copy_file(kfx_source, &dest_kfx_path)?;
    }

    // 2. Sync Cover Thumbnail across system/thumbnails, .system/thumbnails, and amazon-cover-bug
    if let Some(jpeg) = cover_jpeg {
        let mut thumb_dirs = Vec::new();
        if device_root.join(".system").is_dir() {
            thumb_dirs.push(device_root.join(".system").join("thumbnails"));
        }
        if device_root.join("system").is_dir() || thumb_dirs.is_empty() {
            thumb_dirs.push(device_root.join("system").join("thumbnails"));
        }
        // Calibre-compatible amazon-cover-bug directory to survive WiFi sync
        thumb_dirs.push(device_root.join("amazon-cover-bug"));

        let cde = cde_content_type.unwrap_or("EBOK");
        let alt_cde = if cde == "EBOK" { "PDOC" } else { "EBOK" };

        let mut ids_to_sync = vec![safe_asin.clone()];
        if let Some(cid) = content_id {
            let safe_cid = sanitize_asin(cid);
            if safe_cid != safe_asin && !safe_cid.is_empty() {
                ids_to_sync.push(safe_cid);
            }
        }

        for thumb_dir in thumb_dirs {
            let _ = std::fs::create_dir_all(&thumb_dir);
            for id in &ids_to_sync {
                // Primary CDE type thumbnail (e.g. thumbnail_{id}_PDOC_portrait.jpg)
                let p1 = thumb_dir.join(format!("thumbnail_{}_{}_portrait.jpg", id, cde));
                if p1.starts_with(&thumb_dir) {
                    let _ = write_bytes(&p1, jpeg);
                    if let Ok(meta) = std::fs::metadata(&p1) {
                        let mut perms = meta.permissions();
                        perms.set_readonly(true);
                        let _ = std::fs::set_permissions(&p1, perms);
                    }
                }
                // Alternate CDE type thumbnail (e.g. thumbnail_{id}_EBOK_portrait.jpg)
                let p2 = thumb_dir.join(format!("thumbnail_{}_{}_portrait.jpg", id, alt_cde));
                if p2.starts_with(&thumb_dir) {
                    let _ = write_bytes(&p2, jpeg);
                    if let Ok(meta) = std::fs::metadata(&p2) {
                        let mut perms = meta.permissions();
                        perms.set_readonly(true);
                        let _ = std::fs::set_permissions(&p2, perms);
                    }
                }
                // Legacy unqualified thumbnail (e.g. thumbnail_{id}_portrait.jpg)
                let p3 = thumb_dir.join(format!("thumbnail_{}_portrait.jpg", id));
                if p3.starts_with(&thumb_dir) {
                    let _ = write_bytes(&p3, jpeg);
                    if let Ok(meta) = std::fs::metadata(&p3) {
                        let mut perms = meta.permissions();
                        perms.set_readonly(true);
                        let _ = std::fs::set_permissions(&p3, perms);
                    }
                }
            }
        }
    }

    // 3. Ensure companion .sdr folder exists, write covers/thumbnails, and sanitize stale files
    let sdr_dir = docs_dir.join(format!("{}.sdr", safe_title));
    if !sdr_dir.starts_with(&docs_dir) {
        return Err(SyncError::SecurityViolation(format!(
            "Path traversal detected in SDR directory: {:?}",
            safe_title
        )));
    }
    // 3. Deploy companion .sdr folder, Word Wise sidecars, and thumbnails
    crate::enhancements::SdrDeployer::deploy(
        &sdr_dir,
        &safe_asin,
        content_id,
        cde_content_type,
        book_language,
        cover_jpeg,
        kll_bytes,
    )?;

    let purge_ww = kll_bytes.is_none();
    clean_stale_sdr_files(&sdr_dir, Some(&safe_asin), purge_ww);

    // Flush dirty OS page cache to physical storage before reporting completion,
    // preventing truncated files and Kindle .bad_file markers on USB disconnect.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        if let Ok(root_file) = std::fs::File::open(device_root) {
            let _ = unsafe { libc::syncfs(root_file.as_raw_fd()) };
        } else {
            unsafe { libc::sync() };
        }
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    unsafe {
        libc::sync()
    };

    // Final settle delay to ensure all FAT directory transactions are safely buffered
    std::thread::sleep(std::time::Duration::from_millis(200));

    Ok(dest_kfx_path)
}

/// Rejects destinations that already exist as symbolic links (including dangling symlinks).
fn reject_symlink_destination(dst: &Path) -> Result<(), SyncError> {
    match std::fs::symlink_metadata(dst) {
        Ok(meta) if meta.file_type().is_symlink() => Err(SyncError::SecurityViolation(format!(
            "Refusing to overwrite symlink destination: {:?}",
            dst
        ))),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SyncError::Io(e)),
    }
}

/// Clears the read-only attribute on an existing destination file so that atomic overwrites or renames succeed.
#[allow(clippy::permissions_set_readonly_false)]
pub fn prepare_destination_for_overwrite(dst: &Path) {
    if let Ok(meta) = std::fs::symlink_metadata(dst)
        && !meta.file_type().is_symlink()
        && meta.permissions().readonly()
    {
        let mut perms = meta.permissions();
        perms.set_readonly(false);
        let _ = std::fs::set_permissions(dst, perms);
    }
}

/// Robust copy using `gio copy` for Linux GVFS MTP or streamed copy for standard mounts.
/// Note: Avoid calling `sync_data()` / `sync_all()` on individual file handles, as SCSI
/// SYNCHRONIZE CACHE commands cause Kindle's USB gadget driver (fsg) to stall on slow eMMC
/// and trigger "capacity change from N to 0" (medium drop / disconnect).
pub fn copy_file(src: &Path, dst: &Path) -> Result<(), SyncError> {
    let dst_str = dst.to_string_lossy();
    if dst_str.contains("/gvfs/mtp:") {
        // Use gio copy with a 60-second watchdog timeout to circumvent MTP deadlocks
        execute_gio_copy_with_timeout(src, dst, Duration::from_secs(60))?;
    } else {
        use std::io::{Read, Write};
        reject_symlink_destination(dst)?;
        let parent = match dst.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        let mut r = std::fs::File::open(src)?;
        let mut tmp = tempfile::Builder::new()
            .prefix(".kfx_tmp.")
            .suffix(".part")
            .tempfile_in(parent)?;
        // Stream in bounded 64KB chunks to avoid overflowing Kindle USB gadget FIFO buffers
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = r.read(&mut buf)?;
            if n == 0 {
                break;
            }
            tmp.write_all(&buf[..n])?;
        }
        tmp.flush()?;
        drop(r);
        prepare_destination_for_overwrite(dst);
        tmp.persist(dst).map_err(|e| SyncError::Io(e.error))?;
        // Settle delay to allow Kindle eMMC controller to buffer writes cleanly
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

/// Spawns `gio copy` with an explicit watchdog timeout and concurrent stderr draining
/// to prevent UI/CLI thread freezes and kernel pipe buffer deadlocks.
pub fn execute_gio_copy_with_timeout(
    src: &Path,
    dst: &Path,
    timeout: Duration,
) -> Result<(), SyncError> {
    let mut child = Command::new("gio")
        .arg("copy")
        .arg("--")
        .arg(src)
        .arg(dst)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Drain stderr concurrently in a background thread to prevent the 64KB OS kernel pipe
    // buffer from filling up and deadlocking the child process on write() calls.
    let stderr_thread = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut err_msg = String::new();
            use std::io::Read;
            let _ = stderr.read_to_string(&mut err_msg);
            err_msg
        })
    });

    let start = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => {
                let err_msg = stderr_thread
                    .and_then(|t| t.join().ok())
                    .unwrap_or_default();
                if status.success() {
                    return Ok(());
                } else {
                    return Err(SyncError::GioFailed(err_msg));
                }
            }
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait(); // Reap zombie process
                    let _ = stderr_thread.and_then(|t| t.join().ok());
                    return Err(SyncError::GioFailed(format!(
                        "gio copy timed out after {} seconds",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// Writes raw bytes to device, using a temporary file and `gio copy` if on MTP.
pub fn write_bytes(dst: &Path, data: &[u8]) -> Result<(), SyncError> {
    let dst_str = dst.to_string_lossy();
    if dst_str.contains("/gvfs/mtp:") {
        let mut tmp = tempfile::NamedTempFile::new()?;
        use std::io::Write;
        tmp.write_all(data)?;
        tmp.flush()?;
        copy_file(tmp.path(), dst)?;
    } else {
        use std::io::Write;
        reject_symlink_destination(dst)?;
        let parent = match dst.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        let mut tmp = tempfile::Builder::new()
            .prefix(".kfx_tmp.")
            .suffix(".part")
            .tempfile_in(parent)?;
        tmp.write_all(data)?;
        tmp.flush()?;
        prepare_destination_for_overwrite(dst);
        tmp.persist(dst).map_err(|e| SyncError::Io(e.error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::KindleSync;

    #[test]
    fn test_sync_to_device_traversal_rejection() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join("documents");
        std::fs::create_dir_all(&docs).unwrap();

        let kfx_src = temp.path().join("source.kfx");
        std::fs::write(&kfx_src, b"FAKE_KFX_BYTES").unwrap();

        // Sync with a malicious title that attempts traversal
        let result = KindleSync::sync_to_device(
            temp.path(),
            &kfx_src,
            "../../../../etc/shadow",
            "B00TESTASIN",
            Some("1234567890ABCDEF1234567890ABCDEF"),
            Some("EBOK"),
            Some("en"),
            None,
            None,
        );
        assert!(result.is_ok());
        let synced_path = result.unwrap();
        // Ensure destination path stays strictly inside documents
        assert!(synced_path.starts_with(&docs));
        assert!(!synced_path.to_string_lossy().contains(".."));
    }

    #[test]
    fn test_sync_to_device_multi_alias_thumbnails() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join("documents");
        std::fs::create_dir_all(&docs).unwrap();

        let kfx_src = temp.path().join("book.kfx");
        std::fs::write(&kfx_src, b"FAKE_KFX").unwrap();

        let fake_jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
        let result = KindleSync::sync_to_device(
            temp.path(),
            &kfx_src,
            "My Great Novel",
            "B00TEST1234",
            Some("ABCDEF1234567890ABCDEF1234567890"),
            Some("EBOK"),
            Some("en-US"),
            Some(&fake_jpeg),
            None,
        );
        assert!(result.is_ok());

        let thumb_dir = temp.path().join("system").join("thumbnails");
        assert!(thumb_dir.is_dir());

        // 1. ASIN-keyed thumbnails (both EBOK and PDOC)
        let asin_ebok = thumb_dir.join("thumbnail_B00TEST1234_EBOK_portrait.jpg");
        let asin_pdoc = thumb_dir.join("thumbnail_B00TEST1234_PDOC_portrait.jpg");
        assert!(asin_ebok.is_file(), "ASIN EBOK thumbnail missing");
        assert!(asin_pdoc.is_file(), "ASIN PDOC thumbnail missing");

        // 2. Content-ID-keyed thumbnails (both EBOK and PDOC)
        let cid_ebok =
            thumb_dir.join("thumbnail_ABCDEF1234567890ABCDEF1234567890_EBOK_portrait.jpg");
        let cid_pdoc =
            thumb_dir.join("thumbnail_ABCDEF1234567890ABCDEF1234567890_PDOC_portrait.jpg");
        assert!(cid_ebok.is_file(), "Content-ID EBOK thumbnail missing");
        assert!(cid_pdoc.is_file(), "Content-ID PDOC thumbnail missing");

        // 3. amazon-cover-bug backup directory
        let acb_dir = temp.path().join("amazon-cover-bug");
        assert!(acb_dir.is_dir(), "amazon-cover-bug directory missing");
        assert!(
            acb_dir
                .join("thumbnail_B00TEST1234_PDOC_portrait.jpg")
                .is_file()
        );

        // 4. Companion .sdr directory covers
        let sdr_dir = docs.join("book.sdr");
        assert!(sdr_dir.is_dir(), ".sdr directory missing");
        assert!(
            sdr_dir.join("cover.jpg").is_file(),
            "cover.jpg in .sdr missing"
        );
        assert!(
            sdr_dir
                .join("thumbnail_B00TEST1234_PDOC_portrait.jpg")
                .is_file()
        );
    }

    #[test]
    fn test_gio_copy_error_reporting() {
        let dummy_src = Path::new("/nonexistent_src_12345");
        let dummy_dst = Path::new("/nonexistent_dst_67890");
        let res = KindleSync::execute_gio_copy_with_timeout(
            dummy_src,
            dummy_dst,
            Duration::from_millis(500),
        );
        // gio copy must either fail or report GioFailed
        assert!(res.is_err());
    }

    #[test]
    fn test_concurrent_stderr_drain_large_buffer() {
        // Verify that writing >128KB to stderr in a child process is drained concurrently without pipe buffer deadlocks (exceeding standard 64KB kernel buffer)
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("head -c 131072 /dev/zero | tr '\\0' 'x' >&2; exit 1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        let stderr_thread = child.stderr.take().map(|mut stderr| {
            std::thread::spawn(move || {
                let mut err_msg = String::new();
                use std::io::Read;
                let _ = stderr.read_to_string(&mut err_msg);
                err_msg
            })
        });

        let status = child.wait().unwrap();
        assert!(!status.success());
        let err_msg = stderr_thread
            .and_then(|t| t.join().ok())
            .unwrap_or_default();
        assert_eq!(err_msg.len(), 131072);
    }

    #[test]
    fn test_no_direct_file_sync_in_sync_engine() {
        // PERMANENT GUARD: Prohibit sync_data(), sync_all(), or fdatasync() in copy_file or write_bytes.
        // Calling fsync/fdatasync on USB mass storage file handles triggers SCSI SYNCHRONIZE CACHE
        // on Kindle eMMC, causing the fsg USB gadget driver to reset and drop the medium (0B).
        let base_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let transfer_path = base_dir.join("src/sync/transfer.rs");
        let source_path = if transfer_path.exists() {
            transfer_path
        } else {
            base_dir.join("src/sync.rs")
        };
        if let Ok(content) = std::fs::read_to_string(&source_path) {
            // Locate copy_file and write_bytes function bodies
            let copy_file_start = content.find("fn copy_file(").expect("fn copy_file found");
            let copy_file_end = content[copy_file_start..]
                .find("fn execute_gio_copy")
                .expect("copy_file end found");
            let copy_file_code = &content[copy_file_start..copy_file_start + copy_file_end];

            assert!(
                !copy_file_code.contains(".sync_data()") && !copy_file_code.contains(".sync_all()"),
                "FATAL REGRESSION: copy_file MUST NOT call sync_data() or sync_all() on file handles! \
                 This triggers Kindle USB gadget resets."
            );

            let write_bytes_start = content
                .find("fn write_bytes(")
                .expect("fn write_bytes found");
            let write_bytes_end = content[write_bytes_start..]
                .find("#[cfg(test)]")
                .expect("write_bytes end found");
            let write_bytes_code = &content[write_bytes_start..write_bytes_start + write_bytes_end];

            assert!(
                !write_bytes_code.contains(".sync_data()")
                    && !write_bytes_code.contains(".sync_all()"),
                "FATAL REGRESSION: write_bytes MUST NOT call sync_data() or sync_all() on file handles! \
                 This triggers Kindle USB gadget resets."
            );
        }
    }

    #[test]
    fn test_sync_overwrites_readonly_destination() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("thumbnail_test.jpg");

        // First write
        KindleSync::write_bytes(&target, b"original cover").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"original cover");

        // Make readonly
        let mut perms = std::fs::metadata(&target).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&target, perms).unwrap();
        assert!(std::fs::metadata(&target).unwrap().permissions().readonly());

        // Re-sync overwrite must succeed even when target is read-only
        KindleSync::write_bytes(&target, b"updated cover").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"updated cover");

        // Also test copy_file over a readonly destination
        let src = temp.path().join("source.kfx");
        std::fs::write(&src, b"new kfx contents").unwrap();
        let mut perms = std::fs::metadata(&target).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&target, perms).unwrap();

        KindleSync::copy_file(&src, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new kfx contents");
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_file_and_write_bytes_reject_symlink_and_preexisting_part_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        // Isolated child branch for bare relative filename destination ("rel_*.kfx") without mutating parent CWD
        if std::env::var_os("KFX_REL_TRANSFER_CHILD").is_some() {
            assert_eq!(Path::new("rel_write.kfx").parent(), Some(Path::new("")));
            KindleSync::write_bytes(Path::new("rel_write.kfx"), b"REL_WRITE_BYTES").unwrap();
            assert_eq!(std::fs::read("rel_write.kfx").unwrap(), b"REL_WRITE_BYTES");

            std::fs::write("rel_src.kfx", b"REL_COPY_BYTES").unwrap();
            KindleSync::copy_file(Path::new("rel_src.kfx"), Path::new("rel_copy.kfx")).unwrap();
            assert_eq!(std::fs::read("rel_copy.kfx").unwrap(), b"REL_COPY_BYTES");
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("src.kfx");
        std::fs::write(&src, b"NEW_KFX_PAYLOAD").unwrap();

        // 1. Live symlink destination pointing to read-only victim file
        let victim = temp.path().join("victim.txt");
        std::fs::write(&victim, b"UNTOUCHED_VICTIM").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o444)).unwrap();

        let live_link = temp.path().join("live_dest.kfx");
        symlink(&victim, &live_link).unwrap();

        let err_write = KindleSync::write_bytes(&live_link, b"EVIL").unwrap_err();
        assert!(matches!(err_write, SyncError::SecurityViolation(_)));

        let err_copy = KindleSync::copy_file(&src, &live_link).unwrap_err();
        assert!(matches!(err_copy, SyncError::SecurityViolation(_)));

        assert_eq!(std::fs::read(&victim).unwrap(), b"UNTOUCHED_VICTIM");
        assert_eq!(
            std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
            0o444
        );

        // 2. Dangling symlink destination
        let missing_target = temp.path().join("never_created.txt");
        let dangling_link = temp.path().join("dangling_dest.kfx");
        symlink(&missing_target, &dangling_link).unwrap();

        let err_dangling_w = KindleSync::write_bytes(&dangling_link, b"EVIL").unwrap_err();
        assert!(matches!(err_dangling_w, SyncError::SecurityViolation(_)));
        let err_dangling_c = KindleSync::copy_file(&src, &dangling_link).unwrap_err();
        assert!(matches!(err_dangling_c, SyncError::SecurityViolation(_)));
        assert!(!missing_target.exists());

        // 3. Pre-existing <dst>.part symlink pointing to a victim file must NOT be clobbered
        let part_victim = temp.path().join("part_victim.txt");
        std::fs::write(&part_victim, b"SAFE_PART_VICTIM").unwrap();

        let dst = temp.path().join("safe_dest.kfx");
        let legacy_part_link = temp.path().join("safe_dest.kfx.part");
        symlink(&part_victim, &legacy_part_link).unwrap();

        KindleSync::write_bytes(&dst, b"DIRECT_BYTES").unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"DIRECT_BYTES");
        assert_eq!(std::fs::read(&part_victim).unwrap(), b"SAFE_PART_VICTIM");
        assert!(
            std::fs::symlink_metadata(&legacy_part_link)
                .unwrap()
                .file_type()
                .is_symlink()
        );

        KindleSync::copy_file(&src, &dst).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"NEW_KFX_PAYLOAD");
        assert_eq!(std::fs::read(&part_victim).unwrap(), b"SAFE_PART_VICTIM");

        // 4. Relative filename destination in isolated child process CWD
        let rel_dir = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let output = Command::new(exe)
            .args([
                "--exact",
                "sync::transfer::tests::test_copy_file_and_write_bytes_reject_symlink_and_preexisting_part_symlink",
            ])
            .current_dir(rel_dir.path())
            .env("KFX_REL_TRANSFER_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Relative transfer child failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read(rel_dir.path().join("rel_write.kfx")).unwrap(),
            b"REL_WRITE_BYTES"
        );
        assert_eq!(
            std::fs::read(rel_dir.path().join("rel_copy.kfx")).unwrap(),
            b"REL_COPY_BYTES"
        );
    }
}
