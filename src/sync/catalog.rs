//! On-Device Catalog & Maintenance for Amazon Kindle
//!
//! Handles scanning documents directories for e-books, verifying companion `.sdr` sidecars,
//! enforcing whitelist-safe deletion, and purging stale or corrupt files.

use super::DEVICE_SYNC_LOCK;
use super::sanitize::collect_sidecar_stems;
use super::types::{KindleBookFile, SyncError};
use std::collections::HashSet;
use std::path::Path;

/// Scans the Kindle documents directory (including author subfolders up to max_depth 3) for valid e-book files.
pub fn scan_device_files(doc_path: &Path) -> Vec<KindleBookFile> {
    let _sync_lock = DEVICE_SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut files = Vec::new();
    let Ok(root_meta) = std::fs::symlink_metadata(doc_path) else {
        return files;
    };
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return files;
    }

    fn scan_dir_recursive(
        doc_path: &Path,
        dir: &Path,
        depth: usize,
        max_depth: usize,
        files: &mut Vec<KindleBookFile>,
    ) {
        if depth > max_depth {
            return;
        }
        let valid_exts = ["kfx", "azw3", "mobi", "pdf", "txt"];
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(meta) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if meta.file_type().is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    if !name.starts_with('.') && !name.ends_with(".sdr") {
                        scan_dir_recursive(doc_path, &path, depth + 1, max_depth, files);
                    }
                } else if meta.is_file() {
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_lowercase())
                        .unwrap_or_default();

                    if valid_exts.contains(&ext.as_str()) {
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let format = ext.to_uppercase();
                        let bytes = meta.len();
                        let size_formatted = if bytes >= 1024 * 1024 {
                            format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
                        } else {
                            format!("{} KB", bytes / 1024)
                        };

                        let (modified_timestamp, modified) = match meta.modified() {
                            Ok(time) => {
                                let ts = time
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0);
                                let duration = time.elapsed().unwrap_or_default();
                                let s = if duration.as_secs() < 60 {
                                    "Just now".into()
                                } else if duration.as_secs() < 3600 {
                                    format!("{}m ago", duration.as_secs() / 60)
                                } else if duration.as_secs() < 86400 {
                                    format!("{}h ago", duration.as_secs() / 3600)
                                } else {
                                    format!("{}d ago", duration.as_secs() / 86400)
                                };
                                (ts, s)
                            }
                            Err(_) => (0, "Unknown".into()),
                        };

                        let (title, author) = if let Ok(rel) = path.strip_prefix(doc_path) {
                            let components: Vec<_> = rel.iter().collect();
                            if components.len() > 1 {
                                let first_dir = components[0].to_string_lossy();
                                if first_dir.eq_ignore_ascii_case("downloads")
                                    || first_dir.eq_ignore_ascii_case("system")
                                {
                                    let stem =
                                        path.file_stem().unwrap_or_default().to_string_lossy();
                                    if let Some((t, a)) = stem.split_once(" - ") {
                                        (t.trim().to_string(), a.trim().to_string())
                                    } else {
                                        (stem.to_string(), "Unknown".to_string())
                                    }
                                } else {
                                    let author_str = first_dir.to_string();
                                    let title_str = path
                                        .file_stem()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_string();
                                    (title_str, author_str)
                                }
                            } else {
                                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                                if let Some((t, a)) = stem.split_once(" - ") {
                                    (t.trim().to_string(), a.trim().to_string())
                                } else {
                                    (stem.to_string(), "Unknown".to_string())
                                }
                            }
                        } else {
                            (
                                path.file_stem()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                                "Unknown".to_string(),
                            )
                        };

                        let file_stem = path.file_stem().unwrap_or_default().to_string_lossy();
                        let parent = path.parent().unwrap_or(dir);
                        let sdr_path = parent.join(format!("{}.sdr", file_stem));
                        let has_sdr = std::fs::symlink_metadata(&sdr_path)
                            .is_ok_and(|m| !m.file_type().is_symlink() && m.is_dir());
                        let mut has_word_wise = false;
                        if has_sdr && let Ok(sdr_entries) = std::fs::read_dir(&sdr_path) {
                            for sdr_entry in sdr_entries.flatten() {
                                let sdr_name =
                                    sdr_entry.file_name().to_string_lossy().to_lowercase();
                                if sdr_name.ends_with(".kll")
                                    && std::fs::symlink_metadata(sdr_entry.path())
                                        .is_ok_and(|m| !m.file_type().is_symlink() && m.is_file())
                                {
                                    has_word_wise = true;
                                    break;
                                }
                            }
                        }

                        files.push(KindleBookFile {
                            name,
                            title,
                            author,
                            format,
                            size_formatted,
                            size_bytes: bytes,
                            modified,
                            modified_timestamp,
                            full_path: path,
                            has_word_wise,
                            has_sdr,
                        });
                    }
                }
            }
        }
    }

    scan_dir_recursive(doc_path, doc_path, 0, 3, &mut files);
    files.sort_by_key(|a| a.name.to_lowercase());
    files
}

/// Deletes an e-book file and its companion `.sdr` directory from the device safely.
///
/// # Security Guardrails:
/// - Rejects non-ebook files (only allows .kfx, .azw3, .azw, .mobi, .epub, .pdf, .txt, .prc).
/// - If `allowed_root` is provided, ensures `full_path` is strictly contained within `allowed_root`.
/// - Verifies `.sdr` directory extension before recursive removal to prevent path traversal.
pub fn delete_book_file_safe(
    allowed_root: Option<&Path>,
    full_path: &Path,
) -> Result<(), std::io::Error> {
    let _sync_lock = DEVICE_SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let ext = full_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let allowed_exts = ["kfx", "azw3", "azw", "mobi", "epub", "pdf", "txt", "prc"];
    if !allowed_exts.contains(&ext.as_str()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Refusing to delete non-ebook file {:?}", full_path),
        ));
    }

    let file_name = full_path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Target path {:?} has no file name", full_path),
        )
    })?;
    let file_stem = full_path.file_stem().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Target path {:?} has no file stem", full_path),
        )
    })?;
    let raw_parent = match full_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    let target_exists = match std::fs::symlink_metadata(full_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("Refusing to delete symlink target {:?}", full_path),
            ));
        }
        Ok(meta) if !meta.file_type().is_file() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Refusing to delete non-regular target {:?}", full_path),
            ));
        }
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && allowed_root.is_none() => false,
        Err(e) => return Err(e),
    };

    let (target_path, effective_parent, canon_root_opt) = if let Some(root) = allowed_root {
        let canon_root = root.canonicalize()?;
        let canon_parent = raw_parent.canonicalize()?;
        if !canon_parent.starts_with(&canon_root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "Target parent {:?} is outside allowed directory {:?}",
                    raw_parent, root
                ),
            ));
        }
        let target_path = canon_parent.join(file_name);
        let re_meta = std::fs::symlink_metadata(&target_path)?;
        if re_meta.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("Refusing to delete symlink target {:?}", target_path),
            ));
        }
        if !re_meta.file_type().is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Refusing to delete non-regular target {:?}", target_path),
            ));
        }
        let canon_target = target_path.canonicalize()?;
        if !canon_target.starts_with(&canon_root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "Target path {:?} is outside allowed directory {:?}",
                    full_path, root
                ),
            ));
        }
        (target_path, canon_parent, Some(canon_root))
    } else {
        (full_path.to_path_buf(), raw_parent.to_path_buf(), None)
    };

    let mut part_name = std::ffi::OsString::from(file_name);
    part_name.push(".part");
    let part_path = effective_parent.join(part_name);

    let mut sdr_name = std::ffi::OsString::from(file_stem);
    sdr_name.push(".sdr");
    let sdr_path = effective_parent.join(sdr_name);

    if target_exists {
        std::fs::remove_file(&target_path)?;
    }

    if let Ok(part_meta) = std::fs::symlink_metadata(&part_path)
        && part_meta.file_type().is_file()
    {
        if let Some(ref canon_root) = canon_root_opt {
            let canon_part = part_path.canonicalize()?;
            if !canon_part.starts_with(canon_root) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!(
                        "Part path {:?} is outside allowed directory {:?}",
                        part_path, canon_root
                    ),
                ));
            }
        }
        let _ = std::fs::remove_file(&part_path);
    }

    if let Ok(sdr_meta) = std::fs::symlink_metadata(&sdr_path)
        && sdr_meta.file_type().is_dir()
        && sdr_path.extension() == Some(std::ffi::OsStr::new("sdr"))
    {
        if let Some(ref canon_root) = canon_root_opt {
            let canon_sdr = sdr_path.canonicalize()?;
            if !canon_sdr.starts_with(canon_root) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!(
                        "SDR path {:?} is outside allowed directory {:?}",
                        sdr_path, canon_root
                    ),
                ));
            }
        }
        let _ = std::fs::remove_dir_all(&sdr_path);
    }

    Ok(())
}

/// Deletes an e-book file and its companion `.sdr` directory from the device.
pub fn delete_book_file(full_path: &Path) -> Result<(), std::io::Error> {
    delete_book_file_safe(None, full_path)
}

/// Internal unlocked helper for [`clean_stale_sdr_files_with_content_id`].
/// Caller must already hold `DEVICE_SYNC_LOCK`.
pub(crate) fn clean_stale_sdr_files_unlocked(
    sdr_dir: &Path,
    current_asin: Option<&str>,
    current_content_id: Option<&str>,
    purge_word_wise: bool,
) -> Result<(), SyncError> {
    let Ok(dir_meta) = std::fs::symlink_metadata(sdr_dir) else {
        return Ok(());
    };
    if dir_meta.file_type().is_symlink() || !dir_meta.is_dir() {
        return Ok(());
    }

    let stale_exts = [
        ".han", ".tan", ".pdr", ".mbp1", ".phl", ".bdr", ".ea", ".yjf", ".yjr", ".part",
    ];
    let valid_stems: HashSet<String> = collect_sidecar_stems(current_asin, current_content_id)
        .into_iter()
        .map(|s| s.to_lowercase())
        .collect();

    for entry in std::fs::read_dir(sdr_dir)?.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_file() {
            let fname_lower = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_lowercase())
                .unwrap_or_default();

            if fname_lower.contains(".bad_file")
                || stale_exts.iter().any(|ext| fname_lower.ends_with(ext))
                || (purge_word_wise && fname_lower.ends_with(".kll"))
            {
                let _ = std::fs::remove_file(&path);
            } else if (fname_lower.ends_with(".kll") || fname_lower.ends_with(".asc"))
                && !valid_stems.is_empty()
            {
                let stem_id = path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .and_then(|s| s.rsplit('.').next())
                    .map(|s| s.to_lowercase());
                if !stem_id.is_some_and(|id| valid_stems.contains(&id)) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        } else if meta.is_dir() && path.file_name().is_some_and(|n| n == "data") {
            let pag_cache = path.join(".pagination.cache");
            if let Ok(cache_meta) = std::fs::symlink_metadata(&pag_cache)
                && !cache_meta.file_type().is_symlink()
                && cache_meta.is_dir()
            {
                let _ = std::fs::remove_dir_all(&pag_cache);
            }
        }
    }

    Ok(())
}

/// Purges `.bad_file` markers, incompatible serialized state files, and orphaned enhancement
/// sidecars while preserving `.kll`/`.asc` entries matching `current_asin` (including its
/// 10-character canonical ASIN alias) or `current_content_id` when `purge_word_wise == false`.
pub fn clean_stale_sdr_files_with_content_id(
    sdr_dir: &Path,
    current_asin: Option<&str>,
    current_content_id: Option<&str>,
    purge_word_wise: bool,
) -> Result<(), SyncError> {
    let _sync_lock = DEVICE_SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clean_stale_sdr_files_unlocked(sdr_dir, current_asin, current_content_id, purge_word_wise)
}

/// Purges .bad_file markers, incompatible serialized state files, and orphaned enhancement sidecars.
pub fn clean_stale_sdr_files(sdr_dir: &Path, current_asin: Option<&str>, purge_word_wise: bool) {
    let _ = clean_stale_sdr_files_with_content_id(sdr_dir, current_asin, None, purge_word_wise);
}

#[cfg(test)]
mod tests {
    use crate::sync::KindleSync;

    #[test]
    fn test_sync_scan_device_files_and_delete() {
        let temp_dir = tempfile::tempdir().unwrap();
        let doc_root = temp_dir.path();

        let book1_kfx = doc_root.join("Novel.kfx");
        std::fs::write(&book1_kfx, b"KFX_DATA").unwrap();

        let sdr_dir = doc_root.join("Novel.sdr");
        std::fs::create_dir_all(&sdr_dir).unwrap();
        std::fs::write(sdr_dir.join("Novel.kll"), b"SQLITE_KLL_DATA").unwrap();

        let sub_dir = doc_root.join("Author Name");
        std::fs::create_dir_all(&sub_dir).unwrap();
        let book2_mobi = sub_dir.join("Story.mobi");
        std::fs::write(&book2_mobi, b"MOBI_DATA").unwrap();

        let scanned = KindleSync::scan_device_files(doc_root);
        assert_eq!(scanned.len(), 2, "Expected 2 scanned books");

        let kfx_item = scanned
            .iter()
            .find(|b| b.name == "Novel.kfx")
            .expect("Novel.kfx found");
        assert_eq!(kfx_item.format, "KFX");
        assert!(kfx_item.has_word_wise);
        assert!(kfx_item.has_sdr);

        let mobi_item = scanned
            .iter()
            .find(|b| b.name == "Story.mobi")
            .expect("Story.mobi found");
        assert_eq!(mobi_item.format, "MOBI");
        assert!(!mobi_item.has_word_wise);

        // Delete book1 and its sidecar
        KindleSync::delete_book_file(&book1_kfx).expect("Delete book file");
        assert!(!book1_kfx.exists());
        assert!(!sdr_dir.exists());

        let scanned_after = KindleSync::scan_device_files(doc_root);
        assert_eq!(scanned_after.len(), 1);
        assert_eq!(scanned_after[0].name, "Story.mobi");
    }

    #[test]
    fn test_delete_book_file_safe_guards() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join("documents");
        std::fs::create_dir_all(&docs).unwrap();

        // 1. Valid ebook deletion
        let book_kfx = docs.join("SafeBook.kfx");
        let book_kfx_part = docs.join("SafeBook.kfx.part");
        let book_sdr = docs.join("SafeBook.sdr");
        std::fs::write(&book_kfx, b"dummy kfx").unwrap();
        std::fs::write(&book_kfx_part, b"part data").unwrap();
        std::fs::create_dir_all(&book_sdr).unwrap();
        std::fs::write(book_sdr.join("cache.bin"), b"cache").unwrap();

        assert!(KindleSync::delete_book_file_safe(Some(&docs), &book_kfx).is_ok());
        assert!(!book_kfx.exists());
        assert!(!book_kfx_part.exists());
        assert!(!book_sdr.exists());

        // 2. Reject non-ebook files (e.g. .rs or .sh)
        let script = docs.join("run.sh");
        std::fs::write(&script, b"echo hello").unwrap();
        let res = KindleSync::delete_book_file_safe(Some(&docs), &script);
        assert!(res.is_err());
        assert!(script.exists()); // must not be deleted

        // 3. Reject file outside allowed_root
        let outside_dir = temp.path().join("outside");
        std::fs::create_dir_all(&outside_dir).unwrap();
        let outside_book = outside_dir.join("Outside.kfx");
        std::fs::write(&outside_book, b"outside").unwrap();
        let res = KindleSync::delete_book_file_safe(Some(&docs), &outside_book);
        assert!(res.is_err());
        assert!(outside_book.exists()); // must not be deleted

        // 4. Fail-closed: non-existent file path outside root must return error
        let non_existent = outside_dir.join("NonExistent.kfx");
        let res_missing = KindleSync::delete_book_file_safe(Some(&docs), &non_existent);
        assert!(
            res_missing.is_err(),
            "Missing file with allowed_root must fail closed"
        );
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
        std::fs::write(sdr.join("test.yjf"), b"stale_yjf").unwrap();
        std::fs::write(sdr.join("test.yjr"), b"stale_yjr").unwrap();
        std::fs::write(sdr.join("aborted_transfer.part"), b"stale_part").unwrap();
        let pag_cache = sdr.join("data").join(".pagination.cache");
        std::fs::create_dir_all(&pag_cache).unwrap();
        std::fs::write(pag_cache.join("cache.bin"), b"stale_pag").unwrap();
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
        assert!(!sdr.join("test.yjf").exists());
        assert!(!sdr.join("test.yjr").exists());
        assert!(!sdr.join("aborted_transfer.part").exists());
        assert!(!sdr.join("data").join(".pagination.cache").exists());
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
    fn test_clean_stale_sdr_files_preserves_content_id_sidecars() {
        let temp = tempfile::tempdir().unwrap();
        let sdr = temp.path().join("Book.sdr");
        std::fs::create_dir_all(&sdr).unwrap();

        let asin = "123456789ABCDEF0123456789ABCDEF0";
        let asin_10 = "B123456789";
        let cid = "CID_DIFFERENT_987654321";

        let keep_asin_kll = sdr.join(format!("LanguageLayer.en.{}.kll", asin));
        let keep_asin10_kll = sdr.join(format!("LanguageLayer.en.{}.kll", asin_10));
        let keep_cid_kll = sdr.join(format!("LanguageLayer.en.{}.kll", cid));
        let keep_cid_asc = sdr.join(format!("XRAY.entities.{}.asc", cid));
        let stale_kll = sdr.join("LanguageLayer.en.OLDSTALE.kll");
        let stale_asc = sdr.join("XRAY.entities.OLDSTALE.asc");

        for p in [
            &keep_asin_kll,
            &keep_asin10_kll,
            &keep_cid_kll,
            &keep_cid_asc,
            &stale_kll,
            &stale_asc,
        ] {
            std::fs::write(p, b"data").unwrap();
        }

        KindleSync::clean_stale_sdr_files_with_content_id(&sdr, Some(asin), Some(cid), false)
            .unwrap();

        assert!(keep_asin_kll.exists());
        assert!(keep_asin10_kll.exists());
        assert!(keep_cid_kll.exists());
        assert!(keep_cid_asc.exists());
        assert!(!stale_kll.exists());
        assert!(!stale_asc.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_delete_book_file_safe_symlink_and_jail_hardening() {
        use std::io::ErrorKind;
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join("documents");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        // 1. Symlink outside allowed_root pointing to a .kfx inside allowed_root
        let inside_target = docs.join("InsideTarget.kfx");
        std::fs::write(&inside_target, b"INSIDE_DATA").unwrap();
        let outside_link = outside.join("AliasToInside.kfx");
        symlink(&inside_target, &outside_link).unwrap();
        let outside_link_part = outside.join("AliasToInside.kfx.part");
        std::fs::write(&outside_link_part, b"OUTSIDE_PART").unwrap();

        let err1 = KindleSync::delete_book_file_safe(Some(&docs), &outside_link).unwrap_err();
        assert_eq!(err1.kind(), ErrorKind::PermissionDenied);
        assert!(outside_link.symlink_metadata().is_ok());
        assert!(outside_link_part.exists());
        assert_eq!(std::fs::read(&inside_target).unwrap(), b"INSIDE_DATA");

        // 2. Symlink inside allowed_root pointing to a file outside allowed_root
        let outside_victim = outside.join("Victim.kfx");
        std::fs::write(&outside_victim, b"KEEP_OUTSIDE").unwrap();
        let inside_escape_link = docs.join("Escape.kfx");
        symlink(&outside_victim, &inside_escape_link).unwrap();

        let err2 = KindleSync::delete_book_file_safe(Some(&docs), &inside_escape_link).unwrap_err();
        assert_eq!(err2.kind(), ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read(&outside_victim).unwrap(), b"KEEP_OUTSIDE");

        let err2_unjailed = KindleSync::delete_book_file(&inside_escape_link).unwrap_err();
        assert_eq!(err2_unjailed.kind(), ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read(&outside_victim).unwrap(), b"KEEP_OUTSIDE");

        // 3. Symlinked intermediate parent directory inside allowed_root pointing outside
        let outside_subdir = outside.join("escaped_dir");
        std::fs::create_dir_all(&outside_subdir).unwrap();
        let outside_nested_file = outside_subdir.join("Nested.kfx");
        std::fs::write(&outside_nested_file, b"NESTED_VICTIM").unwrap();
        let inside_dir_link = docs.join("dir_escape");
        symlink(&outside_subdir, &inside_dir_link).unwrap();

        let err3 =
            KindleSync::delete_book_file_safe(Some(&docs), &inside_dir_link.join("Nested.kfx"))
                .unwrap_err();
        assert_eq!(err3.kind(), ErrorKind::PermissionDenied);
        assert_eq!(
            std::fs::read(&outside_nested_file).unwrap(),
            b"NESTED_VICTIM"
        );

        // 4. .sdr symlink and .part symlink inside allowed_root pointing outside must never touch outside targets
        let victim_sdr_dir = outside.join("VictimSdr");
        std::fs::create_dir_all(&victim_sdr_dir).unwrap();
        let victim_sdr_file = victim_sdr_dir.join("keep.txt");
        std::fs::write(&victim_sdr_file, b"KEEP_SDR").unwrap();

        let victim_part_file = outside.join("VictimPart.bin");
        std::fs::write(&victim_part_file, b"KEEP_PART").unwrap();

        let real_book = docs.join("CleanBook.kfx");
        std::fs::write(&real_book, b"CLEAN_KFX").unwrap();
        symlink(&victim_sdr_dir, docs.join("CleanBook.sdr")).unwrap();
        symlink(&victim_part_file, docs.join("CleanBook.kfx.part")).unwrap();

        KindleSync::delete_book_file_safe(Some(&docs), &real_book).unwrap();
        assert!(!real_book.exists());
        assert_eq!(std::fs::read(&victim_sdr_file).unwrap(), b"KEEP_SDR");
        assert_eq!(std::fs::read(&victim_part_file).unwrap(), b"KEEP_PART");

        // 5. Directory named fake.kfx/ must return InvalidInput and preserve fake.kfx/ and fake.sdr/
        let fake_kfx_dir = docs.join("fake.kfx");
        let fake_sdr_dir = docs.join("fake.sdr");
        std::fs::create_dir_all(&fake_kfx_dir).unwrap();
        std::fs::create_dir_all(&fake_sdr_dir).unwrap();
        std::fs::write(fake_sdr_dir.join("notes.txt"), b"KEEP").unwrap();

        let err5 = KindleSync::delete_book_file_safe(Some(&docs), &fake_kfx_dir).unwrap_err();
        assert_eq!(err5.kind(), ErrorKind::InvalidInput);
        assert!(fake_kfx_dir.is_dir());
        assert!(fake_sdr_dir.is_dir());
        assert!(fake_sdr_dir.join("notes.txt").is_file());

        // 6. Missing target with Some(allowed_root) fails closed and leaves companion Missing.sdr intact
        let missing_book = docs.join("Missing.kfx");
        let missing_sdr = docs.join("Missing.sdr");
        std::fs::create_dir_all(&missing_sdr).unwrap();
        let err6 = KindleSync::delete_book_file_safe(Some(&docs), &missing_book).unwrap_err();
        assert_eq!(err6.kind(), ErrorKind::NotFound);
        assert!(missing_sdr.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn test_scan_device_files_and_clean_stale_sdr_skip_symlinks() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join("documents");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        // Outside files that must NEVER be surfaced or deleted via symlinks
        let outside_kfx = outside.join("Outside.kfx");
        std::fs::write(&outside_kfx, b"OUTSIDE_KFX").unwrap();
        let outside_dir = outside.join("OutsideAuthor");
        std::fs::create_dir_all(&outside_dir).unwrap();
        std::fs::write(outside_dir.join("Hidden.mobi"), b"OUTSIDE_MOBI").unwrap();
        let outside_sdr = outside.join("OutsideSdr");
        std::fs::create_dir_all(&outside_sdr).unwrap();
        std::fs::write(outside_sdr.join("stale.han"), b"KEEP_OUTSIDE_HAN").unwrap();
        std::fs::write(outside_sdr.join("LanguageLayer.en.TEST.kll"), b"KLL").unwrap();

        // Legit book with symlinked .sdr directory and another legit book with symlinked .kll
        let legit1 = docs.join("LegitOne.kfx");
        std::fs::write(&legit1, b"LEGIT1").unwrap();
        symlink(&outside_sdr, docs.join("LegitOne.sdr")).unwrap();

        let legit2 = docs.join("LegitTwo.kfx");
        std::fs::write(&legit2, b"LEGIT2").unwrap();
        let legit2_sdr = docs.join("LegitTwo.sdr");
        std::fs::create_dir_all(&legit2_sdr).unwrap();
        symlink(
            outside_sdr.join("LanguageLayer.en.TEST.kll"),
            legit2_sdr.join("LanguageLayer.en.TEST.kll"),
        )
        .unwrap();

        // Symlinked .kfx and symlinked subdirectory inside docs
        symlink(&outside_kfx, docs.join("SymBook.kfx")).unwrap();
        symlink(&outside_dir, docs.join("SymAuthor")).unwrap();

        let scanned = KindleSync::scan_device_files(&docs);
        let names: Vec<&str> = scanned.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["LegitOne.kfx", "LegitTwo.kfx"]);

        let one = scanned.iter().find(|f| f.name == "LegitOne.kfx").unwrap();
        assert!(!one.has_sdr, "Symlinked .sdr directory must be ignored");
        assert!(!one.has_word_wise);

        let two = scanned.iter().find(|f| f.name == "LegitTwo.kfx").unwrap();
        assert!(two.has_sdr);
        assert!(
            !two.has_word_wise,
            "Symlinked .kll inside .sdr must be ignored"
        );

        // clean_stale_sdr_files_with_content_id must ignore symlinked .sdr directory
        KindleSync::clean_stale_sdr_files_with_content_id(
            &docs.join("LegitOne.sdr"),
            Some("NEW"),
            None,
            true,
        )
        .unwrap();
        assert!(outside_sdr.join("stale.han").exists());
        assert!(outside_sdr.join("LanguageLayer.en.TEST.kll").exists());
    }
}
