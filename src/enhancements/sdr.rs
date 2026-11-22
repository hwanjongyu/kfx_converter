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
        if let Ok(metadata) = std::fs::symlink_metadata(path)
            && !metadata.file_type().is_symlink()
            && metadata.permissions().readonly()
        {
            let mut perms = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(path, perms);
        }
    }

    /// Ensures a target directory exists, falling back to `gio mkdir -p` on Linux GVFS MTP mounts.
    fn ensure_dir_exists(dir: &Path) -> std::io::Result<()> {
        if dir.is_dir() {
            return Ok(());
        }
        match std::fs::create_dir_all(dir) {
            Ok(()) => Ok(()),
            Err(err) if dir.to_string_lossy().contains("/gvfs/mtp:") => {
                let status = std::process::Command::new("gio")
                    .arg("mkdir")
                    .arg("-p")
                    .arg("--")
                    .arg(dir)
                    .status()?;
                if status.success() || dir.is_dir() {
                    Ok(())
                } else {
                    Err(err)
                }
            }
            Err(err) => Err(err),
        }
    }

    /// Safely writes bytes to a file inside the `.sdr` directory, handling pre-existing read-only permissions
    /// and performing atomic file persistence via a staged temporary file (or `gio copy` on GVFS MTP).
    fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if path.to_string_lossy().contains("/gvfs/mtp:") {
            return crate::sync::transfer::write_bytes(path, bytes).map_err(|e| match e {
                crate::sync::SyncError::Io(io_err) => io_err,
                other => std::io::Error::other(other.to_string()),
            });
        }

        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("Refusing to overwrite symlink destination: {:?}", path),
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }

        let parent = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        let mut tmp_file = tempfile::Builder::new()
            .prefix(".sdr_tmp.")
            .tempfile_in(parent)?;
        std::io::Write::write_all(&mut tmp_file, bytes)?;
        std::io::Write::flush(&mut tmp_file)?;
        Self::prepare_for_overwrite(path);
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
        Self::ensure_dir_exists(sdr_dir)?;

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
                        format!(
                            "{}-{}",
                            parts[0].to_ascii_lowercase(),
                            parts[1].to_ascii_uppercase()
                        )
                    } else {
                        clean.to_string()
                    };
                    let primary_upper = parts[0].to_ascii_uppercase();
                    let primary_lower = parts[0].to_ascii_lowercase();

                    for tag in [
                        clean.to_string(),
                        upper,
                        lower,
                        bcp47,
                        primary_upper,
                        primary_lower,
                    ] {
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
        assert!(
            sdr_path
                .join(format!("LanguageLayer.en.{}.kll", asin))
                .is_file()
        );
        assert!(
            sdr_path
                .join(format!("LanguageLayer.EN-CA.{}.kll", asin))
                .is_file()
        );
        assert!(
            sdr_path
                .join(format!("LanguageLayer.EN-CA.B{}.kll", &asin[..9]))
                .is_file()
        );
        assert!(
            sdr_path
                .join(format!("LanguageLayer.en.B{}.kll", &asin[..9]))
                .is_file()
        );
        assert!(
            sdr_path
                .join(format!("LanguageLayer.en.{}.kll", content_id))
                .is_file()
        );
        assert!(
            sdr_path
                .join(format!("LanguageLayer.EN-CA.{}.kll", content_id))
                .is_file()
        );

        // 2. Verify cover.jpg and thumbnails
        assert!(sdr_path.join("cover.jpg").is_file());
        assert!(
            sdr_path
                .join(format!("thumbnail_{}_EBOK_portrait.jpg", asin))
                .is_file()
        );
        assert!(
            sdr_path
                .join(format!("thumbnail_{}_PDOC_portrait.jpg", asin))
                .is_file()
        );
        assert!(
            sdr_path
                .join(format!("thumbnail_{}_portrait.jpg", asin))
                .is_file()
        );
        assert!(
            sdr_path
                .join(format!("thumbnail_{}_EBOK_portrait.jpg", content_id))
                .is_file()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_sdr_write_file_rejects_symlink_destination() {
        use std::io::ErrorKind;
        use std::os::unix::fs::{PermissionsExt, symlink};

        // Isolated child branch for bare relative filename destination without mutating parent CWD
        if std::env::var_os("KFX_REL_SDR_CHILD").is_some() {
            assert_eq!(Path::new("rel_sidecar.kll").parent(), Some(Path::new("")));
            SdrDeployer::write_file(Path::new("rel_sidecar.kll"), b"REL_KLL_DATA").unwrap();
            assert_eq!(std::fs::read("rel_sidecar.kll").unwrap(), b"REL_KLL_DATA");
            return;
        }

        let temp = tempfile::tempdir().unwrap();

        // 1. Live symlink destination pointing to read-only victim file
        let victim = temp.path().join("sdr_victim.txt");
        std::fs::write(&victim, b"UNTOUCHED_SDR_VICTIM").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o444)).unwrap();

        let live_link = temp.path().join("cover.jpg");
        symlink(&victim, &live_link).unwrap();

        let err_live = SdrDeployer::write_file(&live_link, b"OVERWRITE").unwrap_err();
        assert_eq!(err_live.kind(), ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read(&victim).unwrap(), b"UNTOUCHED_SDR_VICTIM");
        assert_eq!(
            std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
            0o444
        );

        // 2. Dangling symlink destination
        let missing = temp.path().join("missing_sdr_target.txt");
        let dangling_link = temp.path().join("dangling.kll");
        symlink(&missing, &dangling_link).unwrap();

        let err_dangling = SdrDeployer::write_file(&dangling_link, b"OVERWRITE").unwrap_err();
        assert_eq!(err_dangling.kind(), ErrorKind::PermissionDenied);
        assert!(!missing.exists());

        // 3. SdrDeployer::deploy leaves pre-planted symlink targets inside .sdr untouched
        let sdr_dir = temp.path().join("Book.sdr");
        std::fs::create_dir_all(&sdr_dir).unwrap();
        let deploy_victim = temp.path().join("deploy_victim.jpg");
        std::fs::write(&deploy_victim, b"DEPLOY_VICTIM").unwrap();
        symlink(&deploy_victim, sdr_dir.join("cover.jpg")).unwrap();

        let kll_victim = temp.path().join("kll_victim.db");
        std::fs::write(&kll_victim, b"KLL_VICTIM").unwrap();
        symlink(&kll_victim, sdr_dir.join("LanguageLayer.en.B00SYMLINK.kll")).unwrap();

        SdrDeployer::deploy(
            &sdr_dir,
            "B00SYMLINK",
            None,
            Some("EBOK"),
            Some("en"),
            Some(b"NEW_JPEG"),
            Some(b"NEW_KLL"),
        )
        .unwrap();

        assert_eq!(std::fs::read(&deploy_victim).unwrap(), b"DEPLOY_VICTIM");
        assert_eq!(std::fs::read(&kll_victim).unwrap(), b"KLL_VICTIM");
        assert_eq!(
            std::fs::read(sdr_dir.join("thumbnail_B00SYMLINK_EBOK_portrait.jpg")).unwrap(),
            b"NEW_JPEG"
        );

        // 4. Relative filename destination in isolated child process CWD
        let rel_dir = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let output = std::process::Command::new(exe)
            .args([
                "--exact",
                "enhancements::sdr::tests::test_sdr_write_file_rejects_symlink_destination",
            ])
            .current_dir(rel_dir.path())
            .env("KFX_REL_SDR_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Relative SDR child failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read(rel_dir.path().join("rel_sidecar.kll")).unwrap(),
            b"REL_KLL_DATA"
        );
    }
}
