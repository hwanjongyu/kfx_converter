//! Kindle File Transfer Engine & Storage Synchronization
//!
//! Handles safe transfer of KFX books, lockscreen cover thumbnails, and Word Wise sidecars
//! over UMS and Linux GVFS MTP (`gio copy`), with atomic `.part` renames and streaming copy.

use super::DEVICE_SYNC_LOCK;
use super::catalog::clean_stale_sdr_files_unlocked;
use super::sanitize::{
    collect_thumbnail_stems, sanitize_asin, sanitize_cde_content_type, sanitize_filename,
    sanitize_sidecar_stem,
};
use super::types::SyncError;
use crate::storage::{
    ensure_dir_exists_safe, is_mtp_path, prepare_writable_non_symlink,
    reject_symlink_ancestors_under, reject_symlink_path, set_readonly_non_symlink,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Returns `true` when `a` and `b` refer to the exact same underlying file on disk
/// (matching lexical equality, canonical paths, or Unix `(dev, ino)` identity).
pub(crate) fn is_same_file_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    if let (Ok(ca), Ok(cb)) = (std::fs::canonicalize(a), std::fs::canonicalize(b))
        && ca == cb
    {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b))
            && ma.dev() == mb.dev()
            && ma.ino() == mb.ino()
        {
            return true;
        }
    }
    false
}

/// Returns the in-place path inside `docs_dir` if `kfx_source` is already a regular `.kfx` file
/// residing inside `docs_dir` (e.g. `documents/Book.kfx` via a relative path or
/// `documents/Author/Book.kfx`) with no symlink ancestors under `device_root`.
fn resolve_in_place_kfx_under_docs(
    device_root: &Path,
    docs_dir: &Path,
    kfx_source: &Path,
) -> Option<PathBuf> {
    let has_kfx_ext = kfx_source
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("kfx"));
    if !has_kfx_ext {
        return None;
    }
    if std::fs::symlink_metadata(kfx_source)
        .map(|m| m.file_type().is_symlink() || !m.file_type().is_file())
        .unwrap_or(true)
    {
        return None;
    }
    let canonical_src = std::fs::canonicalize(kfx_source).ok()?;
    let canonical_docs = std::fs::canonicalize(docs_dir).ok()?;
    let rel = canonical_src.strip_prefix(&canonical_docs).ok()?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    let in_place = docs_dir.join(rel);
    if !is_mtp_path(&in_place) && reject_symlink_ancestors_under(device_root, &in_place).is_err() {
        return None;
    }
    Some(in_place)
}

/// Computes the `.sdr` directory path that `sync_to_device` will deploy to for
/// `(device_root, kfx_source, book_title)`, matching both flat and in-place subfolder targets.
pub(crate) fn expected_sync_sdr_dir(
    device_root: &Path,
    kfx_source: &Path,
    book_title: &str,
) -> PathBuf {
    let docs_dir = device_root.join("documents");
    if let Some(in_place) = resolve_in_place_kfx_under_docs(device_root, &docs_dir, kfx_source) {
        return in_place.with_extension("sdr");
    }
    let safe_title = kfx_source
        .file_stem()
        .and_then(|s| s.to_str())
        .map(sanitize_filename)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| sanitize_filename(book_title));
    docs_dir.join(format!("{}.sdr", safe_title))
}

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
    let docs_meta = match std::fs::symlink_metadata(&docs_dir) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(SyncError::DeviceNotFound(docs_dir));
        }
        Err(e) => return Err(SyncError::Io(e)),
    };
    if docs_meta.file_type().is_symlink() {
        return Err(SyncError::SecurityViolation(format!(
            "Refusing symlinked documents directory: {:?}",
            docs_dir
        )));
    }
    if !docs_meta.file_type().is_dir() {
        return Err(SyncError::DeviceNotFound(docs_dir));
    }

    // Pre-flight check: ensure storage is not mounted read-only from previous unclean unmounts
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        if let Ok(c_path) = CString::new(docs_dir.as_os_str().as_bytes()) {
            // SAFETY: `libc::statvfs` is a C POD struct where all-zero bits is a valid initial state.
            let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
            // SAFETY: `c_path` is a valid NUL-terminated `CString` and `stat` is a valid output buffer.
            if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } == 0
                && (stat.f_flag & libc::ST_RDONLY as libc::c_ulong) != 0
            {
                return Err(SyncError::SecurityViolation(
                    "Kindle storage is mounted read-only (EROFS, likely due to an unclean USB disconnect). Please reconnect the Kindle USB cable.".to_string(),
                ));
            }
        }
    }
    if docs_meta.permissions().readonly() {
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
    let safe_cid = content_id.and_then(sanitize_sidecar_stem);

    let (dest_kfx_path, sdr_dir) = if let Some(in_place) =
        resolve_in_place_kfx_under_docs(device_root, &docs_dir, kfx_source)
    {
        let in_place_sdr = in_place.with_extension("sdr");
        (in_place, in_place_sdr)
    } else {
        let kfx_filename = format!("{}.kfx", safe_title);
        (
            docs_dir.join(&kfx_filename),
            docs_dir.join(format!("{}.sdr", safe_title)),
        )
    };

    // Security assertion: Destination path must reside strictly within docs_dir with no symlink components
    if !dest_kfx_path.starts_with(&docs_dir) {
        return Err(SyncError::SecurityViolation(format!(
            "Path traversal detected in KFX target: {:?}",
            dest_kfx_path
        )));
    }
    if !is_mtp_path(&dest_kfx_path) {
        reject_symlink_ancestors_under(device_root, &dest_kfx_path)
            .map_err(|e| SyncError::SecurityViolation(e.to_string()))?;
    }

    // 1. Copy KFX file if source and destination differ
    if !is_same_file_path(kfx_source, &dest_kfx_path) {
        copy_file_unlocked(kfx_source, &dest_kfx_path)?;
    }

    // 2. Sync Cover Thumbnail across system/thumbnails, .system/thumbnails, and amazon-cover-bug
    if let Some(jpeg) = cover_jpeg {
        let is_real_dir = |p: &Path| {
            std::fs::symlink_metadata(p)
                .is_ok_and(|m| !m.file_type().is_symlink() && m.file_type().is_dir())
        };
        let mut thumb_dirs = Vec::new();
        if is_real_dir(&device_root.join(".system")) {
            thumb_dirs.push(device_root.join(".system").join("thumbnails"));
        }
        if is_real_dir(&device_root.join("system")) || thumb_dirs.is_empty() {
            thumb_dirs.push(device_root.join("system").join("thumbnails"));
        }
        // Calibre-compatible amazon-cover-bug directory to survive WiFi sync
        thumb_dirs.push(device_root.join("amazon-cover-bug"));

        let cde = sanitize_cde_content_type(cde_content_type.unwrap_or("EBOK"));
        let alt_cde = if cde == "EBOK" { "PDOC" } else { "EBOK" };
        let ids_to_sync = collect_thumbnail_stems(Some(&safe_asin), safe_cid.as_deref());

        for thumb_dir in thumb_dirs {
            if !is_mtp_path(&thumb_dir)
                && reject_symlink_ancestors_under(device_root, &thumb_dir).is_err()
            {
                continue;
            }
            if ensure_dir_exists_safe(&thumb_dir).is_err() {
                continue;
            }
            for id in &ids_to_sync {
                let thumb_names = [
                    format!("thumbnail_{}_{}_portrait.jpg", id, cde),
                    format!("thumbnail_{}_{}_portrait.jpg", id, alt_cde),
                    format!("thumbnail_{}_portrait.jpg", id),
                ];
                for name in &thumb_names {
                    let path = thumb_dir.join(name);
                    if path.starts_with(&thumb_dir)
                        && write_bytes_unlocked(&path, jpeg).is_ok()
                        && !is_mtp_path(&path)
                    {
                        let _ = set_readonly_non_symlink(&path, true);
                    }
                }
            }
        }
    }

    // 3. Ensure companion .sdr folder exists, write covers/thumbnails, and sanitize stale files
    if !sdr_dir.starts_with(&docs_dir) {
        return Err(SyncError::SecurityViolation(format!(
            "Path traversal detected in SDR directory: {:?}",
            sdr_dir
        )));
    }
    if !is_mtp_path(&sdr_dir) {
        reject_symlink_ancestors_under(device_root, &sdr_dir)
            .map_err(|e| SyncError::SecurityViolation(e.to_string()))?;
    }
    // 3. Deploy companion .sdr folder, Word Wise sidecars, and thumbnails
    crate::enhancements::SdrDeployer::deploy_unlocked(
        &sdr_dir,
        &safe_asin,
        safe_cid.as_deref(),
        cde_content_type,
        book_language,
        cover_jpeg,
        kll_bytes,
    )?;

    let purge_ww = kll_bytes.is_none();
    let _ =
        clean_stale_sdr_files_unlocked(&sdr_dir, Some(&safe_asin), safe_cid.as_deref(), purge_ww);

    // Flush dirty OS page cache to physical storage before reporting completion,
    // preventing truncated files and Kindle .bad_file markers on USB disconnect.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        if let Ok(root_file) = std::fs::File::open(device_root) {
            // SAFETY: `root_file` is an open `File` whose raw fd remains valid for the `syncfs` call.
            let _ = unsafe { libc::syncfs(root_file.as_raw_fd()) };
        } else {
            // SAFETY: `libc::sync` takes no arguments and flushes kernel buffers without pointer access.
            unsafe { libc::sync() };
        }
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    // SAFETY: `libc::sync` takes no arguments and flushes kernel buffers without pointer access.
    unsafe {
        libc::sync()
    };

    // Final settle delay to ensure all FAT directory transactions are safely buffered
    std::thread::sleep(std::time::Duration::from_millis(200));

    Ok(dest_kfx_path)
}

/// Rejects destinations that already exist as symbolic links (including dangling symlinks).
fn reject_symlink_destination(dst: &Path) -> Result<(), SyncError> {
    reject_symlink_path(dst).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            SyncError::SecurityViolation(e.to_string())
        } else {
            SyncError::Io(e)
        }
    })
}

fn reject_symlink_parent(parent: &Path) -> Result<(), SyncError> {
    crate::storage::reject_symlink_parent(parent).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            SyncError::SecurityViolation(e.to_string())
        } else {
            SyncError::Io(e)
        }
    })
}

fn prepare_writable_sync(dst: &Path) -> Result<(), SyncError> {
    match prepare_writable_non_symlink(dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(SyncError::SecurityViolation(e.to_string()))
        }
        Err(e) => Err(SyncError::Io(e)),
    }
}

/// Clears the read-only attribute on an existing non-symlink destination file so that atomic overwrites or renames succeed.
pub fn prepare_destination_for_overwrite(dst: &Path) {
    let _ = prepare_writable_non_symlink(dst);
}

/// Robust copy using `gio copy` for Linux GVFS MTP or streamed copy for standard mounts.
/// Note: Avoid calling `sync_data()` / `sync_all()` on individual file handles, as SCSI
/// SYNCHRONIZE CACHE commands cause Kindle's USB gadget driver (fsg) to stall on slow eMMC
/// and trigger "capacity change from N to 0" (medium drop / disconnect).
pub fn copy_file(src: &Path, dst: &Path) -> Result<(), SyncError> {
    let _sync_lock = DEVICE_SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    copy_file_unlocked(src, dst)
}

/// Internal unlocked copy helper for callers already holding `DEVICE_SYNC_LOCK`.
pub(crate) fn copy_file_unlocked(src: &Path, dst: &Path) -> Result<(), SyncError> {
    if is_mtp_path(dst) {
        // Use gio copy with a 60-second watchdog timeout to circumvent MTP deadlocks
        execute_gio_copy_with_timeout(src, dst, Duration::from_secs(60))?;
    } else {
        use std::io::{Read, Write};
        reject_symlink_destination(dst)?;
        let parent = match dst.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        reject_symlink_parent(parent)?;
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
        prepare_writable_sync(dst)?;
        reject_symlink_destination(dst)?;
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
    crate::storage::execute_gio_copy_with_timeout(src, dst, timeout).map_err(SyncError::GioFailed)
}

/// Writes raw bytes to device, using a temporary file and `gio copy` if on MTP.
pub fn write_bytes(dst: &Path, data: &[u8]) -> Result<(), SyncError> {
    let _sync_lock = DEVICE_SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    write_bytes_unlocked(dst, data)
}

/// Internal unlocked write helper for callers already holding `DEVICE_SYNC_LOCK`.
pub(crate) fn write_bytes_unlocked(dst: &Path, data: &[u8]) -> Result<(), SyncError> {
    if is_mtp_path(dst) {
        let mut tmp = tempfile::NamedTempFile::new()?;
        use std::io::Write;
        tmp.write_all(data)?;
        tmp.flush()?;
        execute_gio_copy_with_timeout(tmp.path(), dst, Duration::from_secs(60))?;
    } else {
        use std::io::Write;
        reject_symlink_destination(dst)?;
        let parent = match dst.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        reject_symlink_parent(parent)?;
        let mut tmp = tempfile::Builder::new()
            .prefix(".kfx_tmp.")
            .suffix(".part")
            .tempfile_in(parent)?;
        tmp.write_all(data)?;
        tmp.flush()?;
        prepare_writable_sync(dst)?;
        reject_symlink_destination(dst)?;
        tmp.persist(dst).map_err(|e| SyncError::Io(e.error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::KindleSync;
    use std::process::Command;

    #[test]
    fn test_sync_to_device_traversal_rejection() {
        assert_eq!(
            KindleSync::sanitize_filename("../../../../etc/shadow"),
            "________etc_shadow"
        );
        assert_eq!(KindleSync::sanitize_filename("CON"), "book_CON");

        let temp = tempfile::tempdir().unwrap();
        let device_root = temp.path().join("kindle_device");
        let docs = device_root.join("documents");
        let src_dir = temp.path().join("sources");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(&src_dir).unwrap();

        let cases = [
            ("...kfx", "book"),
            ("..hidden.. .kfx", "_hidden"),
            // DOS reserved filenames cannot be created as real files on Windows hosts.
            #[cfg(not(windows))]
            ("CON.kfx", "book_CON"),
        ];

        for (src_name, expected_stem) in cases {
            let kfx_src = src_dir.join(src_name);
            std::fs::write(&kfx_src, b"FAKE_KFX_BYTES").unwrap();

            let synced_path = KindleSync::sync_to_device(
                &device_root,
                &kfx_src,
                "../../../../etc/shadow",
                "../../../../etc/passwd",
                Some("../../etc/cron"),
                Some("../evil"),
                Some("../../en"),
                Some(b"FAKE_JPEG"),
                Some(b"FAKE_KLL"),
            )
            .expect("sync_to_device must sanitize traversal inputs safely");

            let expected_kfx = docs.join(format!("{}.kfx", expected_stem));
            let expected_sdr = docs.join(format!("{}.sdr", expected_stem));
            assert_eq!(synced_path, expected_kfx);
            assert!(synced_path.starts_with(&docs));
            assert!(!synced_path.to_string_lossy().contains(".."));
            assert!(expected_kfx.is_file());
            assert!(expected_sdr.is_dir());
            assert!(
                expected_sdr
                    .join("LanguageLayer.en.etcpasswd.kll")
                    .is_file()
            );
            assert!(expected_sdr.join("LanguageLayer.en.etccron.kll").is_file());
        }

        // Thumbnails must also stay strictly inside device_root using sanitized stems and EBOK fallback.
        assert!(
            device_root
                .join("system")
                .join("thumbnails")
                .join("thumbnail_etcpasswd_EBOK_portrait.jpg")
                .is_file()
        );
        assert!(
            device_root
                .join("amazon-cover-bug")
                .join("thumbnail_etccron_EBOK_portrait.jpg")
                .is_file()
        );

        // No traversal directories/files may escape device_root into the parent sandbox.
        let mut root_entries: Vec<String> = std::fs::read_dir(temp.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        root_entries.sort();
        assert_eq!(
            root_entries,
            vec!["kindle_device".to_string(), "sources".to_string()]
        );
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
        match res {
            Err(SyncError::GioFailed(msg)) => {
                assert!(
                    !msg.trim().is_empty(),
                    "GioFailed must include diagnostic output"
                );
            }
            other => panic!("Expected SyncError::GioFailed, got {:?}", other),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_run_command_with_timeout_error_and_timeout_reporting() {
        // 1. Non-zero command captures stderr deterministically
        let mut fail_cmd = Command::new("sh");
        fail_cmd
            .arg("-c")
            .arg("printf 'simulated gio copy error' >&2; exit 42");
        let output = crate::storage::run_command_with_timeout(
            "mock gio copy",
            &mut fail_cmd,
            Duration::from_secs(2),
        )
        .expect("spawning sh must succeed");
        assert_eq!(output.status.code(), Some(42));
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "simulated gio copy error"
        );

        // 2. Hung command is killed cleanly within the bounded watchdog timeout
        let mut hang_cmd = Command::new("sh");
        hang_cmd.arg("-c").arg("sleep 2");
        let err = crate::storage::run_command_with_timeout(
            "mock gio copy",
            &mut hang_cmd,
            Duration::from_millis(100),
        )
        .expect_err("hung command must return timeout error");
        assert!(
            err.contains("timed out"),
            "timeout error must mention 'timed out', got: {}",
            err
        );
    }

    #[test]
    fn test_is_same_file_path_and_in_place_subfolder_sync() {
        let temp = tempfile::tempdir().unwrap();
        let device_root = temp.path().join("mock_kindle");
        let docs = device_root.join("documents");
        let author_dir = docs.join("Author");
        std::fs::create_dir_all(&author_dir).unwrap();

        let root_kfx = docs.join("RootBook.kfx");
        std::fs::write(&root_kfx, b"ROOT_KFX_PAYLOAD").unwrap();
        let dot_alias = docs.join(".").join("RootBook.kfx");
        assert!(is_same_file_path(&root_kfx, &root_kfx));
        assert!(is_same_file_path(&root_kfx, &dot_alias));

        let sub_kfx = author_dir.join("SubBook.kfx");
        std::fs::write(&sub_kfx, b"SUB_KFX_PAYLOAD").unwrap();
        assert!(!is_same_file_path(&root_kfx, &sub_kfx));

        #[cfg(unix)]
        {
            let hard_alias = temp.path().join("hardlink_root.kfx");
            std::fs::hard_link(&root_kfx, &hard_alias).unwrap();
            assert!(is_same_file_path(&root_kfx, &hard_alias));
        }

        // 1. In-place non-canonical path directly inside documents/ must not fail or self-copy
        let synced_root = KindleSync::sync_to_device(
            &device_root,
            &dot_alias,
            "Root Book",
            "B00ROOTBOOK",
            None,
            Some("EBOK"),
            Some("en"),
            None,
            Some(b"SQLite format 3\0ROOT_KLL"),
        )
        .expect("in-place root sync must succeed");
        assert_eq!(synced_root, root_kfx);
        assert_eq!(std::fs::read(&root_kfx).unwrap(), b"ROOT_KFX_PAYLOAD");
        assert!(docs.join("RootBook.sdr").is_dir());

        // 2. In-place subfolder path documents/Author/SubBook.kfx must stay in-place without
        // creating a duplicate documents/SubBook.kfx or documents/SubBook.sdr
        let synced_sub = KindleSync::sync_to_device(
            &device_root,
            &sub_kfx,
            "Sub Book",
            "B00SUBBOOK1",
            None,
            Some("EBOK"),
            Some("en"),
            None,
            Some(b"SQLite format 3\0SUB_KLL"),
        )
        .expect("in-place subfolder sync must succeed");
        assert_eq!(synced_sub, sub_kfx);
        assert_eq!(std::fs::read(&sub_kfx).unwrap(), b"SUB_KFX_PAYLOAD");
        assert!(
            author_dir.join("SubBook.sdr").is_dir(),
            "companion .sdr must be deployed beside in-place subfolder KFX"
        );
        assert!(
            author_dir
                .join("SubBook.sdr")
                .join("LanguageLayer.en.B00SUBBOOK1.kll")
                .is_file()
        );
        assert!(
            !docs.join("SubBook.kfx").exists(),
            "must NOT create duplicate flat copy in documents/"
        );
        assert!(
            !docs.join("SubBook.sdr").exists(),
            "must NOT create duplicate flat .sdr in documents/"
        );
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

    #[test]
    fn test_sync_to_device_preserves_content_id_sidecars_and_sanitizes_cde() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join("documents");
        std::fs::create_dir_all(&docs).unwrap();

        let kfx_src = temp.path().join("content_id_book.kfx");
        std::fs::write(&kfx_src, b"KFX_BYTES").unwrap();

        let sdr_dir = docs.join("content_id_book.sdr");
        std::fs::create_dir_all(&sdr_dir).unwrap();
        // Pre-existing active X-Ray file keyed by content_id + stale ASIN file
        std::fs::write(sdr_dir.join("XRAY.entities.CID_ACTIVE_123.asc"), b"xray").unwrap();
        std::fs::write(sdr_dir.join("LanguageLayer.en.STALE_999.kll"), b"old_kll").unwrap();

        let fake_jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
        KindleSync::sync_to_device(
            temp.path(),
            &kfx_src,
            "Content ID Book",
            "123456789ABCDEF0123456789ABCDEF0",
            Some("CID_ACTIVE_123"),
            Some("../evil_cde"),
            Some("en"),
            Some(&fake_jpeg),
            Some(b"SQLite format 3\0KLL"),
        )
        .unwrap();

        assert!(
            sdr_dir
                .join("LanguageLayer.en.CID_ACTIVE_123.kll")
                .is_file()
        );
        assert!(sdr_dir.join("XRAY.entities.CID_ACTIVE_123.asc").is_file());
        assert!(!sdr_dir.join("LanguageLayer.en.STALE_999.kll").exists());
        assert!(
            temp.path()
                .join("system")
                .join("thumbnails")
                .join("thumbnail_CID_ACTIVE_123_EBOK_portrait.jpg")
                .is_file()
        );
    }

    #[test]
    fn test_sync_to_device_does_not_deadlock_under_device_sync_lock() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join("documents");
        std::fs::create_dir_all(&docs).unwrap();

        let kfx_src = temp.path().join("lock_free_sync.kfx");
        std::fs::write(&kfx_src, b"KFX_SYNC_BYTES").unwrap();

        let sdr_dir = docs.join("lock_free_sync.sdr");
        std::fs::create_dir_all(&sdr_dir).unwrap();
        std::fs::write(sdr_dir.join("stale.han"), b"stale").unwrap();

        let fake_jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
        let synced = KindleSync::sync_to_device(
            temp.path(),
            &kfx_src,
            "Lock Test Book",
            "B0LOCKTEST1",
            Some("CID_LOCK_TEST"),
            Some("EBOK"),
            Some("en"),
            Some(&fake_jpeg),
            Some(b"SQLite format 3\0KLL"),
        )
        .expect("sync_to_device must complete without deadlocking DEVICE_SYNC_LOCK");

        assert_eq!(synced, docs.join("lock_free_sync.kfx"));
        assert!(synced.is_file());
        assert!(!sdr_dir.join("stale.han").exists());
        assert!(sdr_dir.join("LanguageLayer.en.CID_LOCK_TEST.kll").is_file());

        // Standalone locked entrypoints also work immediately after sync_to_device drops the lock
        let extra_dst = temp.path().join("extra.bin");
        KindleSync::write_bytes(&extra_dst, b"EXTRA").unwrap();
        assert_eq!(std::fs::read(&extra_dst).unwrap(), b"EXTRA");

        let extra_copy = temp.path().join("extra_copy.kfx");
        KindleSync::copy_file(&kfx_src, &extra_copy).unwrap();
        assert_eq!(std::fs::read(&extra_copy).unwrap(), b"KFX_SYNC_BYTES");
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_to_device_rejects_symlinked_documents_system_and_sdr() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let src_kfx = temp.path().join("novel.kfx");
        std::fs::write(&src_kfx, b"KFX_PAYLOAD").unwrap();
        let fake_jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];

        // 1. Symlinked documents/ directory must return SecurityViolation and leave outside untouched
        let dev1 = temp.path().join("dev_sym_docs");
        std::fs::create_dir_all(&dev1).unwrap();
        symlink(&outside, dev1.join("documents")).unwrap();
        let err_docs = KindleSync::sync_to_device(
            &dev1,
            &src_kfx,
            "Novel",
            "B00SYMDOCS",
            None,
            Some("EBOK"),
            Some("en"),
            Some(&fake_jpeg),
            None,
        )
        .unwrap_err();
        assert!(matches!(err_docs, SyncError::SecurityViolation(_)));
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());

        // 2. Symlinked system/ and .system/ directories must NOT receive thumbnails outside device_root
        let dev2 = temp.path().join("dev_sym_sys");
        std::fs::create_dir_all(dev2.join("documents")).unwrap();
        let outside_sys = temp.path().join("outside_sys");
        std::fs::create_dir_all(&outside_sys).unwrap();
        let outside_dotsys = temp.path().join("outside_dotsys");
        std::fs::create_dir_all(&outside_dotsys).unwrap();
        symlink(&outside_sys, dev2.join("system")).unwrap();
        symlink(&outside_dotsys, dev2.join(".system")).unwrap();
        KindleSync::sync_to_device(
            &dev2,
            &src_kfx,
            "Novel",
            "B00SYMSYS",
            None,
            Some("EBOK"),
            Some("en"),
            Some(&fake_jpeg),
            None,
        )
        .expect(
            "sync should skip symlinked system/.system thumbnail dirs without escaping device_root",
        );
        assert!(
            std::fs::read_dir(&outside_sys).unwrap().next().is_none(),
            "Outside directory targeted by symlinked system/ must remain untouched"
        );
        assert!(
            std::fs::read_dir(&outside_dotsys).unwrap().next().is_none(),
            "Outside directory targeted by symlinked .system/ must remain untouched"
        );

        // 3. Symlinked companion .sdr directory must return SecurityViolation and leave outside untouched
        let dev3 = temp.path().join("dev_sym_sdr");
        std::fs::create_dir_all(dev3.join("documents")).unwrap();
        let outside_sdr = temp.path().join("outside_sdr");
        std::fs::create_dir_all(&outside_sdr).unwrap();
        symlink(&outside_sdr, dev3.join("documents").join("novel.sdr")).unwrap();
        let err_sdr = KindleSync::sync_to_device(
            &dev3,
            &src_kfx,
            "Novel",
            "B00SYMSDR",
            None,
            Some("EBOK"),
            Some("en"),
            Some(&fake_jpeg),
            Some(b"KLL"),
        )
        .unwrap_err();
        assert!(matches!(err_sdr, SyncError::SecurityViolation(_)));
        assert!(std::fs::read_dir(&outside_sdr).unwrap().next().is_none());

        // 4. copy_file and write_bytes reject symlinked parent directories
        let link_parent = temp.path().join("link_parent");
        symlink(&outside, &link_parent).unwrap();
        assert!(matches!(
            KindleSync::write_bytes(&link_parent.join("out.bin"), b"EVIL"),
            Err(SyncError::SecurityViolation(_))
        ));
        assert!(matches!(
            KindleSync::copy_file(&src_kfx, &link_parent.join("out.kfx")),
            Err(SyncError::SecurityViolation(_))
        ));
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
    }
}
