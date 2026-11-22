//! Kindle Companion SDR (.sdr) Sidecar & Thumbnail Deployment
//!
//! Provides a single unified implementation for generating Kindle companion `.sdr` directory
//! structures, including Word Wise SQLite database sidecars (`.kll`), 10-character canonical ASIN
//! aliases, and multi-identifier lockscreen thumbnails across both local conversions and USB sync.

use crate::sync::sanitize::{
    collect_sidecar_stems, collect_thumbnail_stems, sanitize_asin, sanitize_cde_content_type,
    sanitize_sidecar_stem,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

struct SdrSidecarTarget<'a> {
    path: PathBuf,
    bytes: &'a [u8],
}

/// Deduplicated target file plan for `.sdr` sidecar deployment.
struct SdrSidecarFiles<'a> {
    targets: Vec<SdrSidecarTarget<'a>>,
    seen_paths: HashSet<PathBuf>,
}

impl<'a> SdrSidecarFiles<'a> {
    fn new() -> Self {
        Self {
            targets: Vec::new(),
            seen_paths: HashSet::new(),
        }
    }

    fn push(&mut self, path: PathBuf, bytes: &'a [u8]) {
        if self.seen_paths.insert(path.clone()) {
            self.targets.push(SdrSidecarTarget { path, bytes });
        }
    }
}

pub struct SdrDeployer;

impl SdrDeployer {
    /// Returns `true` when `path` targets a GVFS/MTP virtual filesystem mount or URI.
    fn is_mtp_path(path: &Path) -> bool {
        let s = path.to_string_lossy();
        s.contains("/gvfs/mtp:") || s.starts_with("mtp://") || s.starts_with("gio://")
    }

    /// Sanitizes an untrusted language tag into a safe filename token (`<= 32` ASCII bytes).
    fn sanitize_language_tag(lang: &str) -> String {
        let mut clean: String = lang
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        clean = clean.trim_matches(['-', '_']).to_string();
        while clean.len() > 32 {
            clean.pop();
        }
        clean = clean.trim_end_matches(['-', '_']).to_string();
        if clean.is_empty() {
            "en".to_string()
        } else {
            clean
        }
    }

    /// Collects deduplicated sanitized language tag variants for `.kll` sidecar filenames.
    fn collect_language_tags(book_language: Option<&str>) -> Vec<String> {
        let mut lang_tags = vec!["en".to_string(), "EN".to_string()];
        let Some(raw_lang) = book_language else {
            return lang_tags;
        };
        let clean = Self::sanitize_language_tag(raw_lang);
        let upper = clean.to_ascii_uppercase();
        let lower = clean.to_ascii_lowercase();
        let parts: Vec<&str> = clean.split(['-', '_']).filter(|p| !p.is_empty()).collect();
        let bcp47 = if parts.len() == 2 {
            format!(
                "{}-{}",
                parts[0].to_ascii_lowercase(),
                parts[1].to_ascii_uppercase()
            )
        } else {
            clean.clone()
        };
        let primary = parts.first().copied().unwrap_or(clean.as_str());
        let primary_upper = primary.to_ascii_uppercase();
        let primary_lower = primary.to_ascii_lowercase();

        for tag in [clean, upper, lower, bcp47, primary_upper, primary_lower] {
            if !tag.is_empty() && !lang_tags.contains(&tag) {
                lang_tags.push(tag);
            }
        }
        lang_tags
    }

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

    fn run_gio_mkdir(dir: &Path) -> std::io::Result<()> {
        let output = std::process::Command::new("gio")
            .arg("mkdir")
            .arg("-p")
            .arg("--")
            .arg(dir)
            .output()?;
        if output.status.success() || dir.is_dir() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(std::io::Error::other(format!(
                "gio mkdir -p failed for {:?} (status {:?}): {}",
                dir,
                output.status.code(),
                stderr.trim()
            )))
        }
    }

    /// Ensures a target directory exists, using `gio mkdir -p --` on GVFS/MTP paths
    /// and `std::fs::create_dir_all` on local filesystems.
    pub fn ensure_dir_exists(dir: &Path) -> std::io::Result<()> {
        let s = dir.to_string_lossy();
        if s.starts_with("gio://") || s.starts_with("mtp://") {
            return Self::run_gio_mkdir(dir);
        }
        if dir.is_dir() {
            return Ok(());
        }
        match std::fs::create_dir_all(dir) {
            Ok(()) => Ok(()),
            Err(_) if Self::is_mtp_path(dir) => Self::run_gio_mkdir(dir),
            Err(err) => Err(err),
        }
    }

    /// Safely writes bytes to a file inside the `.sdr` directory, handling pre-existing read-only permissions
    /// and performing atomic file persistence via a staged temporary file (or `gio copy` on GVFS MTP).
    fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if Self::is_mtp_path(path) {
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

        let safe_asin = sanitize_asin(asin);
        let safe_cid = content_id.and_then(sanitize_sidecar_stem);
        let mut plan = SdrSidecarFiles::new();

        // 1. Plan deduplicated Word Wise sidecars (.kll) across language tags and ID aliases
        if let Some(kll) = kll_bytes {
            let lang_tags = Self::collect_language_tags(book_language);
            let kll_ids = collect_sidecar_stems(Some(&safe_asin), safe_cid.as_deref());
            for id in &kll_ids {
                for tag in &lang_tags {
                    plan.push(
                        sdr_dir.join(format!("LanguageLayer.{}.{}.kll", tag, id)),
                        kll,
                    );
                }
            }
        }

        // 2. Plan deduplicated Cover Thumbnails
        if let Some(jpeg) = cover_jpeg {
            plan.push(sdr_dir.join("cover.jpg"), jpeg);

            let cde = sanitize_cde_content_type(cde_content_type.unwrap_or("EBOK"));
            let alt_cde = if cde == "EBOK" { "PDOC" } else { "EBOK" };
            let thumb_ids = collect_thumbnail_stems(Some(&safe_asin), safe_cid.as_deref());

            for id in &thumb_ids {
                plan.push(
                    sdr_dir.join(format!("thumbnail_{}_{}_portrait.jpg", id, cde)),
                    jpeg,
                );
                plan.push(
                    sdr_dir.join(format!("thumbnail_{}_{}_portrait.jpg", id, alt_cde)),
                    jpeg,
                );
                plan.push(sdr_dir.join(format!("thumbnail_{}_portrait.jpg", id)), jpeg);
            }
        }

        for target in plan.targets {
            Self::write_file(&target.path, target.bytes)?;
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

        // 3. SdrDeployer::deploy propagates symlink rejection errors while leaving targets untouched
        let sdr_dir = temp.path().join("Book.sdr");
        std::fs::create_dir_all(&sdr_dir).unwrap();
        let deploy_victim = temp.path().join("deploy_victim.jpg");
        std::fs::write(&deploy_victim, b"DEPLOY_VICTIM").unwrap();
        symlink(&deploy_victim, sdr_dir.join("cover.jpg")).unwrap();

        let kll_victim = temp.path().join("kll_victim.db");
        std::fs::write(&kll_victim, b"KLL_VICTIM").unwrap();
        symlink(&kll_victim, sdr_dir.join("LanguageLayer.en.B00SYMLINK.kll")).unwrap();

        let err_deploy_kll = SdrDeployer::deploy(
            &sdr_dir,
            "B00SYMLINK",
            None,
            Some("EBOK"),
            Some("en"),
            Some(b"NEW_JPEG"),
            Some(b"NEW_KLL"),
        )
        .unwrap_err();
        assert_eq!(err_deploy_kll.kind(), ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read(&deploy_victim).unwrap(), b"DEPLOY_VICTIM");
        assert_eq!(std::fs::read(&kll_victim).unwrap(), b"KLL_VICTIM");

        // Cover-only deploy also rejects pre-planted cover.jpg symlink
        let err_deploy_cover = SdrDeployer::deploy(
            &sdr_dir,
            "B00SYMLINK",
            None,
            Some("EBOK"),
            Some("en"),
            Some(b"NEW_JPEG"),
            None,
        )
        .unwrap_err();
        assert_eq!(err_deploy_cover.kind(), ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read(&deploy_victim).unwrap(), b"DEPLOY_VICTIM");

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

    #[test]
    fn test_sdr_deployer_sanitizes_tags_and_deduplicates_targets() {
        let temp = tempfile::tempdir().unwrap();
        let sdr_path = temp.path().join("Sanitize.sdr");

        // Malicious traversal tags and aliasing content_id ("B123456789")
        let asin = "123456789ABCDEF0123456789ABCDEF0";
        SdrDeployer::deploy(
            &sdr_path,
            asin,
            Some("b123456789"),
            Some("../../evil"),
            Some("../../etc/en-US"),
            Some(b"JPEG"),
            Some(b"KLL"),
        )
        .unwrap();

        // cde_content_type defaulted to EBOK/PDOC only
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
        // No file escaped into temp.path()
        assert!(!temp.path().join("evil").exists());
        assert!(sdr_path.join("LanguageLayer.en.B123456789.kll").is_file());
    }

    #[test]
    fn test_sdr_ensure_dir_exists_creates_nested_thumbnail_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let dot_thumbs = temp.path().join(".system").join("thumbnails");
        let sys_thumbs = temp.path().join("system").join("thumbnails");

        SdrDeployer::ensure_dir_exists(&dot_thumbs).expect("create .system/thumbnails");
        SdrDeployer::ensure_dir_exists(&dot_thumbs).expect("idempotent .system/thumbnails");
        SdrDeployer::ensure_dir_exists(&sys_thumbs).expect("create system/thumbnails");

        assert!(dot_thumbs.is_dir());
        assert!(sys_thumbs.is_dir());
    }
}
