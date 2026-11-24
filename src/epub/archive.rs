//! EPUB zip archive handling, path traversal protection, and decompression bomb defenses.

use quick_xml::Reader as XmlReader;
use quick_xml::events::Event;
use std::io::Read;
use zip::ZipArchive;

use crate::epub::EpubError;
pub use crate::util::{collapse_zip_path, decode_percent, resolve_zip_path};

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

fn resolve_zip_entry_index<R: Read + std::io::Seek>(
    archive: &ZipArchive<R>,
    raw_path: &str,
) -> Option<usize> {
    if raw_path.is_empty() {
        return None;
    }

    // 1. Exact path match
    if let Some(idx) = archive.index_for_name(raw_path) {
        return Some(idx);
    }

    // 2. Backslash-to-slash & stripped leading '/'
    let norm = raw_path.replace('\\', "/");
    let trimmed = norm.trim_start_matches('/');
    if !trimmed.is_empty()
        && let Some(idx) = archive.index_for_name(trimmed)
    {
        return Some(idx);
    }

    // 3. Collapsed '.' / '..' segments
    let collapsed = collapse_zip_path(trimmed);
    if !collapsed.is_empty()
        && let Some(idx) = archive.index_for_name(&collapsed)
    {
        return Some(idx);
    }

    // 4. Percent-decoded variants
    let decoded_raw = decode_percent(raw_path);
    if !decoded_raw.is_empty()
        && let Some(idx) = archive.index_for_name(&decoded_raw)
    {
        return Some(idx);
    }
    let decoded_norm = decode_percent(trimmed).replace('\\', "/");
    let decoded_trimmed = decoded_norm.trim_start_matches('/');
    if !decoded_trimmed.is_empty()
        && let Some(idx) = archive.index_for_name(decoded_trimmed)
    {
        return Some(idx);
    }
    let decoded_collapsed = collapse_zip_path(decoded_trimmed);
    if !decoded_collapsed.is_empty()
        && let Some(idx) = archive.index_for_name(&decoded_collapsed)
    {
        return Some(idx);
    }

    // 5. Case-insensitive fallback scan over archive entry names
    let mut lc_candidates: Vec<String> = Vec::with_capacity(4);
    for cand in [trimmed, &collapsed, decoded_trimmed, &decoded_collapsed] {
        if !cand.is_empty() {
            let lc = cand.to_lowercase();
            if !lc_candidates.contains(&lc) {
                lc_candidates.push(lc);
            }
        }
    }
    if lc_candidates.is_empty() {
        return None;
    }

    let matched_name = archive
        .file_names()
        .find(|name| {
            let entry_norm = name.replace('\\', "/");
            let entry_trimmed = entry_norm.trim_start_matches('/');
            let entry_collapsed = collapse_zip_path(entry_trimmed);
            let e1 = entry_trimmed.to_lowercase();
            let e2 = entry_collapsed.to_lowercase();
            lc_candidates.iter().any(|c| c == &e1 || c == &e2)
        })
        .map(str::to_string)?;

    archive.index_for_name(&matched_name)
}

/// Opens a ZIP entry with tolerant path normalization: exact match -> backslash/leading-slash
/// normalization -> `.`/`..` collapse -> percent-decoding -> case-insensitive scan.
pub(crate) fn open_zip_entry<'a, R: Read + std::io::Seek>(
    archive: &'a mut ZipArchive<R>,
    raw_path: &str,
) -> zip::result::ZipResult<zip::read::ZipFile<'a>> {
    match resolve_zip_entry_index(archive, raw_path) {
        Some(idx) => archive.by_index(idx),
        None => Err(zip::result::ZipError::FileNotFound),
    }
}

/// Locates the package OPF file by parsing `META-INF/container.xml` under a shared decompression budget.
pub fn find_opf_path_with_budget<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    budget: &mut DecompressionBudget,
) -> Result<String, EpubError> {
    let mut entry = open_zip_entry(archive, "META-INF/container.xml")
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
                            let path = crate::epub::unescape_attr_value(&attr);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::{SimpleFileOptions, ZipWriter};

    #[test]
    fn test_resolve_zip_path_root_relative_and_parent_segments() {
        assert_eq!(
            resolve_zip_path("OEBPS", "/Images/cover.jpg"),
            "Images/cover.jpg"
        );
        assert_eq!(
            resolve_zip_path("OEBPS/Text", "\\OEBPS\\Styles\\main.css"),
            "OEBPS/Styles/main.css"
        );
        assert_eq!(resolve_zip_path("OEBPS", "../Images/a.jpg"), "Images/a.jpg");
        assert_eq!(
            resolve_zip_path("OEBPS/Text", "../Images/a.jpg"),
            "OEBPS/Images/a.jpg"
        );
    }

    #[test]
    fn test_open_zip_entry_tolerant_lookup() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();
            zip.start_file("OEBPS/Text/My Chapter.xhtml", opts).unwrap();
            zip.write_all(b"<html/>").unwrap();
            zip.start_file("OEBPS/Images/Cover.JPG", opts).unwrap();
            zip.write_all(b"img").unwrap();
            zip.finish().unwrap();
        }
        let mut archive = ZipArchive::new(Cursor::new(buf.into_inner())).unwrap();

        assert!(open_zip_entry(&mut archive, "OEBPS/Text/My Chapter.xhtml").is_ok());
        assert!(open_zip_entry(&mut archive, "/OEBPS/Text/My%20Chapter.xhtml").is_ok());
        assert!(open_zip_entry(&mut archive, "OEBPS\\Text\\./sub/../My%20Chapter.xhtml").is_ok());
        assert!(open_zip_entry(&mut archive, "oebps/images/cover.jpg").is_ok());
        assert!(open_zip_entry(&mut archive, "missing/file.xhtml").is_err());
    }
}
