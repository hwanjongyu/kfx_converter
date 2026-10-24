//! EPUB zip archive handling, path traversal protection, and decompression bomb defenses.

use std::io::Read;
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use zip::ZipArchive;

use crate::epub::EpubError;

pub const MAX_EPUB_FILES: usize = 10_000;
pub const MAX_EPUB_UNCOMPRESSED_TOTAL: u64 = 1024 * 1024 * 1024; // 1 GB
pub const MAX_EPUB_FILE_SIZE: u64 = 500 * 1024 * 1024; // 500 MB
pub const MAX_CHAPTER_XHTML_BYTES: u64 = 20 * 1024 * 1024; // 20 MB limit for single chapter XHTML
pub const MAX_METADATA_BYTES: u64 = 10 * 1024 * 1024; // 10 MB limit for XML/NCX/NAV metadata
pub const MAX_COMPRESSION_RATIO: f64 = 100.0;

/// Resolves a relative path against a base directory within a zip archive,
/// preventing path traversal vulnerabilities (`..`).
pub fn resolve_zip_path(base_dir: &str, relative_path: &str) -> String {
    let normalized_rel = relative_path.replace('\\', "/");
    let combined = if base_dir.is_empty() {
        normalized_rel
    } else {
        format!("{}/{}", base_dir.trim_matches('/'), normalized_rel)
    };
    let mut parts = Vec::new();
    for seg in combined.split('/') {
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

/// Decodes standard percent-encoded URI strings (e.g. `%20` -> space).
pub fn decode_percent(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16)
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

/// Locates the package OPF file by parsing `META-INF/container.xml`.
pub fn find_opf_path<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<String, EpubError> {
    let mut entry = archive
        .by_name("META-INF/container.xml")
        .map_err(|_| EpubError::Invalid("META-INF/container.xml not found".to_string()))?;

    let mut content = String::new();
    (&mut entry).take(MAX_METADATA_BYTES).read_to_string(&mut content)?;

    let mut xml_reader = XmlReader::from_str(&content);
    let mut buf = Vec::new();

    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"rootfile" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"full-path" {
                            let path = String::from_utf8_lossy(&attr.value).to_string();
                            return Ok(path);
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(EpubError::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Err(EpubError::Invalid("No rootfile element found in container.xml".to_string()))
}
