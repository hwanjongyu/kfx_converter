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

    /// Safely writes bytes to a file inside the `.sdr` directory, handling pre-existing read-only permissions.
    fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        Self::prepare_for_overwrite(path);
        std::fs::write(path, bytes)
    }

    /// Deploys all companion sidecar files into the target `.sdr` directory.
    pub fn deploy(
        sdr_dir: &Path,
        asin: &str,
        content_id: Option<&str>,
        cde_content_type: Option<&str>,
        cover_jpeg: Option<&[u8]>,
        kll_bytes: Option<&[u8]>,
    ) -> std::io::Result<()> {
        std::fs::create_dir_all(sdr_dir)?;

        let safe_asin = crate::sync::KindleSync::sanitize_asin(asin);
        let safe_cid = content_id.map(crate::sync::KindleSync::sanitize_asin);

        // 1. Deploy Word Wise sidecar (.kll) and ASIN aliases
        if let Some(kll) = kll_bytes {
            let primary_kll = sdr_dir.join(format!("LanguageLayer.en.{}.kll", safe_asin));
            Self::write_file(&primary_kll, kll)?;

            // 10-char canonical ASIN alias (B...) if safe_asin is 32-hex
            if safe_asin.len() >= 9 {
                let asin_10 = format!("B{}", &safe_asin[..9]);
                if asin_10 != safe_asin {
                    let alias_kll = sdr_dir.join(format!("LanguageLayer.en.{}.kll", asin_10));
                    let _ = Self::write_file(&alias_kll, kll);
                }
            }

            // Fallback content ID sidecar for dual-lookup compatibility
            if let Some(ref cid) = safe_cid
                && cid != &safe_asin
                && !cid.is_empty()
            {
                let cid_kll = sdr_dir.join(format!("LanguageLayer.en.{}.kll", cid));
                let _ = Self::write_file(&cid_kll, kll);
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
            Some(cover_data),
            Some(kll_data),
        )
        .unwrap();

        // 1. Verify primary and alias .kll files
        assert!(sdr_path.join(format!("LanguageLayer.en.{}.kll", asin)).is_file());
        assert!(sdr_path.join(format!("LanguageLayer.en.B{}.kll", &asin[..9])).is_file());
        assert!(sdr_path.join(format!("LanguageLayer.en.{}.kll", content_id)).is_file());

        // 2. Verify cover.jpg and thumbnails
        assert!(sdr_path.join("cover.jpg").is_file());
        assert!(sdr_path.join(format!("thumbnail_{}_EBOK_portrait.jpg", asin)).is_file());
        assert!(sdr_path.join(format!("thumbnail_{}_PDOC_portrait.jpg", asin)).is_file());
        assert!(sdr_path.join(format!("thumbnail_{}_portrait.jpg", asin)).is_file());
        assert!(sdr_path.join(format!("thumbnail_{}_EBOK_portrait.jpg", content_id)).is_file());
    }
}
