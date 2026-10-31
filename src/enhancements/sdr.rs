//! Kindle Companion SDR (.sdr) Sidecar & Thumbnail Deployment
//!
//! Provides a single unified implementation for generating Kindle companion `.sdr` directory
//! structures, including Word Wise SQLite database sidecars (`.kll`), 10-character canonical ASIN
//! aliases, and multi-identifier lockscreen thumbnails across both local conversions and USB sync.

use std::path::Path;

pub struct SdrDeployer;

impl SdrDeployer {
    /// Prepares a destination file path for overwriting, stripping read-only permissions if present.
    fn prepare_for_overwrite(path: &Path) {
        if let Ok(metadata) = path.metadata()
            && metadata.permissions().readonly()
        {
            let mut perms = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(path, perms);
        }
    }

    /// Safely writes bytes to a file inside the `.sdr` directory, handling pre-existing read-only permissions
    /// and performing atomic file persistence via a staged temporary file to prevent corruption on USB disconnect.
    fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        Self::prepare_for_overwrite(path);
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut tmp_file = tempfile::Builder::new()
            .prefix(".sdr_tmp.")
            .tempfile_in(parent)?;
        std::io::Write::write_all(&mut tmp_file, bytes)?;
        std::io::Write::flush(&mut tmp_file)?;
        tmp_file.persist(path).map_err(|e| e.error)?;
        Ok(())
    }

    /// Deploys all companion sidecar files into the target `.sdr` directory.
    pub fn deploy(
        sdr_dir: &Path,
        asin: &str,
        content_id: Option<&str>,
        cde_content_type: Option<&str>,
        book_language: Option<&str>,
        cover_jpeg: Option<&[u8]>,
        kll_bytes: Option<&[u8]>,
    ) -> std::io::Result<()> {
        std::fs::create_dir_all(sdr_dir)?;

        let safe_asin = crate::sync::KindleSync::sanitize_asin(asin);
        let safe_cid = content_id.map(crate::sync::KindleSync::sanitize_asin);

        // 1. Deploy Word Wise sidecar (.kll) and multi-language / ASIN aliases
        if let Some(kll) = kll_bytes {
            let mut lang_tags: Vec<String> = vec!["en".to_string(), "EN".to_string()];
            if let Some(raw_lang) = book_language {
                let clean = raw_lang.trim();
                if !clean.is_empty() {
                    let upper = clean.to_ascii_uppercase();
                    let lower = clean.to_ascii_lowercase();
                    let parts: Vec<&str> = clean.split(['-', '_']).collect();
                    let bcp47 = if parts.len() == 2 {
                        format!("{}-{}", parts[0].to_ascii_lowercase(), parts[1].to_ascii_uppercase())
                    } else {
                        clean.to_string()
                    };
                    let primary_upper = parts[0].to_ascii_uppercase();
                    let primary_lower = parts[0].to_ascii_lowercase();

                    for tag in [clean.to_string(), upper, lower, bcp47, primary_upper, primary_lower] {
                        if !lang_tags.contains(&tag) {
                            lang_tags.push(tag);
                        }
                    }
                }
            }

            let mut ids = vec![safe_asin.clone()];
            let prefix: String = safe_asin.chars().take(9).collect();
            if prefix.len() == 9 {
                let asin_10 = format!("B{}", prefix);
                if asin_10 != safe_asin {
                    ids.push(asin_10);
                }
            }
            if let Some(ref cid) = safe_cid
                && cid != &safe_asin
                && !cid.is_empty()
            {
                ids.push(cid.clone());
            }

            for id in &ids {
                for tag in &lang_tags {
                    let kll_path = sdr_dir.join(format!("LanguageLayer.{}.{}.kll", tag, id));
                    let _ = Self::write_file(&kll_path, kll);
                }
            }
        }

        // 2. Deploy Cover Thumbnails
        if let Some(jpeg) = cover_jpeg {
            let _ = Self::write_file(&sdr_dir.join("cover.jpg"), jpeg);

            let cde = cde_content_type.unwrap_or("EBOK");
            let alt_cde = if cde == "EBOK" { "PDOC" } else { "EBOK" };

            let mut ids = vec![safe_asin.as_str()];
            if let Some(ref cid) = safe_cid
                && cid != &safe_asin
                && !cid.is_empty()
            {
                ids.push(cid.as_str());
            }

            for id in ids {
                let _ = Self::write_file(
                    &sdr_dir.join(format!("thumbnail_{}_{}_portrait.jpg", id, cde)),
                    jpeg,
                );
                let _ = Self::write_file(
                    &sdr_dir.join(format!("thumbnail_{}_{}_portrait.jpg", id, alt_cde)),
                    jpeg,
                );
                let _ = Self::write_file(
                    &sdr_dir.join(format!("thumbnail_{}_portrait.jpg", id)),
                    jpeg,
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sdr_deployer_unified() {
        let temp = tempfile::tempdir().unwrap();
        let sdr_path = temp.path().join("SampleBook.sdr");

        let asin = "123456789ABCDEF0123456789ABCDEF0";
        let content_id = "CID_DIFFERENT_987654321";
        let cover_data = b"MOCK_JPEG_DATA";
        let kll_data = b"MOCK_SQLITE_KLL";

        SdrDeployer::deploy(
            &sdr_path,
            asin,
            Some(content_id),
            Some("EBOK"),
            Some("EN-CA"),
            Some(cover_data),
            Some(kll_data),
        )
        .unwrap();

        // 1. Verify primary, EN-CA, and alias .kll files
        assert!(sdr_path.join(format!("LanguageLayer.en.{}.kll", asin)).is_file());
        assert!(sdr_path.join(format!("LanguageLayer.EN-CA.{}.kll", asin)).is_file());
        assert!(sdr_path.join(format!("LanguageLayer.EN-CA.B{}.kll", &asin[..9])).is_file());
        assert!(sdr_path.join(format!("LanguageLayer.en.B{}.kll", &asin[..9])).is_file());
        assert!(sdr_path.join(format!("LanguageLayer.en.{}.kll", content_id)).is_file());
        assert!(sdr_path.join(format!("LanguageLayer.EN-CA.{}.kll", content_id)).is_file());

        // 2. Verify cover.jpg and thumbnails
        assert!(sdr_path.join("cover.jpg").is_file());
        assert!(sdr_path.join(format!("thumbnail_{}_EBOK_portrait.jpg", asin)).is_file());
        assert!(sdr_path.join(format!("thumbnail_{}_PDOC_portrait.jpg", asin)).is_file());
        assert!(sdr_path.join(format!("thumbnail_{}_portrait.jpg", asin)).is_file());
        assert!(sdr_path.join(format!("thumbnail_{}_EBOK_portrait.jpg", content_id)).is_file());
    }
}
