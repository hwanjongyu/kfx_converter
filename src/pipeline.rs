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

    std::env::temp_dir()
        .join("kfx-converter")
        .join("conversion.log")
}

/// Rotates `path` to `log.old` when it exceeds `max_bytes`, falling back to
/// copy + truncate/remove if `rename` fails (e.g., locked or cross-filesystem targets).
fn rotate_log_if_needed(path: &Path, max_bytes: u64) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() <= max_bytes {
        return;
    }

    let backup_path = path.with_extension("log.old");
    if let Err(err) = std::fs::remove_file(&backup_path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        // Best-effort: fall through to rename and copy+truncate fallback.
    }

    if std::fs::rename(path, &backup_path).is_err() {
        let _ = std::fs::copy(path, &backup_path);
        if std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .is_err()
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Appends a timestamped log line to `path` with non-fatal rotation.
fn append_conversion_log_at(path: &Path, message: &str) {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        let _ = std::fs::create_dir_all(parent);
    }
    rotate_log_if_needed(path, 2 * 1024 * 1024);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
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
            content_type: "EBOK".to_string(),
            word_wise: true,
            auto_deploy: true,
            device_root: None,
        }
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
        let mut packager = KfxPackager::new(&ir, &options.content_type);
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
                let fn_opt = std::path::Path::new(&target_id)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string());
                let fn_str = fn_opt.as_deref()?;
                let mut keys: Vec<&String> = ir.resources.keys().collect();
                keys.sort();
                keys.into_iter().find_map(|k| {
                    if k == fn_str || k.ends_with(fn_str) {
                        ir.resources.get(k)
                    } else {
                        None
                    }
                })
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
                options.content_type,
                options.auto_deploy,
                ir.metadata.language
            ),
        );

        if options.word_wise {
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

        // 4b. Always deploy companion .sdr directory and sidecars alongside output_path
        let sdr_path = output_path.with_extension("sdr");
        if let Err(e) = crate::enhancements::SdrDeployer::deploy(
            &sdr_path,
            &asin,
            Some(&content_id),
            Some(&options.content_type),
            Some(&ir.metadata.language),
            cover_jpeg.as_deref(),
            kll_bytes.as_deref(),
        ) {
            let msg = format!("SDR sidecar deploy failed ({}): {}", sdr_path.display(), e);
            log_pipeline_event(log_path, &format!("ERROR {}", msg));
            if word_wise_error.is_none() {
                word_wise_error = Some(msg);
            }
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

        // 5. Automatic Device Sync if enabled or if destination is inside a Kindle drive
        let mut synced_to_device = false;
        let mut synced_device_path = None;
        let mut sync_error = None;

        let target_root = options
            .device_root
            .clone()
            .or_else(|| {
                // Check if output_path is located inside a Kindle directory structure
                let effective_output = if output_path.is_absolute() {
                    output_path.clone()
                } else {
                    std::env::current_dir()
                        .map(|cwd| cwd.join(&output_path))
                        .unwrap_or_else(|_| output_path.clone())
                };
                let mut current = effective_output
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty());
                while let Some(parent) = current {
                    if parent.join("documents").is_dir()
                        && (parent.join("system").is_dir() || parent.join(".system").is_dir())
                    {
                        return Some(parent.to_path_buf());
                    }
                    current = parent.parent().filter(|p| !p.as_os_str().is_empty());
                }
                None
            })
            .or_else(|| {
                if options.auto_deploy {
                    KindleSync::detect_kindle_root()
                } else {
                    None
                }
            });

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
                Some(&options.content_type),
                Some(&ir.metadata.language),
                cover_jpeg.as_deref(),
                kll_bytes.as_deref(),
            ) {
                Ok(sdr_path) => {
                    log_pipeline_event(
                        log_path,
                        &format!(
                            "SYNC ok sdr_path=\"{}\" has_kll={}",
                            sdr_path.display(),
                            kll_bytes.is_some()
                        ),
                    );
                    synced_to_device = true;
                    synced_device_path = Some(sdr_path);
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
        assert!(opts.word_wise);
        assert!(opts.auto_deploy);
        assert!(opts.device_root.is_none());
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
            let options = ConversionOptions {
                content_type: "EBOK".into(),
                word_wise: false,
                auto_deploy: false,
                device_root: None,
            };
            let report = ConversionPipeline::run_with_log_path(
                Path::new("input.epub"),
                Some(Path::new("rel_book.kfx")),
                &options,
                Some(Path::new("../test_pipeline.log")),
                None::<fn(f32, &str)>,
            )
            .expect("Relative pipeline run inside Kindle documents/ must succeed");
            assert!(
                report.synced_to_device,
                "Relative output inside Kindle documents/ must trigger ancestor Kindle root discovery"
            );
            assert!(Path::new("rel_book.kfx").is_file());
            assert!(Path::new("rel_book.sdr").is_dir());
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
        assert!(docs_dir.join("rel_book.kfx").is_file());
        assert!(docs_dir.join("rel_book.sdr").is_dir());
    }
}
