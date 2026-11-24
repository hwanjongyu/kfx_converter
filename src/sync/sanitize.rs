//! Naming & Security Sanitization for Kindle Devices
//!
//! Provides FAT32/VFAT filename, ASIN, and companion sidecar stem sanitization
//! preventing directory traversal attacks (CWE-22 / CWE-73) and illegal characters.

pub use crate::storage::{
    collect_sidecar_stems, collect_thumbnail_stems, normalize_asin_10, sanitize_asin,
    sanitize_cde_content_type, sanitize_filename, sanitize_sidecar_stem,
};

#[cfg(test)]
mod tests {
    use crate::sync::KindleSync;

    #[test]
    fn test_sanitize_filename_basic() {
        assert_eq!(
            KindleSync::sanitize_filename("Simple Title"),
            "Simple Title"
        );
        assert_eq!(
            KindleSync::sanitize_filename("What If?: Serious Scientific Answers"),
            "What If__ Serious Scientific Answers"
        );
        assert_eq!(
            KindleSync::sanitize_filename("../../etc/passwd"),
            "____etc_passwd"
        );
        assert_eq!(KindleSync::sanitize_filename("   "), "book");
        assert_eq!(
            KindleSync::sanitize_filename("Trailing Dots..."),
            "Trailing Dots"
        );
    }

    #[test]
    fn test_sanitize_filename_dos_devices() {
        assert_eq!(KindleSync::sanitize_filename("CON"), "book_CON");
        assert_eq!(KindleSync::sanitize_filename("aux.kfx"), "book_aux.kfx");
        assert_eq!(KindleSync::sanitize_filename("nul"), "book_nul");
        assert_eq!(KindleSync::sanitize_filename("COM1.epub"), "book_COM1.epub");
        assert_eq!(KindleSync::sanitize_filename("PRN"), "book_PRN");

        // Post-truncation re-checks: trailing whitespace/dots shrinking into DOS device or empty
        let con_padded_spaces = format!("CON{}x", " ".repeat(130));
        assert_eq!(
            KindleSync::sanitize_filename(&con_padded_spaces),
            "book_CON"
        );

        let con_trailing_dots = format!("CON{}", ".".repeat(130));
        assert_eq!(
            KindleSync::sanitize_filename(&con_trailing_dots),
            "book_CON"
        );

        let only_underscores_then_tail = format!("{}x", "_".repeat(130));
        assert_eq!(
            KindleSync::sanitize_filename(&only_underscores_then_tail),
            "book"
        );

        let long_con_ext = format!("CON.{}", "a".repeat(140));
        let sanitized_long_con = KindleSync::sanitize_filename(&long_con_ext);
        assert!(sanitized_long_con.starts_with("book_CON."));
        assert!(sanitized_long_con.len() <= 128);

        // Long string with multi-byte characters capped at <= 128 bytes
        let long_korean = "한".repeat(100); // 300 bytes in UTF-8
        let sanitized = KindleSync::sanitize_filename(&long_korean);
        assert!(sanitized.len() <= 128);
        assert!(std::str::from_utf8(sanitized.as_bytes()).is_ok());
    }

    #[test]
    fn test_sidecar_stem_and_cde_helpers() {
        use super::{
            collect_sidecar_stems, normalize_asin_10, sanitize_cde_content_type,
            sanitize_sidecar_stem,
        };

        assert_eq!(sanitize_sidecar_stem("   "), None);
        assert_eq!(sanitize_sidecar_stem("../../.."), None);
        assert_eq!(
            sanitize_sidecar_stem("  ../CID_123 \0 "),
            Some("CID_123".to_string())
        );

        assert_eq!(sanitize_cde_content_type("ebok"), "EBOK");
        assert_eq!(sanitize_cde_content_type(" pdoc "), "PDOC");
        assert_eq!(sanitize_cde_content_type("../../evil"), "EBOK");

        assert_eq!(
            normalize_asin_10("123456789ABCDEF0123456789ABCDEF0"),
            Some("B123456789".to_string())
        );
        assert_eq!(normalize_asin_10("B000JML542"), None);

        // Deduplicates aliasing asin_10 and content_id case-insensitively
        let stems =
            collect_sidecar_stems(Some("123456789ABCDEF0123456789ABCDEF0"), Some("b123456789"));
        assert_eq!(
            stems,
            vec![
                "123456789ABCDEF0123456789ABCDEF0".to_string(),
                "B123456789".to_string(),
            ]
        );
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
}
