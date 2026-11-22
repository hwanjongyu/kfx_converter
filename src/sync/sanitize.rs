//! Naming & Security Sanitization for Kindle Devices
//!
//! Provides FAT32/VFAT filename, ASIN, and companion sidecar stem sanitization
//! preventing directory traversal attacks (CWE-22 / CWE-73) and illegal characters.

use std::collections::HashSet;

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
