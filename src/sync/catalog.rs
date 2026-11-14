//! On-Device Catalog & Maintenance for Amazon Kindle
//!
//! Handles scanning documents directories for e-books, verifying companion `.sdr` sidecars,
//! enforcing whitelist-safe deletion, and purging stale or corrupt files.

use std::path::{Path, PathBuf};
use super::types::KindleBookFile;
use super::DEVICE_SYNC_LOCK;

/// Scans the Kindle documents directory (including author subfolders up to max_depth 3) for valid e-book files.
pub fn scan_device_files(doc_path: &Path) -> Vec<KindleBookFile> {
    let mut files = Vec::new();
    if !doc_path.is_dir() {
        return files;
    }

    fn scan_dir_recursive(doc_path: &Path, dir: &Path, depth: usize, max_depth: usize, files: &mut Vec<KindleBookFile>) {
        if depth > max_depth {
            return;
        }
        let valid_exts = ["kfx", "azw3", "mobi", "pdf", "txt"];
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                    if !name.starts_with('.') && !name.ends_with(".sdr") {
                        scan_dir_recursive(doc_path, &path, depth + 1, max_depth, files);
                    }
                } else if path.is_file() {
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_lowercase())
                        .unwrap_or_default();

                    if valid_exts.contains(&ext.as_str()) {
                        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        let format = ext.to_uppercase();
                        let meta_res = std::fs::metadata(&path);
                        let (size_bytes, size_formatted) = match &meta_res {
                            Ok(meta) => {
                                let bytes = meta.len();
                                let s = if bytes >= 1024 * 1024 {
                                    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
                                } else {
                                    format!("{} KB", bytes / 1024)
                                };
                                (bytes, s)
                            }
                            Err(_) => (0, "0 KB".into()),
                        };

                        let (modified_timestamp, modified) = match meta_res.as_ref().ok().and_then(|m| m.modified().ok()) {
                            Some(time) => {
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
                            None => (0, "Unknown".into()),
                        };

                        let (title, author) = if let Ok(rel) = path.strip_prefix(doc_path) {
                            let components: Vec<_> = rel.iter().collect();
                            if components.len() > 1 {
                                let first_dir = components[0].to_string_lossy();
                                if first_dir.eq_ignore_ascii_case("downloads") || first_dir.eq_ignore_ascii_case("system") {
                                    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                                    if let Some((t, a)) = stem.split_once(" - ") {
                                        (t.trim().to_string(), a.trim().to_string())
                                    } else {
                                        (stem.to_string(), "Unknown".to_string())
                                    }
                                } else {
                                    let author_str = first_dir.to_string();
                                    let title_str = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
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
                            (path.file_stem().unwrap_or_default().to_string_lossy().to_string(), "Unknown".to_string())
                        };

                        let file_stem = path.file_stem().unwrap_or_default().to_string_lossy();
                        let parent = path.parent().unwrap_or(dir);
                        let sdr_path = parent.join(format!("{}.sdr", file_stem));
                        let has_sdr = sdr_path.is_dir();
                        let mut has_word_wise = false;
                        if has_sdr
                            && let Ok(sdr_entries) = std::fs::read_dir(&sdr_path) {
                                for sdr_entry in sdr_entries.flatten() {
                                    let sdr_name = sdr_entry.file_name().to_string_lossy().to_lowercase();
                                    if sdr_name.ends_with(".kll") {
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
                            size_bytes,
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
pub fn delete_book_file_safe(allowed_root: Option<&Path>, full_path: &Path) -> Result<(), std::io::Error> {
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

    if let Some(root) = allowed_root {
        let canon_root = root.canonicalize()?;
        let canon_target = full_path.canonicalize()?;
        if !canon_target.starts_with(&canon_root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("Target path {:?} is outside allowed directory {:?}", full_path, root),
            ));
        }
    }

    if full_path.is_file() {
        std::fs::remove_file(full_path)?;
    }
    let part_path = PathBuf::from(format!("{}.part", full_path.display()));
    if part_path.is_file() {
        let _ = std::fs::remove_file(&part_path);
    }
    let file_stem = full_path.file_stem().unwrap_or_default().to_string_lossy();
    if let Some(parent) = full_path.parent() {
        let sdr_path = parent.join(format!("{}.sdr", file_stem));
        if sdr_path.is_dir()
            && !sdr_path.is_symlink()
            && sdr_path.extension().and_then(|e| e.to_str()) == Some("sdr")
        {
            if let Some(root) = allowed_root
                && let (Ok(canon_root), Ok(canon_sdr)) = (root.canonicalize(), sdr_path.canonicalize())
                && !canon_sdr.starts_with(&canon_root)
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("SDR path {:?} is outside allowed directory {:?}", sdr_path, root),
                ));
            }
            let _ = std::fs::remove_dir_all(&sdr_path);
        }
    }
    Ok(())
}

/// Deletes an e-book file and its companion `.sdr` directory from the device.
pub fn delete_book_file(full_path: &Path) -> Result<(), std::io::Error> {
    delete_book_file_safe(None, full_path)
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
    let stale_exts = [".han", ".tan", ".pdr", ".mbp1", ".phl", ".bdr", ".ea", ".yjf", ".yjr", ".part"];
    if let Ok(entries) = std::fs::read_dir(sdr_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let fname_lower = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();

                if fname_lower.contains(".bad_file")
                    || stale_exts.iter().any(|ext| fname_lower.ends_with(ext))
                    || (purge_word_wise && fname_lower.contains(".kll"))
                {
                    let _ = std::fs::remove_file(&path);
                } else if let Some(asin) = current_asin {
                    let asin_lower = asin.to_lowercase();
                    let prefix: String = asin_lower.chars().take(9).collect();
                    let asin_10 = if prefix.len() == 9 {
                        format!("b{}", prefix)
                    } else {
                        asin_lower.clone()
                    };
                    if (fname_lower.contains(".kll") || fname_lower.contains(".asc"))
                        && !fname_lower.contains(&asin_lower)
                        && !fname_lower.contains(&asin_10)
                    {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            } else if path.is_dir() && path.file_name().is_some_and(|n| n == "data") {
                let pag_cache = path.join(".pagination.cache");
                if pag_cache.is_dir() {
                    let _ = std::fs::remove_dir_all(&pag_cache);
                }
            }
        }
    }
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

        let kfx_item = scanned.iter().find(|b| b.name == "Novel.kfx").expect("Novel.kfx found");
        assert_eq!(kfx_item.format, "KFX");
        assert!(kfx_item.has_word_wise);
        assert!(kfx_item.has_sdr);

        let mobi_item = scanned.iter().find(|b| b.name == "Story.mobi").expect("Story.mobi found");
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
        assert!(res_missing.is_err(), "Missing file with allowed_root must fail closed");
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
}
