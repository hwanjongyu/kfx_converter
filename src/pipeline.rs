//! Unified Conversion Pipeline Service
//!
//! Provides a single, clean facade for converting EPUB documents into Amazon KFX containers,
//! generating companion Word Wise SQLite sidecars, normalizing baseline covers, and deploying
//! to physical Kindle hardware via USB Mass Storage (UMS) or MTP.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

static LOG_LOCK: Mutex<()> = Mutex::new(());

/// Returns the default persistent conversion log file path.
pub fn conversion_log_path() -> PathBuf {
    #[cfg(windows)]
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join("kfx-converter")
            .join("conversion.log");
    }

    if let Some(xdg_data) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg_data)
            .join("kfx-converter")
            .join("conversion.log");
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("kfx-converter")
            .join("conversion.log");
    }

    #[cfg(unix)]
    {
        // SAFETY: libc::geteuid() takes no arguments and is always safe to call.
        let uid = unsafe { libc::geteuid() };
        std::env::temp_dir()
            .join(format!("kfx-converter-{}", uid))
            .join("conversion.log")
    }

    #[cfg(not(unix))]
    {
        std::env::temp_dir()
            .join("kfx-converter")
            .join("conversion.log")
    }
}

#[cfg(unix)]
fn is_inside_temp_dir(path: &Path) -> bool {
    let tmp = std::env::temp_dir();
    if path.starts_with(&tmp) {
        return true;
    }
    if let Ok(canon_tmp) = tmp.canonicalize()
        && path.starts_with(&canon_tmp)
    {
        return true;
    }
    false
}

/// Verifies or creates the conversion log directory without following symlinks.
/// On Unix, directories under the shared temp directory must be owned by the current UID
/// and restricted to mode `0o700`.
fn ensure_safe_log_dir(parent: &Path) -> Result<(), ()> {
    if parent.as_os_str().is_empty() || parent == Path::new(".") {
        return Ok(());
    }

    #[cfg(unix)]
    let verify_unix_temp_dir = |dir: &Path, meta: &std::fs::Metadata| -> Result<(), ()> {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        if !is_inside_temp_dir(dir) {
            return Ok(());
        }
        // SAFETY: libc::geteuid() takes no arguments and is always safe to call.
        let euid = unsafe { libc::geteuid() };
        if meta.uid() != euid {
            return Err(());
        }
        if (meta.mode() & 0o777) != 0o700 {
            let dir_file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
                .open(dir)
                .map_err(|_| ())?;
            let fd_meta = dir_file.metadata().map_err(|_| ())?;
            if fd_meta.uid() != euid || !fd_meta.is_dir() {
                return Err(());
            }
            dir_file
                .set_permissions(std::fs::Permissions::from_mode(0o700))
                .map_err(|_| ())?;
        }
        Ok(())
    };

    match std::fs::symlink_metadata(parent) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
                return Err(());
            }
            #[cfg(unix)]
            verify_unix_temp_dir(parent, &meta)?;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(grandparent) = parent.parent()
                && !grandparent.as_os_str().is_empty()
            {
                crate::storage::ensure_non_symlink_dir(grandparent).map_err(|_| ())?;
            }
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(parent) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(()),
            }
            let meta = std::fs::symlink_metadata(parent).map_err(|_| ())?;
            if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
                return Err(());
            }
            #[cfg(unix)]
            verify_unix_temp_dir(parent, &meta)?;
            Ok(())
        }
        Err(_) => Err(()),
    }
}

/// Rotates `path` to `log.old` when it exceeds `max_bytes`, rejecting symlink targets
/// and falling back to `O_NOFOLLOW` copy + truncate/remove if `rename` fails.
fn rotate_log_if_needed(path: &Path, max_bytes: u64) -> Result<(), ()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(()),
    };
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(());
    }
    if meta.len() <= max_bytes {
        return Ok(());
    }

    let backup_path = path.with_extension("log.old");
    let mut backup_is_dir = false;
    match std::fs::symlink_metadata(&backup_path) {
        Ok(bm) if bm.file_type().is_symlink() => {
            // Remove the symlink entry itself without following it; abort if removal fails.
            if std::fs::remove_file(&backup_path).is_err() {
                return Err(());
            }
        }
        Ok(bm) if bm.file_type().is_file() => {
            let _ = std::fs::remove_file(&backup_path);
        }
        Ok(bm) if bm.file_type().is_dir() => {
            // Preserve blocking directory so rename fails into the truncation fallback cleanly.
            backup_is_dir = true;
        }
        Ok(_) => return Err(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(()),
    }

    if !backup_is_dir && std::fs::rename(path, &backup_path).is_ok() {
        return Ok(());
    }

    if !backup_is_dir
        && !std::fs::symlink_metadata(&backup_path)
            .is_ok_and(|bm| bm.file_type().is_symlink() || bm.file_type().is_dir())
    {
        let mut src_opts = std::fs::OpenOptions::new();
        src_opts.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            src_opts.custom_flags(libc::O_NOFOLLOW);
        }

        let mut dst_opts = std::fs::OpenOptions::new();
        dst_opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            dst_opts.custom_flags(libc::O_NOFOLLOW).mode(0o600);
        }

        if let (Ok(mut src_file), Ok(mut dst_file)) =
            (src_opts.open(path), dst_opts.open(&backup_path))
        {
            let _ = std::io::copy(&mut src_file, &mut dst_file);
        }
    }

    let mut trunc_opts = std::fs::OpenOptions::new();
    trunc_opts.write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        trunc_opts.custom_flags(libc::O_NOFOLLOW);
    }
    if trunc_opts.open(path).is_err()
        && std::fs::symlink_metadata(path)
            .is_ok_and(|m| !m.file_type().is_symlink() && m.file_type().is_file())
    {
        let _ = std::fs::remove_file(path);
    }

    Ok(())
}

/// Appends a timestamped log line to `path` with symlink-safe, non-fatal rotation.
fn append_conversion_log_at(path: &Path, message: &str) {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && ensure_safe_log_dir(parent).is_err()
    {
        return;
    }
    if rotate_log_if_needed(path, 2 * 1024 * 1024).is_err() {
        return;
    }
    if crate::storage::reject_symlink_path(path).is_err() {
        return;
    }
    let mut open_opts = std::fs::OpenOptions::new();
    open_opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open_opts.custom_flags(libc::O_NOFOLLOW).mode(0o600);
    }
    if let Ok(mut file) = open_opts.open(path) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "[{}] {}", ts, message);
    }
}

fn log_pipeline_event(log_path: Option<&Path>, message: &str) {
    let Some(path) = log_path else {
        return;
    };
    let _guard = match LOG_LOCK.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    append_conversion_log_at(path, message);
}

use crate::enhancements::{CoverOptimizer, WordWiseBuilder};
use crate::epub::{EpubError, EpubReader};
use crate::packager::{KfxPackager, PackagerError};
use crate::sync::{KindleSync, SyncError};

/// Returns `true` when `lang` is empty or has an English primary language subtag (`en` / `eng`),
/// matching BCP-47 hyphen or underscore tags (e.g. `"en"`, `"en-US"`, `"en_GB"`, `"eng"`).
pub(crate) fn is_english_language_tag(lang: &str) -> bool {
    let trimmed = lang.trim();
    if trimmed.is_empty() {
        return true;
    }
    let primary = trimmed
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(primary.as_str(), "en" | "eng")
}

/// Returns `true` when two target directory paths (which may not exist yet) resolve to the same
/// canonical parent directory and matching leaf directory name (ASCII case-insensitive on
/// Windows/macOS/MTP, exact on case-sensitive Unix).
fn paths_match_target_dir(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    let (Some(leaf_a), Some(leaf_b)) = (a.file_name(), b.file_name()) else {
        return false;
    };
    let parent_a = match a.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let parent_b = match b.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let (Ok(canon_a), Ok(canon_b)) = (
        std::fs::canonicalize(parent_a),
        std::fs::canonicalize(parent_b),
    ) else {
        return false;
    };
    if canon_a != canon_b {
        return false;
    }
    let case_insensitive = cfg!(any(target_os = "windows", target_os = "macos"))
        || crate::storage::is_mtp_path(a)
        || crate::storage::is_mtp_path(b);
    if case_insensitive {
        leaf_a
            .to_string_lossy()
            .eq_ignore_ascii_case(&leaf_b.to_string_lossy())
    } else {
        leaf_a == leaf_b
    }
}

#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("EPUB read error: {0}")]
    Epub(#[from] EpubError),
    #[error("KFX packaging error: {0}")]
    Packager(#[from] PackagerError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Sync error: {0}")]
    Sync(#[from] SyncError),
    #[error("Pipeline error: {0}")]
    Other(String),
}

/// Configuration options for the conversion pipeline.
#[derive(Debug, Clone)]
pub struct ConversionOptions {
    pub content_type: String,
    pub word_wise: bool,
    pub auto_deploy: bool,
    pub device_root: Option<PathBuf>,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            content_type: crate::ir::ContentType::default().as_str().to_string(),
            word_wise: true,
            auto_deploy: true,
            device_root: None,
        }
    }
}

impl ConversionOptions {
    pub fn with_content_type(mut self, content_type: crate::ir::ContentType) -> Self {
        self.content_type = content_type.as_str().to_string();
        self
    }

    pub fn resolved_content_type(&self) -> crate::ir::ContentType {
        crate::ir::ContentType::from_lossy(&self.content_type)
    }
}

/// Execution report detailing the output of a conversion pipeline run.
#[derive(Debug, Clone)]
pub struct ConversionReport {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub book_title: String,
    pub creators: Vec<String>,
    pub sections_count: usize,
    pub resources_count: usize,
    pub total_bytes: u64,
    pub duration: Duration,
    pub cover_optimized: bool,
    pub word_wise_glosses: Option<usize>,
    pub word_wise_error: Option<String>,
    pub sdr_error: Option<String>,
    pub synced_to_device: bool,
    pub synced_device_path: Option<PathBuf>,
    pub sync_error: Option<String>,
}

/// Unified conversion pipeline coordinator.
pub struct ConversionPipeline;

impl ConversionPipeline {
    /// Executes the complete EPUB to KFX conversion pipeline with an optional progress callback.
    pub fn run<P: AsRef<Path>, F: Fn(f32, &str)>(
        input_path: P,
        output_path: Option<&Path>,
        options: &ConversionOptions,
        progress: Option<F>,
    ) -> Result<ConversionReport, PipelineError> {
        let log_path = conversion_log_path();
        Self::run_with_log_path(input_path, output_path, options, Some(&log_path), progress)
    }

    /// Executes the complete EPUB to KFX conversion pipeline, optionally logging events to `log_path`.
    pub fn run_with_log_path<P: AsRef<Path>, F: Fn(f32, &str)>(
        input_path: P,
        output_path: Option<&Path>,
        options: &ConversionOptions,
        log_path: Option<&Path>,
        progress: Option<F>,
    ) -> Result<ConversionReport, PipelineError> {
        let input_path = input_path.as_ref().to_path_buf();
        if !input_path.exists() {
            return Err(PipelineError::Other(format!(
                "Input file not found: {:?}",
                input_path
            )));
        }

        let output_path = match output_path {
            Some(p) => p.to_path_buf(),
            None => {
                let mut out = input_path.clone();
                out.set_extension("kfx");
                out
            }
        };

        let start_time = Instant::now();

        // 1. Read EPUB into BookIR
        if let Some(ref cb) = progress {
            cb(0.1, "Reading and validating EPUB archive...");
        }
        let mut ir = EpubReader::read_from_file(&input_path)?;
        ir.normalize();
        let book_title = ir.metadata.title.clone();
        let creators = ir.metadata.creators.clone();
        let sections_count = ir.sections.len();
        let resources_count = ir.resources.len();

        // 2. Package into Amazon KFX container
        if let Some(ref cb) = progress {
            cb(0.3, "Serializing Yellowjacket KFX container...");
        }
        let resolved_content_type = options.resolved_content_type();
        let mut packager = KfxPackager::new_with_content_type(&ir, resolved_content_type);
        let content_id = packager.content_id().to_string();
        let container_id = packager.container_id().to_string();
        let asin = packager.asin().to_string();

        let total_bytes = packager.package_to_file(&output_path)?;

        // 3. Normalize Cover Thumbnail
        if let Some(ref cb) = progress {
            cb(0.6, "Normalizing baseline cover image...");
        }
        let mut cover_jpeg = None;
        let mut cover_optimized = false;
        let cover_id_opt = ir.resolve_cover_resource_id();

        if let Some(target_id) = cover_id_opt {
            let asset = ir.resources.get(&target_id).or_else(|| {
                ir.resolve_resource_key(&target_id)
                    .and_then(|k| ir.resources.get(&k))
            });
            if let Some(asset) = asset
                && let Ok(optimized) = CoverOptimizer::optimize_thumbnail(&asset.data)
            {
                cover_jpeg = Some(optimized);
                cover_optimized = true;
            }
        }

        // 4. Generate Word Wise Sidecar (.kll) if enabled
        let mut kll_bytes = None;
        let mut word_wise_glosses = None;
        let mut word_wise_error = None;
        log_pipeline_event(
            log_path,
            &format!(
                "START input=\"{}\" output=\"{}\" word_wise={} content_type={} auto_deploy={} lang=\"{}\"",
                input_path.display(),
                output_path.display(),
                options.word_wise,
                resolved_content_type,
                options.auto_deploy,
                ir.metadata.language
            ),
        );

        if options.word_wise {
            if !is_english_language_tag(&ir.metadata.language) {
                word_wise_glosses = Some(0);
                log_pipeline_event(
                    log_path,
                    &format!(
                        "INFO Word Wise skipped for non-English book (asin={}, lang=\"{}\"); skipping .kll sidecar emission",
                        asin, ir.metadata.language
                    ),
                );
            } else {
                if let Some(ref cb) = progress {
                    cb(0.75, "Generating Word Wise lemma sidecar database...");
                }
                let book_revision = content_id
                    .chars()
                    .take(8)
                    .collect::<String>()
                    .to_lowercase();
                let mut ww_builder =
                    WordWiseBuilder::new(&asin, &container_id, "en", "en", &book_revision);
                let gloss_count = ww_builder.generate_from_book(&ir);
                word_wise_glosses = Some(gloss_count);

                if gloss_count == 0 {
                    log_pipeline_event(
                        log_path,
                        &format!(
                            "WARN Word Wise enabled but 0 glosses matched (asin={}, lang=\"{}\"); skipping .kll sidecar emission",
                            asin, ir.metadata.language
                        ),
                    );
                } else {
                    match ww_builder.to_bytes() {
                        Ok(bytes) => {
                            log_pipeline_event(
                                log_path,
                                &format!(
                                    "WORD_WISE generated glosses={} kll_bytes={}",
                                    gloss_count,
                                    bytes.len()
                                ),
                            );
                            kll_bytes = Some(bytes);
                        }
                        Err(e) => {
                            let msg = format!("Word Wise KLL serialization failed: {}", e);
                            log_pipeline_event(log_path, &format!("ERROR {}", msg));
                            word_wise_error = Some(msg);
                        }
                    }
                }
            }
        }

        // Resolve device sync target before Step 4b so local SdrDeployer::deploy is skipped when
        // Step 5 (KindleSync::sync_to_device) will deploy to the exact same .sdr destination.
        let target_root = options.device_root.clone().or_else(|| {
            if !options.auto_deploy {
                return None;
            }
            // Check if output_path is located inside a Kindle directory structure
            let effective_output = if output_path.is_absolute() {
                output_path.clone()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(&output_path))
                    .unwrap_or_else(|_| output_path.clone())
            };
            let is_real_dir = |p: PathBuf| {
                std::fs::symlink_metadata(&p)
                    .is_ok_and(|m| !m.file_type().is_symlink() && m.file_type().is_dir())
            };
            let mut current = effective_output
                .parent()
                .filter(|p| !p.as_os_str().is_empty());
            while let Some(parent) = current {
                if is_real_dir(parent.join("documents"))
                    && (is_real_dir(parent.join("system")) || is_real_dir(parent.join(".system")))
                {
                    return Some(parent.to_path_buf());
                }
                current = parent.parent().filter(|p| !p.as_os_str().is_empty());
            }
            KindleSync::detect_kindle_root()
        });

        // 4b. Deploy companion .sdr directory and sidecars alongside output_path unless Step 5
        // deploys to the exact same .sdr destination under DEVICE_SYNC_LOCK.
        let mut sdr_error = None;
        let sdr_path = output_path.with_extension("sdr");
        let skip_local_sdr = target_root.as_ref().is_some_and(|device_root| {
            let device_sdr = crate::sync::transfer::expected_sync_sdr_dir(
                device_root,
                &output_path,
                &ir.metadata.title,
            );
            paths_match_target_dir(&sdr_path, &device_sdr)
        });
        if skip_local_sdr {
            log_pipeline_event(
                log_path,
                &format!(
                    "SDR deferred to device sync path=\"{}\" has_kll={}",
                    sdr_path.display(),
                    kll_bytes.is_some()
                ),
            );
        } else if let Err(e) = crate::enhancements::SdrDeployer::deploy(
            &sdr_path,
            &asin,
            Some(&content_id),
            Some(resolved_content_type.as_str()),
            Some(&ir.metadata.language),
            cover_jpeg.as_deref(),
            kll_bytes.as_deref(),
        ) {
            let msg = format!("SDR sidecar deploy failed ({}): {}", sdr_path.display(), e);
            log_pipeline_event(log_path, &format!("ERROR {}", msg));
            sdr_error = Some(msg);
        } else {
            log_pipeline_event(
                log_path,
                &format!(
                    "SDR deployed path=\"{}\" has_kll={}",
                    sdr_path.display(),
                    kll_bytes.is_some()
                ),
            );
        }

        // 5. Automatic Device Sync when device_root is explicitly provided or auto_deploy is enabled
        let mut synced_to_device = false;
        let mut synced_device_path = None;
        let mut sync_error = None;

        if let Some(device_root) = target_root {
            if let Some(ref cb) = progress {
                cb(0.9, "Synchronizing to Kindle storage...");
            }
            match KindleSync::sync_to_device(
                &device_root,
                &output_path,
                &ir.metadata.title,
                &asin,
                Some(&content_id),
                Some(resolved_content_type.as_str()),
                Some(&ir.metadata.language),
                cover_jpeg.as_deref(),
                kll_bytes.as_deref(),
            ) {
                Ok(synced_kfx_path) => {
                    log_pipeline_event(
                        log_path,
                        &format!(
                            "SYNC ok synced_kfx_path=\"{}\" has_kll={}",
                            synced_kfx_path.display(),
                            kll_bytes.is_some()
                        ),
                    );
                    synced_to_device = true;
                    synced_device_path = Some(synced_kfx_path);
                }
                Err(e) => {
                    log_pipeline_event(log_path, &format!("ERROR SYNC failed: {}", e));
                    sync_error = Some(e.to_string());
                }
            }
        }

        if let Some(ref cb) = progress {
            cb(1.0, "Conversion completed successfully.");
        }

        let duration = start_time.elapsed();
        log_pipeline_event(
            log_path,
            &format!(
                "DONE output=\"{}\" bytes={} duration_ms={} word_wise_glosses={:?} synced={}",
                output_path.display(),
                total_bytes,
                duration.as_millis(),
                word_wise_glosses,
                synced_to_device
            ),
        );

        Ok(ConversionReport {
            input_path,
            output_path,
            book_title,
            creators,
            sections_count,
            resources_count,
            total_bytes,
            duration,
            cover_optimized,
            word_wise_glosses,
            word_wise_error,
            sdr_error,
            synced_to_device,
            synced_device_path,
            sync_error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_epub(epub_path: &Path) {
        use std::io::Cursor;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();

            zip.start_file("mimetype", opts).unwrap();
            std::io::Write::write_all(&mut zip, b"application/epub+zip").unwrap();

            zip.start_file("META-INF/container.xml", opts).unwrap();
            std::io::Write::write_all(
                &mut zip,
                br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .unwrap();

            zip.start_file("content.opf", opts).unwrap();
            std::io::Write::write_all(
                &mut zip,
                br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Vocabulary Test Book</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/></spine>
</package>"#,
            )
            .unwrap();

            zip.start_file("c1.xhtml", opts).unwrap();
            std::io::Write::write_all(&mut zip, br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>This is an abstruse and chimerical test for Word Wise vocabulary glosses.</p></body></html>"#).unwrap();

            zip.finish().unwrap();
        }

        std::fs::write(epub_path, buf.into_inner()).unwrap();
    }

    #[test]
    fn test_pipeline_nonexistent_file() {
        let options = ConversionOptions::default();
        let res = ConversionPipeline::run_with_log_path(
            Path::new("/nonexistent/fake.epub"),
            None,
            &options,
            None,
            None::<fn(f32, &str)>,
        );
        assert!(res.is_err());
    }

    #[test]
    fn test_conversion_options_default() {
        let opts = ConversionOptions::default();
        assert_eq!(opts.content_type, "EBOK");
        assert_eq!(opts.resolved_content_type(), crate::ir::ContentType::Ebok);
        assert!(opts.word_wise);
        assert!(opts.auto_deploy);
        assert!(opts.device_root.is_none());

        let pdoc_opts =
            ConversionOptions::default().with_content_type(crate::ir::ContentType::Pdoc);
        assert_eq!(pdoc_opts.content_type, "PDOC");
        assert_eq!(
            pdoc_opts.resolved_content_type(),
            crate::ir::ContentType::Pdoc
        );

        let sanitized_opts = ConversionOptions {
            content_type: "../../evil".to_string(),
            ..Default::default()
        };
        assert_eq!(
            sanitized_opts.resolved_content_type(),
            crate::ir::ContentType::Ebok
        );
    }

    #[test]
    fn test_pipeline_generates_word_wise_sdr_sidecars() {
        let temp_dir = tempfile::tempdir().unwrap();
        let epub_path = temp_dir.path().join("test_book.epub");
        let kfx_path = temp_dir.path().join("test_book.kfx");
        let log_path = temp_dir.path().join("test.log");
        create_test_epub(&epub_path);

        let options = ConversionOptions {
            content_type: "EBOK".into(),
            word_wise: true,
            auto_deploy: false,
            device_root: None,
        };

        let report = ConversionPipeline::run_with_log_path(
            &epub_path,
            Some(&kfx_path),
            &options,
            Some(&log_path),
            None::<fn(f32, &str)>,
        )
        .expect("Pipeline should succeed");

        assert!(kfx_path.is_file(), "KFX file must be created");
        assert!(
            report.word_wise_glosses.is_some_and(|g| g > 0),
            "Should find vocabulary glosses"
        );

        // Verify .sdr directory and .kll database deployment
        let sdr_path = temp_dir.path().join("test_book.sdr");
        assert!(sdr_path.is_dir(), "Companion SDR directory must exist");

        let mut kll_count = 0;
        for entry in std::fs::read_dir(&sdr_path).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".kll") {
                kll_count += 1;
                let kll_data = std::fs::read(entry.path()).unwrap();
                assert!(
                    kll_data.starts_with(b"SQLite format 3\0"),
                    "KLL must be valid SQLite database"
                );
            }
        }
        assert!(
            kll_count >= 1,
            "At least one .kll file must exist in the SDR directory"
        );
    }

    #[test]
    fn test_conversion_log_double_rotation_overwrites_old_backup() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("conversion.log");
        let backup_path = temp_dir.path().join("conversion.log.old");

        // First oversized log (> 2 MiB)
        let first_payload = vec![b'A'; 2 * 1024 * 1024 + 16];
        std::fs::write(&log_path, &first_payload).unwrap();
        append_conversion_log_at(&log_path, "FIRST_ROTATED");

        assert!(backup_path.is_file());
        assert_eq!(std::fs::read(&backup_path).unwrap(), first_payload);
        let log_after_first = std::fs::read_to_string(&log_path).unwrap();
        assert!(log_after_first.contains("FIRST_ROTATED"));
        assert!(log_after_first.len() < 1024);

        // Second oversized log while conversion.log.old already exists
        let second_payload = vec![b'B'; 2 * 1024 * 1024 + 32];
        std::fs::write(&log_path, &second_payload).unwrap();
        append_conversion_log_at(&log_path, "SECOND_ROTATED");

        assert!(backup_path.is_file());
        assert_eq!(std::fs::read(&backup_path).unwrap(), second_payload);
        let log_after_second = std::fs::read_to_string(&log_path).unwrap();
        assert!(log_after_second.contains("SECOND_ROTATED"));
        assert!(!log_after_second.contains("FIRST_ROTATED"));
        assert!(log_after_second.len() < 1024);
    }

    #[test]
    fn test_conversion_log_rotation_fallback_truncates_when_rename_fails() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("conversion.log");
        let backup_blocker = temp_dir.path().join("conversion.log.old");

        // Blocking directory prevents remove_file/rename/copy on all platforms.
        std::fs::create_dir_all(&backup_blocker).unwrap();
        let oversized = vec![b'C'; 2 * 1024 * 1024 + 8];
        std::fs::write(&log_path, &oversized).unwrap();

        append_conversion_log_at(&log_path, "FALLBACK_FRESH_LINE");

        assert!(backup_blocker.is_dir());
        let active_log = std::fs::read_to_string(&log_path).unwrap();
        assert!(active_log.contains("FALLBACK_FRESH_LINE"));
        assert!(active_log.len() < 1024);
    }

    #[test]
    fn test_pipeline_run_with_log_path_isolation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let epub_path = temp_dir.path().join("book.epub");
        let kfx_path = temp_dir.path().join("book.kfx");
        create_test_epub(&epub_path);

        let options = ConversionOptions {
            content_type: "EBOK".into(),
            word_wise: true,
            auto_deploy: false,
            device_root: None,
        };

        // 1. Some(custom_log) emits START and DONE to custom_log only
        let custom_log = temp_dir.path().join("logs").join("custom.log");
        ConversionPipeline::run_with_log_path(
            &epub_path,
            Some(&kfx_path),
            &options,
            Some(&custom_log),
            None::<fn(f32, &str)>,
        )
        .expect("run_with_log_path(Some) should succeed");
        let log_content = std::fs::read_to_string(&custom_log).unwrap();
        assert!(log_content.contains("START "));
        assert!(log_content.contains("DONE "));

        // 2. None suppresses log file creation
        let no_log_dir = temp_dir.path().join("no_log_out");
        std::fs::create_dir_all(&no_log_dir).unwrap();
        let kfx_no_log = no_log_dir.join("silent.kfx");
        ConversionPipeline::run_with_log_path(
            &epub_path,
            Some(&kfx_no_log),
            &options,
            None,
            None::<fn(f32, &str)>,
        )
        .expect("run_with_log_path(None) should succeed");
        assert!(kfx_no_log.is_file());
        assert!(!no_log_dir.join("conversion.log").exists());
    }

    #[test]
    fn test_pipeline_relative_output_path_discovers_ancestor_kindle_root() {
        if std::env::var_os("KFX_PIPELINE_REL_CHILD").is_some() {
            // Case 1: auto_deploy: false, device_root: None must NOT silently sync
            let no_sync_options = ConversionOptions {
                content_type: "EBOK".into(),
                word_wise: false,
                auto_deploy: false,
                device_root: None,
            };
            let report_nosync = ConversionPipeline::run_with_log_path(
                Path::new("input.epub"),
                Some(Path::new("rel_book_nosync.kfx")),
                &no_sync_options,
                Some(Path::new("../test_pipeline_nosync.log")),
                None::<fn(f32, &str)>,
            )
            .expect("Relative pipeline run with auto_deploy=false must succeed");
            assert!(
                !report_nosync.synced_to_device,
                "auto_deploy=false must NOT trigger ancestor Kindle sync"
            );
            assert!(report_nosync.synced_device_path.is_none());
            assert!(Path::new("rel_book_nosync.kfx").is_file());
            assert!(Path::new("rel_book_nosync.sdr").is_dir());
            assert!(
                !Path::new("../amazon-cover-bug").exists(),
                "amazon-cover-bug must not be created when auto_deploy=false"
            );

            // Case 2: auto_deploy: true, device_root: None discovers ancestor Kindle root and syncs
            let sync_options = ConversionOptions {
                content_type: "EBOK".into(),
                word_wise: false,
                auto_deploy: true,
                device_root: None,
            };
            let report_sync = ConversionPipeline::run_with_log_path(
                Path::new("input.epub"),
                Some(Path::new("rel_book_sync.kfx")),
                &sync_options,
                Some(Path::new("../test_pipeline_sync.log")),
                None::<fn(f32, &str)>,
            )
            .expect("Relative pipeline run with auto_deploy=true must succeed");
            assert!(
                report_sync.synced_to_device,
                "Relative output inside Kindle documents/ with auto_deploy=true must sync"
            );
            assert!(report_sync.synced_device_path.is_some());
            assert!(Path::new("rel_book_sync.kfx").is_file());
            assert!(Path::new("rel_book_sync.sdr").is_dir());
            let sync_log = std::fs::read_to_string("../test_pipeline_sync.log").unwrap();
            assert!(
                sync_log.contains("SYNC ok synced_kfx_path="),
                "Expected synced_kfx_path= in sync log, got: {}",
                sync_log
            );
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let kindle_root = temp.path().join("mock_kindle");
        let docs_dir = kindle_root.join("documents");
        std::fs::create_dir_all(&docs_dir).unwrap();
        std::fs::create_dir_all(kindle_root.join("system")).unwrap();
        create_test_epub(&docs_dir.join("input.epub"));

        let exe = std::env::current_exe().unwrap();
        let output = std::process::Command::new(exe)
            .args([
                "--exact",
                "pipeline::tests::test_pipeline_relative_output_path_discovers_ancestor_kindle_root",
            ])
            .current_dir(&docs_dir)
            .env("KFX_PIPELINE_REL_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Pipeline relative child failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(docs_dir.join("rel_book_nosync.kfx").is_file());
        assert!(docs_dir.join("rel_book_nosync.sdr").is_dir());
        assert!(docs_dir.join("rel_book_sync.kfx").is_file());
        assert!(docs_dir.join("rel_book_sync.sdr").is_dir());
    }

    #[test]
    fn test_pipeline_sdr_deploy_failure_sets_sdr_error_not_word_wise_error() {
        let temp = tempfile::tempdir().unwrap();
        let epub_path = temp.path().join("sdr_err_book.epub");
        create_test_epub(&epub_path);

        for word_wise in [false, true] {
            let kfx_path = temp.path().join(format!("out_ww_{}.kfx", word_wise));
            let blocking_sdr = kfx_path.with_extension("sdr");
            std::fs::write(&blocking_sdr, b"blocking-file").unwrap();

            let options = ConversionOptions {
                content_type: "EBOK".into(),
                word_wise,
                auto_deploy: false,
                device_root: None,
            };
            let report = ConversionPipeline::run_with_log_path(
                &epub_path,
                Some(&kfx_path),
                &options,
                None,
                None::<fn(f32, &str)>,
            )
            .expect("Non-fatal SDR deploy failure must still return Ok(report)");

            assert!(
                report.sdr_error.is_some(),
                "sdr_error must be populated when SdrDeployer::deploy fails (word_wise={})",
                word_wise
            );
            assert!(
                report.word_wise_error.is_none(),
                "word_wise_error must remain None when only SDR deployment failed (word_wise={})",
                word_wise
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_conversion_log_rejects_symlinks_for_file_backup_and_parent() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();

        // 1. Symlinked conversion.log must not follow or clobber victim
        let victim = temp.path().join("victim.txt");
        std::fs::write(&victim, b"UNTOUCHED_LOG_VICTIM").unwrap();
        let sym_log = temp.path().join("conversion.log");
        symlink(&victim, &sym_log).unwrap();
        append_conversion_log_at(&sym_log, "SHOULD_NOT_WRITE");
        assert_eq!(std::fs::read(&victim).unwrap(), b"UNTOUCHED_LOG_VICTIM");

        // 2. Symlinked conversion.log.old during rotation must not overwrite victim
        let rot_dir = temp.path().join("rot_case");
        std::fs::create_dir_all(&rot_dir).unwrap();
        let rot_victim = temp.path().join("rot_victim.txt");
        std::fs::write(&rot_victim, b"UNTOUCHED_ROT_VICTIM").unwrap();
        let rot_log = rot_dir.join("conversion.log");
        let rot_backup_link = rot_dir.join("conversion.log.old");
        let oversized = vec![b'Z'; 2 * 1024 * 1024 + 32];
        std::fs::write(&rot_log, &oversized).unwrap();
        symlink(&rot_victim, &rot_backup_link).unwrap();

        append_conversion_log_at(&rot_log, "POST_SAFE_ROTATE");
        assert_eq!(std::fs::read(&rot_victim).unwrap(), b"UNTOUCHED_ROT_VICTIM");
        let after_rot = std::fs::read_to_string(&rot_log).unwrap();
        assert!(after_rot.contains("POST_SAFE_ROTATE"));
        assert!(
            !std::fs::symlink_metadata(&rot_backup_link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "Rotated backup should be a real file after safely unlinking the symlink"
        );

        // 3. Symlinked parent directory must be refused without creating log inside target
        let target_dir = temp.path().join("real_target_dir");
        std::fs::create_dir_all(&target_dir).unwrap();
        let sym_parent = temp.path().join("sym_parent_dir");
        symlink(&target_dir, &sym_parent).unwrap();
        append_conversion_log_at(&sym_parent.join("conversion.log"), "NO_ESCAPE");
        assert!(!target_dir.join("conversion.log").exists());
    }

    #[test]
    fn test_is_english_language_tag_variants() {
        for tag in ["", "  ", "en", "EN", "en-US", "en_GB", "eng", "ENG-US"] {
            assert!(
                is_english_language_tag(tag),
                "Expected English tag {:?} to return true",
                tag
            );
        }
        for tag in [
            "ko", "ko-KR", "ja", "zh-Hans", "fr", "de-DE", "es", "enigma",
        ] {
            assert!(
                !is_english_language_tag(tag),
                "Expected non-English tag {:?} to return false",
                tag
            );
        }
    }

    #[test]
    fn test_pipeline_skips_double_sdr_deploy_when_output_is_on_device() {
        let temp = tempfile::tempdir().unwrap();
        let kindle_root = temp.path().join("mock_kindle");
        let docs_dir = kindle_root.join("documents");
        std::fs::create_dir_all(&docs_dir).unwrap();
        std::fs::create_dir_all(kindle_root.join("system")).unwrap();

        let epub_path = temp.path().join("book.epub");
        create_test_epub(&epub_path);
        let output_kfx = docs_dir.join("book.kfx");
        let log_path = temp.path().join("single_sdr.log");

        let options = ConversionOptions {
            content_type: "EBOK".into(),
            word_wise: true,
            auto_deploy: true,
            device_root: Some(kindle_root.clone()),
        };

        let report = ConversionPipeline::run_with_log_path(
            &epub_path,
            Some(&output_kfx),
            &options,
            Some(&log_path),
            None::<fn(f32, &str)>,
        )
        .expect("pipeline targeting Kindle documents/ must succeed");

        assert!(report.synced_to_device);
        assert_eq!(report.synced_device_path, Some(output_kfx.clone()));
        assert!(docs_dir.join("book.sdr").is_dir());

        let log_text = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            log_text.contains("SDR deferred to device sync"),
            "Expected Step 4b to defer .sdr deployment to Step 5, got:\n{}",
            log_text
        );
        assert!(
            !log_text.contains("SDR deployed path="),
            "Step 4b must NOT deploy .sdr a second time when Step 5 targets the same .sdr:\n{}",
            log_text
        );
        assert!(log_text.contains("SYNC ok synced_kfx_path="));
    }
}
