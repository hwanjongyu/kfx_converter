//! Naming & Security Sanitization for Kindle Devices
//!
//! Provides FAT32/VFAT filename and ASIN sanitization logic preventing
//! directory traversal attacks (CWE-22 / CWE-73) and illegal filesystem characters.

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

#[cfg(test)]
mod tests {
    use crate::sync::KindleSync;

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
}
