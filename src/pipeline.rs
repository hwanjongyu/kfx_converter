//! Unified Conversion Pipeline Service
//!
//! Provides a single, clean facade for converting EPUB documents into Amazon KFX containers,
//! generating companion Word Wise SQLite sidecars, normalizing baseline covers, and deploying
//! to physical Kindle hardware via USB Mass Storage (UMS) or MTP.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thiserror::Error;

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
        ir.fragment_large_sections(
            crate::ir::DEFAULT_MAX_SECTION_CHARS,
            crate::ir::DEFAULT_MAX_SECTION_BLOCKS,
        );
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
                ir.resources.iter().find_map(|(k, v)| {
                    if let Some(ref fn_str) = fn_opt
                        && (k == fn_str || k.ends_with(fn_str)) {
                            return Some(v);
                        }
                    None
                })
            });
            if let Some(asset) = asset
                && let Ok(optimized) = CoverOptimizer::optimize_thumbnail(&asset.data) {
                    cover_jpeg = Some(optimized);
                    cover_optimized = true;
                }
        }

        // 4. Generate Word Wise Sidecar (.kll) if enabled
        let mut kll_bytes = None;
        let mut word_wise_glosses = None;
        if options.word_wise {
            if let Some(ref cb) = progress {
                cb(0.75, "Generating Word Wise lemma sidecar database...");
            }
            let book_revision = content_id[..8].to_lowercase();
            let mut ww_builder = WordWiseBuilder::new(
                &asin,
                &container_id,
                "en",
                "en",
                &book_revision,
            );
            let gloss_count = ww_builder.generate_from_book(&ir);
            word_wise_glosses = Some(gloss_count);

            if let Ok(bytes) = ww_builder.to_bytes() {
                kll_bytes = Some(bytes);
            }

            // Always deploy companion .sdr directory and .kll database alongside output_path
            let sdr_path = output_path.with_extension("sdr");
            if std::fs::create_dir_all(&sdr_path).is_ok() {
                let safe_asin = KindleSync::sanitize_asin(&asin);
                let safe_cid = KindleSync::sanitize_asin(&content_id);

                if let Some(ref kll) = kll_bytes {
                    let kll_path = sdr_path.join(format!("LanguageLayer.en.{}.kll", safe_asin));
                    let _ = std::fs::write(&kll_path, kll);
                    // Also write 10-char canonical ASIN alias if safe_asin is 32-hex
                    if safe_asin.len() >= 9 {
                        let asin_10 = format!("B{}", &safe_asin[..9]);
                        if asin_10 != safe_asin {
                            let _ = std::fs::write(
                                sdr_path.join(format!("LanguageLayer.en.{}.kll", asin_10)),
                                kll,
                            );
                        }
                    }
                    // Also write fallback with content_id for dual-lookup compatibility
                    if safe_cid != safe_asin && !safe_cid.is_empty() {
                        let fallback_path =
                            sdr_path.join(format!("LanguageLayer.en.{}.kll", safe_cid));
                        let _ = std::fs::write(fallback_path, kll);
                    }
                }

                // Write cover thumbnail aliases directly into the .sdr folder
                if let Some(ref jpeg) = cover_jpeg {
                    let _ = std::fs::write(sdr_path.join("cover.jpg"), jpeg);
                    let _ = std::fs::write(
                        sdr_path.join(format!("thumbnail_{}_EBOK_portrait.jpg", safe_asin)),
                        jpeg,
                    );
                    let _ = std::fs::write(
                        sdr_path.join(format!("thumbnail_{}_PDOC_portrait.jpg", safe_asin)),
                        jpeg,
                    );
                    let _ = std::fs::write(
                        sdr_path.join(format!("thumbnail_{}_portrait.jpg", safe_asin)),
                        jpeg,
                    );
                    if safe_cid != safe_asin && !safe_cid.is_empty() {
                        let _ = std::fs::write(
                            sdr_path.join(format!("thumbnail_{}_EBOK_portrait.jpg", safe_cid)),
                            jpeg,
                        );
                        let _ = std::fs::write(
                            sdr_path.join(format!("thumbnail_{}_PDOC_portrait.jpg", safe_cid)),
                            jpeg,
                        );
                        let _ = std::fs::write(
                            sdr_path.join(format!("thumbnail_{}_portrait.jpg", safe_cid)),
                            jpeg,
                        );
                    }
                }
            }
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
                let mut current = output_path.parent();
                while let Some(parent) = current {
                    if parent.join("documents").is_dir()
                        && (parent.join("system").is_dir() || parent.join(".system").is_dir())
                    {
                        return Some(parent.to_path_buf());
                    }
                    current = parent.parent();
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
                cover_jpeg.as_deref(),
                kll_bytes.as_deref(),
            ) {
                Ok(sdr_path) => {
                    synced_to_device = true;
                    synced_device_path = Some(sdr_path);
                }
                Err(e) => {
                    sync_error = Some(e.to_string());
                }
            }
        }

        if let Some(ref cb) = progress {
            cb(1.0, "Conversion completed successfully.");
        }

        Ok(ConversionReport {
            input_path,
            output_path,
            book_title,
            creators,
            sections_count,
            resources_count,
            total_bytes,
            duration: start_time.elapsed(),
            cover_optimized,
            word_wise_glosses,
            synced_to_device,
            synced_device_path,
            sync_error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_nonexistent_file() {
        let options = ConversionOptions::default();
        let res = ConversionPipeline::run(
            Path::new("/nonexistent/fake.epub"),
            None,
            &options,
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
        use std::io::Cursor;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let temp_dir = tempfile::tempdir().unwrap();
        let epub_path = temp_dir.path().join("test_book.epub");
        let kfx_path = temp_dir.path().join("test_book.kfx");

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();

            zip.start_file("mimetype", opts).unwrap();
            std::io::Write::write_all(&mut zip, b"application/epub+zip").unwrap();

            zip.start_file("META-INF/container.xml", opts).unwrap();
            std::io::Write::write_all(&mut zip, br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#).unwrap();

            zip.start_file("content.opf", opts).unwrap();
            std::io::Write::write_all(&mut zip, br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Vocabulary Test Book</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/></spine>
</package>"#).unwrap();

            zip.start_file("c1.xhtml", opts).unwrap();
            std::io::Write::write_all(&mut zip, br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>This is an abstruse and chimerical test for Word Wise vocabulary glosses.</p></body></html>"#).unwrap();

            zip.finish().unwrap();
        }

        std::fs::write(&epub_path, buf.into_inner()).unwrap();

        let options = ConversionOptions {
            content_type: "EBOK".into(),
            word_wise: true,
            auto_deploy: false,
            device_root: None,
        };

        let report = ConversionPipeline::run(
            &epub_path,
            Some(&kfx_path),
            &options,
            None::<fn(f32, &str)>,
        ).expect("Pipeline should succeed");

        assert!(kfx_path.is_file(), "KFX file must be created");
        assert!(report.word_wise_glosses.is_some_and(|g| g > 0), "Should find vocabulary glosses");

        // Verify .sdr directory and .kll database deployment
        let sdr_path = temp_dir.path().join("test_book.sdr");
        assert!(sdr_path.is_dir(), "Companion SDR directory must exist");

        let mut kll_count = 0;
        for entry in std::fs::read_dir(&sdr_path).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".kll") {
                kll_count += 1;
                let kll_data = std::fs::read(entry.path()).unwrap();
                assert!(kll_data.starts_with(b"SQLite format 3\0"), "KLL must be valid SQLite database");
            }
        }
        assert!(kll_count >= 1, "At least one .kll file must exist in the SDR directory");
    }
}
