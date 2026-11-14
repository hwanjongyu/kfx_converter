//! Integration tests for Kindle synchronization and path sanitization.
//! Covers `src/sync/sanitize.rs`, `src/sync/transfer.rs`, and `src/sync/catalog.rs`.

mod common;

use kfx_converter::{KindleSync, SyncError};
use std::fs;

// =========================================================================
// 1. Filename Sanitization & Defense Tests
// =========================================================================

#[test]
fn test_sanitize_filename_all_dos_devices_with_various_extensions() {
    let devices = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    for dev in devices {
        // Uppercase bare
        let res = KindleSync::sanitize_filename(dev);
        assert_eq!(res, format!("{}_book", dev));

        // Lowercase bare
        let lower = dev.to_lowercase();
        let res_lower = KindleSync::sanitize_filename(&lower);
        assert_eq!(res_lower, format!("{}_book", lower));

        // With extension .kfx
        let with_ext = format!("{}.kfx", dev);
        let res_ext = KindleSync::sanitize_filename(&with_ext);
        assert_eq!(res_ext, format!("{}_book", with_ext));

        // Lowercase with extension
        let lower_ext = format!("{}.epub", lower);
        let res_lower_ext = KindleSync::sanitize_filename(&lower_ext);
        assert_eq!(res_lower_ext, format!("{}_book", lower_ext));
    }
}

#[test]
fn test_sanitize_filename_path_traversal_and_forbidden_chars() {
    // Basic traversal
    assert_eq!(
        KindleSync::sanitize_filename("../../etc/shadow"),
        "____etc_shadow"
    );
    assert_eq!(KindleSync::sanitize_filename("a/b/c/../d"), "a_b_c___d");
    assert_eq!(KindleSync::sanitize_filename(".../....//"), "book");

    // Forbidden filesystem chars
    let raw = "Title: With *Stars* & <Tags> | \"Quotes\" & ?Marks & \\Backslashes/";
    let clean = KindleSync::sanitize_filename(raw);
    for forbidden in ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0'] {
        assert!(
            !clean.contains(forbidden),
            "Clean string should not contain '{}': {}",
            forbidden,
            clean
        );
    }

    // Control characters (BEL, LF, CR, TAB, NUL)
    let with_controls = "Title\x00With\x07Control\nChars\rAnd\tTabs";
    let clean_controls = KindleSync::sanitize_filename(with_controls);
    assert!(!clean_controls.chars().any(|c| c.is_control()));
}

#[test]
fn test_sanitize_filename_multibyte_utf8_preservation_and_truncation() {
    // Preserves East Asian CJK characters
    let korean = "해리 포터와 마법사의 돌";
    assert_eq!(KindleSync::sanitize_filename(korean), korean);

    let japanese = "吾輩は猫である。名前はまだ無い。";
    assert_eq!(KindleSync::sanitize_filename(japanese), japanese);

    // Accented European characters and emoji
    let french = "Les Misérables — Épisode 1";
    assert_eq!(
        KindleSync::sanitize_filename(french),
        "Les Misérables — Épisode 1"
    );

    // Massive multibyte string exceeding 128 bytes limit
    // Each Korean character is 3 bytes in UTF-8
    let long_korean = "한글제목입니다".repeat(20); // 140 chars * 3 = 420 bytes
    let truncated = KindleSync::sanitize_filename(&long_korean);
    assert!(
        truncated.len() <= 128,
        "Sanitized filename must be <= 128 bytes: got {}",
        truncated.len()
    );
    // Must be valid UTF-8 without partial multi-byte character slicing
    assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());
}

#[test]
fn test_sanitize_filename_empty_and_whitespace_fallback() {
    assert_eq!(KindleSync::sanitize_filename(""), "book");
    assert_eq!(KindleSync::sanitize_filename("   "), "book");
    assert_eq!(KindleSync::sanitize_filename("....."), "book");
    assert_eq!(KindleSync::sanitize_filename("___"), "book");
}

// =========================================================================
// 2. ASIN Sanitization Tests
// =========================================================================

#[test]
fn test_sanitize_asin_standard_and_traversal() {
    assert_eq!(KindleSync::sanitize_asin("B000JML542"), "B000JML542");
    assert_eq!(
        KindleSync::sanitize_asin("../../root/B000JML542"),
        "rootB000JML542"
    );
    assert_eq!(KindleSync::sanitize_asin(""), "UNKNOWN_ASIN");
    assert_eq!(KindleSync::sanitize_asin("   "), "UNKNOWN_ASIN");

    // Truncates at 64 characters without UTF-8 slicing error
    let long_asin = "B0".to_string() + &"A".repeat(100);
    let sanitized_asin = KindleSync::sanitize_asin(&long_asin);
    assert_eq!(sanitized_asin.chars().count(), 64);
}

// =========================================================================
// 3. Kindle Device Sync & Read-Only Mount Detection Tests
// =========================================================================

#[test]
fn test_sync_to_device_missing_documents_dir_returns_error() {
    let temp = common::temp_dir();
    let kfx_file = temp.path().join("book.kfx");
    std::fs::write(&kfx_file, b"DUMMY_KFX").unwrap();

    // Device root without `documents` directory
    let res = KindleSync::sync_to_device(
        temp.path(),
        &kfx_file,
        "Test Book",
        "B00TESTASIN",
        None,
        None,
        None,
        None,
        None,
    );

    match res {
        Err(SyncError::DeviceNotFound(_)) => {}
        other => panic!("Expected DeviceNotFound, got {:?}", other),
    }
}

#[test]
fn test_sync_to_device_read_only_mount_detection() {
    let temp = common::temp_dir();
    let docs_dir = temp.path().join("documents");
    fs::create_dir_all(&docs_dir).unwrap();

    let kfx_file = temp.path().join("test_book.kfx");
    std::fs::write(&kfx_file, b"DUMMY_KFX").unwrap();

    // Mark documents directory permissions as read-only
    let mut perms = fs::metadata(&docs_dir).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&docs_dir, perms).unwrap();

    let res = KindleSync::sync_to_device(
        temp.path(),
        &kfx_file,
        "Test Book",
        "B00TESTASIN",
        None,
        None,
        None,
        None,
        None,
    );

    // Restore writable permissions before assertions so temp_dir cleanup succeeds
    let mut restore_perms = fs::metadata(&docs_dir).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    restore_perms.set_readonly(false);
    let _ = fs::set_permissions(&docs_dir, restore_perms);

    match res {
        Err(SyncError::SecurityViolation(msg)) => {
            assert!(
                msg.contains("read-only"),
                "SecurityViolation message should mention read-only: {}",
                msg
            );
        }
        other => panic!(
            "Expected SecurityViolation for read-only mount, got {:?}",
            other
        ),
    }
}

#[test]
fn test_sync_clean_stale_sdr_files() {
    let temp = common::temp_dir();
    let sdr_dir = temp.path().join("Book.sdr");
    fs::create_dir_all(&sdr_dir).unwrap();

    // Create stale reader cache files
    fs::write(sdr_dir.join("journal.yjf"), b"stale journal").unwrap();
    fs::write(sdr_dir.join("reader.yjr"), b"stale reader state").unwrap();
    fs::write(sdr_dir.join("corrupt.bad_file"), b"stale error marker").unwrap();
    // Create valid sidecar that should NOT be deleted
    let valid_kll = sdr_dir.join("LanguageLayer.en.B00TEST.kll");
    fs::write(&valid_kll, b"SQLite format 3\0").unwrap();

    KindleSync::clean_stale_sdr_files(&sdr_dir, Some("B00TEST"), false);

    assert!(!sdr_dir.join("journal.yjf").exists());
    assert!(!sdr_dir.join("reader.yjr").exists());
    assert!(!sdr_dir.join("corrupt.bad_file").exists());
    assert!(valid_kll.exists(), "Valid .kll sidecar must be preserved");
}

#[test]
fn test_sync_scan_device_files_and_safe_delete() {
    let temp = common::temp_dir();
    let docs_dir = temp.path().join("documents");
    fs::create_dir_all(&docs_dir).unwrap();

    // Create books in root and author subfolder
    let root_book = docs_dir.join("DirectBook.kfx");
    fs::write(&root_book, b"KFX_DATA").unwrap();

    let author_dir = docs_dir.join("Famous Author");
    fs::create_dir_all(&author_dir).unwrap();
    let author_book = author_dir.join("AuthorBook.kfx");
    fs::write(&author_book, b"KFX_DATA_2").unwrap();

    // Companion SDR for author book
    let author_sdr = author_dir.join("AuthorBook.sdr");
    fs::create_dir_all(&author_sdr).unwrap();
    fs::write(author_sdr.join("state.yjf"), b"state").unwrap();

    let scanned = KindleSync::scan_device_files(&docs_dir);
    assert_eq!(scanned.len(), 2, "Should discover both books");

    // Safe delete author book
    KindleSync::delete_book_file_safe(Some(&docs_dir), &author_book)
        .expect("Safe delete should succeed");

    assert!(!author_book.exists(), "Book file should be deleted");
    assert!(
        !author_sdr.exists(),
        "Companion SDR directory should also be deleted"
    );

    // Attempt safe delete outside allowed root
    let outside_file = temp.path().join("secret.txt");
    fs::write(&outside_file, b"secret").unwrap();
    let delete_err = KindleSync::delete_book_file_safe(Some(&docs_dir), &outside_file);
    assert!(
        delete_err.is_err(),
        "Must reject deletion outside allowed root"
    );
}
