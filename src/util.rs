//! Level-0 pure utility helpers shared across EPUB parsing, IR resolution, and KFX packaging.

/// Decodes standard percent-encoded URI strings (e.g. `%20` -> space).
pub fn decode_percent(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
            && let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or_default(),
                16,
            )
        {
            result.push(byte);
            i += 3;
            continue;
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).to_string()
}

/// Collapses empty, `.` and `..` path segments inside a slash-delimited archive path.
pub fn collapse_zip_path(path: &str) -> String {
    let mut parts = Vec::new();
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        } else if seg == ".." {
            parts.pop();
        } else {
            parts.push(seg);
        }
    }
    parts.join("/")
}

/// Resolves a relative or root-relative path against a base directory within a zip archive,
/// preventing path traversal vulnerabilities (`..`).
pub fn resolve_zip_path(base_dir: &str, relative_path: &str) -> String {
    let normalized_rel = relative_path.replace('\\', "/");
    let combined = if base_dir.is_empty() || normalized_rel.starts_with('/') {
        normalized_rel
    } else {
        format!("{}/{}", base_dir.trim_matches('/'), normalized_rel)
    };
    collapse_zip_path(&combined)
}

/// Checks if a character is an opening quotation mark in Western or CJK typography.
#[inline]
pub fn is_opening_quote(c: char) -> bool {
    matches!(c, '“' | '‘' | '"' | '\'' | '«' | '‹' | '「' | '『')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_percent_and_zip_paths() {
        assert_eq!(
            decode_percent("My%20Chapter%231.xhtml"),
            "My Chapter#1.xhtml"
        );
        assert_eq!(decode_percent("100%ZZ"), "100%ZZ");
        assert_eq!(
            collapse_zip_path("OEBPS/Text/../Images/./a.jpg"),
            "OEBPS/Images/a.jpg"
        );
        assert_eq!(
            resolve_zip_path("OEBPS/Text", "../Part2/ch1.xhtml"),
            "OEBPS/Part2/ch1.xhtml"
        );
        assert_eq!(
            resolve_zip_path("OEBPS", "/Images/cover.jpg"),
            "Images/cover.jpg"
        );
    }

    #[test]
    fn test_is_opening_quote() {
        assert!(is_opening_quote('“'));
        assert!(is_opening_quote('「'));
        assert!(is_opening_quote('"'));
        assert!(!is_opening_quote('A'));
    }
}
