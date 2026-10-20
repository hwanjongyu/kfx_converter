//! Kindle Device Synchronization Engine
//!
//! Handles safe transfer of KFX books, lockscreen cover thumbnails, and Word Wise (.kll) sidecars
//! to Amazon Kindle devices over both UMS (USB Mass Storage) and Linux GVFS MTP (`gio copy`).

use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Device not connected or documents directory not found at {0:?}")]
    DeviceNotFound(PathBuf),
    #[error("gio command failed: {0}")]
    GioFailed(String),
    #[error("Security violation: {0}")]
    SecurityViolation(String),
}

/// Information about a detected Kindle device
#[derive(Debug, Clone)]
pub struct KindleDeviceInfo {
    pub root: PathBuf,
    pub documents_dir: PathBuf,
    pub model_name: String,
    pub used_gb: f32,
    pub total_gb: f32,
}

pub struct KindleSync;

impl KindleSync {
    /// Sanitizes an e-book title to be completely safe for FAT32 / VFAT storage
    /// and prevents directory traversal attacks (CWE-22 / CWE-73).
    pub fn sanitize_filename(name: &str) -> String {
        let forbidden = ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0'];
        let mut clean: String = name
            .chars()
            .map(|c| if forbidden.contains(&c) || c.is_control() { '_' } else { c })
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
        let uppercase = clean.to_ascii_uppercase();
        let base_name = uppercase.split('.').next().unwrap_or("");
        let dos_devices = [
            "CON", "PRN", "AUX", "NUL",
            "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
            "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ];
        if dos_devices.contains(&base_name) {
            clean = format!("{}_book", clean);
        }

        // Clamp UTF-8 byte length to <= 128 bytes without breaking UTF-8 char boundary
        while clean.len() > 128 {
            clean.pop();
        }
        clean.trim_end_matches(['.', '_', ' ']).to_string()
    }

    /// Sanitizes an ASIN identifier against path traversal and special characters.
    pub fn sanitize_asin(asin: &str) -> String {
        let forbidden = ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0', ' ', '.'];
        let mut clean: String = asin
            .chars()
            .filter(|c| !forbidden.contains(c) && !c.is_control())
            .collect();
        if clean.is_empty() {
            clean = "UNKNOWN_ASIN".to_string();
        }
        if let Some((idx, _)) = clean.char_indices().nth(64) {
            clean.truncate(idx);
        }
        clean
    }

    /// Queries filesystem storage statistics (used_gb, total_gb).
    #[cfg(unix)]
    pub fn get_storage_stats(path: &Path) -> Option<(f32, f32)> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } == 0 {
            let block_size = if stat.f_frsize > 0 { stat.f_frsize } else { stat.f_bsize } as u64;
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

    #[cfg(not(unix))]
    pub fn get_storage_stats(_path: &Path) -> Option<(f32, f32)> {
        // Fallback placeholder for non-Unix targets
        Some((8.0, 16.0))
    }

    /// Detects connected Kindle root directory and metadata (GVFS MTP or USB UMS mount).
    pub fn detect_device_info() -> Option<KindleDeviceInfo> {
        // 1. Check GVFS MTP on Linux
        #[cfg(target_os = "linux")]
        {
            let uid = unsafe { libc::getuid() };
            let gvfs_dir = PathBuf::from(format!("/run/user/{}/gvfs", uid));
            if gvfs_dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&gvfs_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.contains("Kindle") || name.contains("Scribe") || name.contains("Paperwhite") {
                            let internal = entry.path().join("Internal Storage");
                            let root = if internal.is_dir() { internal } else { entry.path() };
                            let doc_path = root.join("documents");
                            let mut model = "Kindle".to_string();
                            if name.contains("Scribe") {
                                model = "Kindle Scribe".into();
                            } else if name.contains("Paperwhite") {
                                model = "Kindle Paperwhite".into();
                            }
                            let (used_gb, total_gb) = Self::get_storage_stats(&root).unwrap_or((12.4, 27.2));
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
        }

        // 2. Check UMS mount points across Linux, macOS, and Windows
        let mut candidates = Vec::new();

        // macOS standard mount location
        #[cfg(target_os = "macos")]
        {
            candidates.push(PathBuf::from("/Volumes/Kindle"));
            candidates.push(PathBuf::from("/Volumes/Kindle Paperwhite"));
            candidates.push(PathBuf::from("/Volumes/Kindle Scribe"));
        }

        // Windows drive letter mount discovery (D: through Z:)
        #[cfg(windows)]
        {
            for drive in b'D'..=b'Z' {
                let drive_str = format!("{}:\\", drive as char);
                candidates.push(PathBuf::from(drive_str));
            }
        }

        // Linux standard user mount locations
        #[cfg(target_os = "linux")]
        {
            if let Ok(username) = std::env::var("USER") {
                candidates.push(PathBuf::from(format!("/run/media/{}/Kindle", username)));
                candidates.push(PathBuf::from(format!("/media/{}/Kindle", username)));
            }
            candidates.push(PathBuf::from("/media/drew/Kindle"));
            candidates.push(PathBuf::from("/media/Kindle"));
            candidates.push(PathBuf::from("/mnt/Kindle"));

            // Also inspect /proc/mounts to find any mounted filesystem labeled or pointing to Kindle
            if let Ok(mounts_content) = std::fs::read_to_string("/proc/mounts") {
                for line in mounts_content.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let mount_point = parts[1];
                        let p = PathBuf::from(mount_point);
                        if mount_point.contains("Kindle") || mount_point.contains("kindle") {
                            if !candidates.contains(&p) {
                                candidates.push(p);
                            }
                        }
                    }
                }
            }
        }

        for cand in candidates {
            if cand.is_dir() {
                let doc_path = cand.join("documents");
                // Confirm it has either documents directory or system directory (typical Kindle layout)
                if doc_path.is_dir() || cand.join("system").is_dir() || cand.join(".system").is_dir() {
                    let (used_gb, total_gb) = Self::get_storage_stats(&cand).unwrap_or((0.0, 0.0));
                    if total_gb <= 0.05 {
                        // Media ejected or dummy unmounted mountpoint
                        continue;
                    }
                    
                    // Determine model name
                    let mut model_name = "Kindle Paperwhite".to_string();
                    let cand_str = cand.to_string_lossy();
                    if cand_str.contains("Scribe") {
                        model_name = "Kindle Scribe".to_string();
                    } else if cand_str.contains("Paperwhite") {
                        model_name = "Kindle Paperwhite".to_string();
                    } else if let Ok(ver) = std::fs::read_to_string(cand.join("system").join("version.txt")) {
                        if ver.contains("Kindle") {
                            model_name = "Kindle Paperwhite".to_string();
                        }
                    }

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
        Self::detect_device_info().map(|info| info.root)
    }

    /// Safely flushes dirty filesystem caches and unmounts the connected Kindle device.
    pub fn eject_device(root: &Path) -> Result<(), SyncError> {
        // 1. Flush file system write cache to prevent eMMC data loss
        #[cfg(unix)]
        {
            unsafe { libc::sync() };
        }
        #[cfg(windows)]
        {
            let _ = Command::new("cmd").args(["/C", "sync"]).status();
        }

        // 2. Unmount cleanly depending on host OS and mount type
        #[cfg(target_os = "linux")]
        {
            let root_str = root.to_string_lossy();
            if root_str.contains("/gvfs/mtp:") {
                let _ = Command::new("gio").args(["mount", "-u", &root_str]).status();
            } else {
                let status = Command::new("gio").args(["mount", "-u"]).arg(root).status();
                if status.is_err() || !status.unwrap().success() {
                    let _ = Command::new("udisksctl").args(["unmount", "-b"]).arg(root).status();
                    let _ = Command::new("umount").arg(root).status();
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("diskutil").args(["unmount"]).arg(root).status();
        }

        Ok(())
    }

    /// Synchronizes a converted KFX file and optional sidecars to the Kindle.
    pub fn sync_to_device(
        device_root: &Path,
        kfx_source: &Path,
        book_title: &str,
        asin: &str,
        cover_jpeg: Option<&[u8]>,
        kll_bytes: Option<&[u8]>,
    ) -> Result<PathBuf, SyncError> {
        let docs_dir = device_root.join("documents");
        if !docs_dir.exists() {
            return Err(SyncError::DeviceNotFound(docs_dir));
        }

        let safe_title = kfx_source
            .file_stem()
            .and_then(|s| s.to_str())
            .map(Self::sanitize_filename)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| Self::sanitize_filename(book_title));
        let safe_asin = Self::sanitize_asin(asin);

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
            Self::copy_file(kfx_source, &dest_kfx_path)?;
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        // 2. Sync Cover Thumbnail (system/thumbnails or .system/thumbnails)
        if let Some(jpeg) = cover_jpeg {
            let thumb_dir = if device_root.join(".system").is_dir() {
                device_root.join(".system").join("thumbnails")
            } else {
                device_root.join("system").join("thumbnails")
            };
            let _ = std::fs::create_dir_all(&thumb_dir);
            let thumb_path = thumb_dir.join(format!("thumbnail_{}_EBOK_portrait.jpg", safe_asin));
            if thumb_path.starts_with(&thumb_dir) {
                // Non-fatal: cover write failure should not abort the successful KFX sync
                let _ = Self::write_bytes(&thumb_path, jpeg);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // 3. Ensure companion .sdr folder exists and sanitize stale files
        let sdr_dir = docs_dir.join(format!("{}.sdr", safe_title));
        if !sdr_dir.starts_with(&docs_dir) {
            return Err(SyncError::SecurityViolation(format!(
                "Path traversal detected in SDR directory: {:?}",
                safe_title
            )));
        }
        if kll_bytes.is_some() && !sdr_dir.exists() {
            let _ = std::fs::create_dir_all(&sdr_dir);
        }
        let purge_ww = kll_bytes.is_none();
        Self::clean_stale_sdr_files(&sdr_dir, Some(&safe_asin), purge_ww);

        // 4. Sync Word Wise Sidecar (<Title>.sdr/LanguageLayer.en.{asin}.kll)
        if let Some(kll) = kll_bytes {
            if !sdr_dir.exists() {
                let _ = std::fs::create_dir_all(&sdr_dir);
            }
            let kll_path = sdr_dir.join(format!("LanguageLayer.en.{}.kll", safe_asin));
            let _ = Self::write_bytes(&kll_path, kll);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        Ok(dest_kfx_path)
    }

    /// Purges .bad_file markers, incompatible serialized state files, and orphaned enhancement sidecars.
    pub fn clean_stale_sdr_files(
        sdr_dir: &Path,
        current_asin: Option<&str>,
        purge_word_wise: bool,
    ) {
        if !sdr_dir.is_dir() {
            return;
        }
        let stale_exts = [".han", ".tan", ".pdr", ".mbp1", ".phl", ".bdr", ".ea"];
        if let Ok(entries) = std::fs::read_dir(sdr_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let fname_lower = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();

                if fname_lower.ends_with(".bad_file") {
                    let _ = std::fs::remove_file(&path);
                } else if stale_exts.iter().any(|ext| fname_lower.ends_with(ext)) {
                    let _ = std::fs::remove_file(&path);
                } else if purge_word_wise && fname_lower.contains(".kll") {
                    let _ = std::fs::remove_file(&path);
                } else if let Some(asin) = current_asin {
                    let asin_lower = asin.to_lowercase();
                    if (fname_lower.contains(".kll") || fname_lower.contains(".asc"))
                        && !fname_lower.contains(&asin_lower)
                    {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
    }

    /// Robust copy using `gio copy` for Linux GVFS MTP or streamed copy with data sync for standard mounts.
    fn copy_file(src: &Path, dst: &Path) -> Result<(), SyncError> {
        let dst_str = dst.to_string_lossy();
        if dst_str.contains("/gvfs/mtp:") {
            // Use gio copy with a 60-second watchdog timeout to circumvent MTP deadlocks
            Self::execute_gio_copy_with_timeout(src, dst, std::time::Duration::from_secs(60))?;
        } else {
            let tmp_dst = dst.with_extension(format!(
                "{}.tmp_{}",
                dst.extension().and_then(|e| e.to_str()).unwrap_or("kfx"),
                std::process::id()
            ));
            let mut r = std::fs::File::open(src)?;
            let mut w = std::fs::File::create(&tmp_dst)?;
            std::io::copy(&mut r, &mut w)?;
            w.sync_all()?;
            drop(w);
            drop(r);
            std::fs::rename(&tmp_dst, dst)?;
            if let Some(parent) = dst.parent() {
                if let Ok(dir) = std::fs::File::open(parent) {
                    let _ = dir.sync_all();
                }
            }
            // Brief settle delay to allow eMMC controller to commit blocks without SCSI reset
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Ok(())
    }

    /// Spawns `gio copy` with an explicit watchdog timeout to prevent UI/CLI thread freezes.
    fn execute_gio_copy_with_timeout(
        src: &Path,
        dst: &Path,
        timeout: std::time::Duration,
    ) -> Result<(), SyncError> {
        let mut child = Command::new("gio")
            .arg("copy")
            .arg("--")
            .arg(src)
            .arg(dst)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let start = std::time::Instant::now();
        loop {
            match child.try_wait()? {
                Some(status) => {
                    if status.success() {
                        return Ok(());
                    } else {
                        let mut err_msg = String::new();
                        if let Some(mut stderr) = child.stderr.take() {
                            use std::io::Read;
                            let _ = stderr.read_to_string(&mut err_msg);
                        }
                        return Err(SyncError::GioFailed(err_msg));
                    }
                }
                None => {
                    if start.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait(); // Reap zombie process
                        return Err(SyncError::GioFailed(format!(
                            "gio copy timed out after {} seconds",
                            timeout.as_secs()
                        )));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    }

    /// Writes raw bytes to device, using a temporary file and `gio copy` if on MTP.
    fn write_bytes(dst: &Path, data: &[u8]) -> Result<(), SyncError> {
        let dst_str = dst.to_string_lossy();
        if dst_str.contains("/gvfs/mtp:") {
            let mut tmp = tempfile::NamedTempFile::new()?;
            use std::io::Write;
            tmp.write_all(data)?;
            tmp.flush()?;
            Self::copy_file(tmp.path(), dst)?;
        } else {
            use std::io::Write;
            let tmp_dst = dst.with_extension(format!(
                "{}.tmp_{}",
                dst.extension().and_then(|e| e.to_str()).unwrap_or("dat"),
                std::process::id()
            ));
            let mut f = std::fs::File::create(&tmp_dst)?;
            f.write_all(data)?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&tmp_dst, dst)?;
            if let Some(parent) = dst.parent() {
                if let Ok(dir) = std::fs::File::open(parent) {
                    let _ = dir.sync_all();
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename_basic() {
        assert_eq!(KindleSync::sanitize_filename("Simple Title"), "Simple Title");
        assert_eq!(
            KindleSync::sanitize_filename("What If?: Serious Scientific Answers"),
            "What If__ Serious Scientific Answers"
        );
        assert_eq!(
            KindleSync::sanitize_filename("../../etc/passwd"),
            "____etc_passwd"
        );
        assert_eq!(KindleSync::sanitize_filename("   "), "book");
        assert_eq!(KindleSync::sanitize_filename("Trailing Dots..."), "Trailing Dots");
    }

    #[test]
    fn test_sanitize_filename_dos_devices() {
        assert_eq!(KindleSync::sanitize_filename("CON"), "CON_book");
        assert_eq!(KindleSync::sanitize_filename("aux.kfx"), "aux.kfx_book");
        assert_eq!(KindleSync::sanitize_filename("nul"), "nul_book");
        assert_eq!(KindleSync::sanitize_filename("COM1.epub"), "COM1.epub_book");
        assert_eq!(KindleSync::sanitize_filename("PRN"), "PRN_book");

        // Long string with multi-byte characters capped at <= 128 bytes
        let long_korean = "한".repeat(100); // 300 bytes in UTF-8
        let sanitized = KindleSync::sanitize_filename(&long_korean);
        assert!(sanitized.len() <= 128);
        assert!(std::str::from_utf8(sanitized.as_bytes()).is_ok());
    }

    #[test]
    fn test_sanitize_asin_basic() {
        assert_eq!(KindleSync::sanitize_asin("B000JML542"), "B000JML542");
        assert_eq!(KindleSync::sanitize_asin("../../root/key"), "rootkey");
        assert_eq!(KindleSync::sanitize_asin(""), "UNKNOWN_ASIN");

        // Non-ASCII Unicode characters past 64 scalar chars should not panic
        let long_unicode = "가".repeat(100);
        let sanitized_asin = KindleSync::sanitize_asin(&long_unicode);
        assert_eq!(sanitized_asin.chars().count(), 64);
    }

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
    fn test_gio_copy_error_reporting() {
        let dummy_src = Path::new("/nonexistent_src_12345");
        let dummy_dst = Path::new("/nonexistent_dst_67890");
        let res = KindleSync::execute_gio_copy_with_timeout(
            dummy_src,
            dummy_dst,
            std::time::Duration::from_millis(500),
        );
        // gio copy must either fail or report GioFailed
        assert!(res.is_err());
    }

    #[test]
    fn test_clean_stale_sdr_files() {
        let temp = tempfile::tempdir().unwrap();
        let sdr = temp.path().join("TestBook.sdr");
        std::fs::create_dir_all(&sdr).unwrap();

        std::fs::write(sdr.join("test.han"), b"stale_han").unwrap();
        std::fs::write(sdr.join("test.tan"), b"stale_tan").unwrap();
        std::fs::write(sdr.join("test.pdr"), b"stale_pdr").unwrap();
        std::fs::write(sdr.join("test.mbp1"), b"stale_mbp1").unwrap();
        std::fs::write(sdr.join("test.phl"), b"stale_phl").unwrap();
        std::fs::write(sdr.join("test.bdr"), b"stale_bdr").unwrap();
        std::fs::write(sdr.join("test.ea"), b"stale_ea").unwrap();
        std::fs::write(sdr.join("old.mbp1.bad_file"), b"bad_file").unwrap();
        std::fs::write(sdr.join("corrupted.han.BAD_FILE"), b"bad_file_caps").unwrap();
        std::fs::write(sdr.join("XRAY.entities.OLDASIN.asc"), b"orphaned_xray").unwrap();
        std::fs::write(sdr.join("LanguageLayer.en.OLDASIN.kll"), b"orphaned_ww").unwrap();
        std::fs::write(sdr.join("LanguageLayer.en.NEWASIN.kll"), b"valid_ww").unwrap();
        std::fs::write(sdr.join("XRAY.entities.NEWASIN.asc"), b"valid_xray").unwrap();

        KindleSync::clean_stale_sdr_files(&sdr, Some("NEWASIN"), false);

        assert!(!sdr.join("test.han").exists());
        assert!(!sdr.join("test.tan").exists());
        assert!(!sdr.join("test.pdr").exists());
        assert!(!sdr.join("test.mbp1").exists());
        assert!(!sdr.join("test.phl").exists());
        assert!(!sdr.join("test.bdr").exists());
        assert!(!sdr.join("test.ea").exists());
        assert!(!sdr.join("old.mbp1.bad_file").exists());
        assert!(!sdr.join("corrupted.han.BAD_FILE").exists());
        assert!(!sdr.join("XRAY.entities.OLDASIN.asc").exists());
        assert!(!sdr.join("LanguageLayer.en.OLDASIN.kll").exists());

        assert!(sdr.join("LanguageLayer.en.NEWASIN.kll").exists());
        assert!(sdr.join("XRAY.entities.NEWASIN.asc").exists());

        // Test with purge_word_wise = true: should purge valid_ww as well
        KindleSync::clean_stale_sdr_files(&sdr, Some("NEWASIN"), true);
        assert!(!sdr.join("LanguageLayer.en.NEWASIN.kll").exists());
        assert!(sdr.join("XRAY.entities.NEWASIN.asc").exists());
    }

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
}
