//! EPUB zip archive handling, path traversal protection, and decompression bomb defenses.

use quick_xml::Reader as XmlReader;
use quick_xml::events::Event;
use std::io::Read;
use zip::ZipArchive;

use crate::epub::EpubError;

pub const MAX_EPUB_FILES: usize = 10_000;
pub const MAX_EPUB_UNCOMPRESSED_TOTAL: u64 = 1024 * 1024 * 1024; // 1 GB
pub const MAX_EPUB_FILE_SIZE: u64 = 500 * 1024 * 1024; // 500 MB
pub const MAX_CHAPTER_XHTML_BYTES: u64 = 20 * 1024 * 1024; // 20 MB limit for single chapter XHTML
pub const MAX_METADATA_BYTES: u64 = 10 * 1024 * 1024; // 10 MB limit for XML/NCX/NAV metadata
pub const MAX_COMPRESSION_RATIO: f64 = 100.0;

/// Tracks cumulative decompressed bytes read from an EPUB archive against a hard limit.
#[derive(Debug, Clone, Copy)]
pub struct DecompressionBudget {
    pub total_decompressed: u64,
    pub max_total: u64,
}

impl DecompressionBudget {
    pub fn new(max_total: u64) -> Self {
        Self {
            total_decompressed: 0,
            max_total,
        }
    }

    pub fn charge(&mut self, bytes: u64) -> Result<(), EpubError> {
        let next = self.total_decompressed.checked_add(bytes).ok_or_else(|| {
            EpubError::Security(
                "Total cumulative decompressed payload overflows budget".to_string(),
            )
        })?;
        if next > self.max_total {
            return Err(EpubError::Security(format!(
                "Total cumulative decompressed payload ({} bytes) exceeds {} byte limit",
                next, self.max_total
            )));
        }
        self.total_decompressed = next;
        Ok(())
    }
}

/// Streams a ZIP entry in bounded 64 KiB chunks while enforcing per-entry limits,
/// cumulative decompression budgets, and live compression ratio checks.
pub fn read_entry_bytes(
    entry: &mut zip::read::ZipFile<'_>,
    max_entry_bytes: u64,
    budget: &mut DecompressionBudget,
) -> Result<Vec<u8>, EpubError> {
    let entry_name = entry.name().to_string();
    let compressed = entry.compressed_size();
    let mut out = Vec::with_capacity(entry.size().min(max_entry_bytes).min(64 * 1024) as usize);
    let mut entry_read: u64 = 0;
    let mut buf = [0u8; 64 * 1024];

    loop {
        match entry.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                entry_read = entry_read.checked_add(n as u64).ok_or_else(|| {
                    EpubError::Security(format!(
                        "Member '{}' decompressed size overflows u64",
                        entry_name
                    ))
                })?;
                if entry_read > max_entry_bytes {
                    return Err(EpubError::Security(format!(
                        "Member '{}' exceeds {} byte entry limit",
                        entry_name, max_entry_bytes
                    )));
                }
                budget.charge(n as u64)?;
                if compressed > 0 && entry_read > 1024 * 1024 {
                    let ratio = (entry_read as f64) / (compressed as f64);
                    if ratio > MAX_COMPRESSION_RATIO {
                        return Err(EpubError::Security(format!(
                            "Member '{}' actual compression ratio {:.1}:1 exceeds limit",
                            entry_name, ratio
                        )));
                    }
                }
                out.extend_from_slice(&buf[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(EpubError::Io(e)),
        }
    }

    Ok(out)
}

/// Streams a UTF-8 text entry from an EPUB archive under entry and cumulative budget limits.
pub fn read_entry_string(
    entry: &mut zip::read::ZipFile<'_>,
    max_entry_bytes: u64,
    budget: &mut DecompressionBudget,
) -> Result<String, EpubError> {
    let bytes = read_entry_bytes(entry, max_entry_bytes, budget)?;
    String::from_utf8(bytes)
        .map_err(|e| EpubError::Invalid(format!("Invalid UTF-8 in EPUB entry: {}", e)))
}

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

/// Locates the package OPF file by parsing `META-INF/container.xml` under a shared decompression budget.
pub fn find_opf_path_with_budget<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    budget: &mut DecompressionBudget,
) -> Result<String, EpubError> {
    let mut entry = archive
        .by_name("META-INF/container.xml")
        .map_err(|_| EpubError::Invalid("META-INF/container.xml not found".to_string()))?;

    let content = read_entry_string(&mut entry, MAX_METADATA_BYTES, budget)?;

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

    Err(EpubError::Invalid(
        "No rootfile element found in container.xml".to_string(),
    ))
}

/// Locates the package OPF file by parsing `META-INF/container.xml`.
pub fn find_opf_path<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<String, EpubError> {
    let mut budget = DecompressionBudget::new(MAX_EPUB_UNCOMPRESSED_TOTAL);
    find_opf_path_with_budget(archive, &mut budget)
}
