//! Kindle Companion SDR (.sdr) Sidecar & Thumbnail Deployment
//!
//! Provides a single unified implementation for generating Kindle companion `.sdr` directory
//! structures, including Word Wise SQLite database sidecars (`.kll`), 10-character canonical ASIN
//! aliases, and multi-identifier lockscreen thumbnails across both local conversions and USB sync.

use crate::storage::{
    collect_sidecar_stems, collect_thumbnail_stems, ensure_dir_exists_safe, sanitize_asin,
    sanitize_cde_content_type, sanitize_sidecar_stem, write_bytes_with_mtp,
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
    seen_keys: HashSet<String>,
    case_insensitive: bool,
}

impl<'a> SdrSidecarFiles<'a> {
    fn new(case_insensitive: bool) -> Self {
        Self {
            targets: Vec::new(),
            seen_keys: HashSet::new(),
            case_insensitive,
        }
    }

    fn push(&mut self, path: PathBuf, bytes: &'a [u8]) {
        let key = if self.case_insensitive {
            path.to_string_lossy().to_ascii_lowercase()
        } else {
            path.to_string_lossy().into_owned()
        };
        if self.seen_keys.insert(key) {
            self.targets.push(SdrSidecarTarget { path, bytes });
        }
    }
}

/// Detects whether `dir` targets a case-insensitive or MTP filesystem where case-variant
/// sidecar filenames (e.g. `en` vs `EN`, `en-US` vs `EN-US`) alias to the exact same entry.
fn is_case_insensitive_target(dir: &Path) -> bool {
    if cfg!(any(target_os = "windows", target_os = "macos")) || crate::storage::is_mtp_path(dir) {
        return true;
    }

    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        const MSDOS_SUPER_MAGIC: u64 = 0x4d44;
        const EXFAT_SUPER_MAGIC: u64 = 0x2011_bab0;
        const NTFS_SB_MAGIC: u64 = 0x5346_544e;
        const FUSE_SUPER_MAGIC: u64 = 0x6573_5546;

        let mut probe = dir;
        while !probe.exists() {
            match probe.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => probe = parent,
                _ => {
                    probe = Path::new(".");
                    break;
                }
            }
        }

        if let Ok(c_path) = CString::new(probe.as_os_str().as_bytes()) {
            // SAFETY: `libc::statfs` is a C POD struct where all-zero bits is a valid initial state.
            let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
            // SAFETY: `c_path` is a valid NUL-terminated `CString` and `stat` is a valid output buffer.
            if unsafe { libc::statfs(c_path.as_ptr(), &mut stat) } == 0 {
                return matches!(
                    stat.f_type as u64,
                    MSDOS_SUPER_MAGIC | EXFAT_SUPER_MAGIC | NTFS_SB_MAGIC | FUSE_SUPER_MAGIC
                );
            }
        }
    }

    false
}

pub struct SdrDeployer;

impl SdrDeployer {
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

    /// Ensures a target directory exists safely, using bounded `gio mkdir -p --` on GVFS/MTP paths
    /// and symlink-rejecting directory creation on local filesystems.
    pub fn ensure_dir_exists(dir: &Path) -> std::io::Result<()> {
        ensure_dir_exists_safe(dir)
    }

    /// Safely writes bytes to a file inside the `.sdr` directory, handling pre-existing read-only permissions
    /// and performing atomic file persistence via a staged temporary file (or bounded `gio copy` on GVFS MTP).
    #[cfg_attr(not(test), allow(dead_code))]
    fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        Self::write_file_with_mtp(path, bytes, true)
    }

    fn write_file_with_mtp(path: &Path, bytes: &[u8], lock_mtp: bool) -> std::io::Result<()> {
        write_bytes_with_mtp(path, bytes, lock_mtp)
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
        Self::deploy_inner(
            sdr_dir,
            asin,
            content_id,
            cde_content_type,
            book_language,
            cover_jpeg,
            kll_bytes,
            true,
        )
    }

    /// Internal unlocked deployment helper for callers already holding `DEVICE_SYNC_LOCK`.
    pub(crate) fn deploy_unlocked(
        sdr_dir: &Path,
        asin: &str,
        content_id: Option<&str>,
        cde_content_type: Option<&str>,
        book_language: Option<&str>,
        cover_jpeg: Option<&[u8]>,
        kll_bytes: Option<&[u8]>,
    ) -> std::io::Result<()> {
        Self::deploy_inner(
            sdr_dir,
            asin,
            content_id,
            cde_content_type,
            book_language,
            cover_jpeg,
            kll_bytes,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_sidecar_targets<'a>(
        sdr_dir: &Path,
        asin: &str,
        content_id: Option<&str>,
        cde_content_type: Option<&str>,
        book_language: Option<&str>,
        cover_jpeg: Option<&'a [u8]>,
        kll_bytes: Option<&'a [u8]>,
        case_insensitive: bool,
    ) -> Vec<SdrSidecarTarget<'a>> {
        let safe_asin = sanitize_asin(asin);
        let safe_cid = content_id.and_then(sanitize_sidecar_stem);
        let mut plan = SdrSidecarFiles::new(case_insensitive);

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

        plan.targets
    }

    #[allow(clippy::too_many_arguments)]
    fn deploy_inner(
        sdr_dir: &Path,
        asin: &str,
        content_id: Option<&str>,
        cde_content_type: Option<&str>,
        book_language: Option<&str>,
        cover_jpeg: Option<&[u8]>,
        kll_bytes: Option<&[u8]>,
        lock_mtp: bool,
    ) -> std::io::Result<()> {
        Self::ensure_dir_exists(sdr_dir)?;

        let case_insensitive = is_case_insensitive_target(sdr_dir);
        let targets = Self::plan_sidecar_targets(
            sdr_dir,
            asin,
            content_id,
            cde_content_type,
            book_language,
            cover_jpeg,
            kll_bytes,
            case_insensitive,
        );

        for target in targets {
            Self::write_file_with_mtp(&target.path, target.bytes, lock_mtp)?;
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

    #[cfg(unix)]
    #[test]
    fn test_sdr_ensure_dir_exists_and_deploy_reject_symlinked_sdr_dir() {
        use std::io::ErrorKind;
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let outside_dir = temp.path().join("outside_target");
        std::fs::create_dir_all(&outside_dir).unwrap();
        let sdr_link = temp.path().join("Book.sdr");
        symlink(&outside_dir, &sdr_link).unwrap();

        let err_dir = SdrDeployer::ensure_dir_exists(&sdr_link).unwrap_err();
        assert_eq!(err_dir.kind(), ErrorKind::PermissionDenied);

        let err_nested = SdrDeployer::ensure_dir_exists(&sdr_link.join("nested")).unwrap_err();
        assert_eq!(err_nested.kind(), ErrorKind::PermissionDenied);

        let err_deploy = SdrDeployer::deploy(
            &sdr_link,
            "B00SYMLINKDIR",
            None,
            Some("EBOK"),
            Some("en"),
            Some(b"JPEG"),
            Some(b"KLL"),
        )
        .unwrap_err();
        assert_eq!(err_deploy.kind(), ErrorKind::PermissionDenied);
        assert!(
            std::fs::read_dir(&outside_dir).unwrap().next().is_none(),
            "Outside directory targeted by symlinked .sdr must remain untouched"
        );
    }

    #[test]
    fn test_plan_sidecar_targets_case_insensitive_deduplication() {
        let sdr_dir = Path::new("/mock/Kindle/documents/Book.sdr");
        let asin = "123456789ABCDEF0123456789ABCDEF0"; // yields 2 stems: full asin + B123456789

        // en-US with 2 stems: 5 case-sensitive tags x 2 = 10 writes vs 2 case-insensitive tags x 2 = 4 writes
        let cs_en_us = SdrDeployer::plan_sidecar_targets(
            sdr_dir,
            asin,
            None,
            Some("EBOK"),
            Some("en-US"),
            None,
            Some(b"KLL"),
            false,
        );
        let ci_en_us = SdrDeployer::plan_sidecar_targets(
            sdr_dir,
            asin,
            None,
            Some("EBOK"),
            Some("en-US"),
            None,
            Some(b"KLL"),
            true,
        );
        assert_eq!(cs_en_us.len(), 10);
        assert_eq!(ci_en_us.len(), 4);
        let ci_names: Vec<String> = ci_en_us
            .iter()
            .map(|t| t.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            ci_names,
            vec![
                format!("LanguageLayer.en.{}.kll", asin),
                format!("LanguageLayer.en-US.{}.kll", asin),
                "LanguageLayer.en.B123456789.kll".to_string(),
                "LanguageLayer.en-US.B123456789.kll".to_string(),
            ]
        );

        // en with 2 stems: 2 case-sensitive tags x 2 = 4 writes vs 1 case-insensitive tag x 2 = 2 writes
        let cs_en = SdrDeployer::plan_sidecar_targets(
            sdr_dir,
            asin,
            None,
            Some("EBOK"),
            Some("en"),
            None,
            Some(b"KLL"),
            false,
        );
        let ci_en = SdrDeployer::plan_sidecar_targets(
            sdr_dir,
            asin,
            None,
            Some("EBOK"),
            Some("en"),
            None,
            Some(b"KLL"),
            true,
        );
        assert_eq!(cs_en.len(), 4);
        assert_eq!(ci_en.len(), 2);

        // MTP paths must always be treated as case-insensitive targets
        assert!(is_case_insensitive_target(Path::new(
            "/run/user/1000/gvfs/mtp:host=Amazon_Kindle/Internal Storage/documents/Book.sdr"
        )));
        assert!(is_case_insensitive_target(Path::new(
            "mtp://Amazon_Kindle/documents/Book.sdr"
        )));
        assert!(is_case_insensitive_target(Path::new(
            "gio://mtp/Kindle/documents/Book.sdr"
        )));
    }
}
