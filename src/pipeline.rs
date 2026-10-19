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
use crate::packager::{self, KfxPackager, PackagerError};
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
        let ir = EpubReader::read_from_file(&input_path)?;
        let book_title = ir.metadata.title.clone();
        let creators = ir.metadata.creators.clone();
        let sections_count = ir.sections.len();
        let resources_count = ir.resources.len();

        // 2. Package into Amazon KFX container
        if let Some(ref cb) = progress {
            cb(0.3, "Serializing Yellowjacket KFX container...");
        }
        let mut packager = KfxPackager::new(&ir, &options.content_type);
        let seed = if !ir.metadata.identifier.is_empty() {
            &ir.metadata.identifier
        } else {
            &ir.metadata.title
        };
        let content_id = packager::generate_content_id(seed);
        let container_id = packager::generate_container_id(seed);

        let total_bytes = packager.package_to_file(&output_path)?;

        // 3. Normalize Cover Thumbnail
        if let Some(ref cb) = progress {
            cb(0.6, "Normalizing baseline cover image...");
        }
        let mut cover_jpeg = None;
        let mut cover_optimized = false;
        if let Some(cover_id) = &ir.metadata.cover_image_id {
            if let Some(asset) = ir.resources.get(cover_id) {
                if let Ok(optimized) = CoverOptimizer::optimize_thumbnail(&asset.data) {
                    cover_jpeg = Some(optimized);
                    cover_optimized = true;
                }
            }
        }

        // 4. Generate Word Wise Sidecar (.kll) if enabled
        let mut kll_bytes = None;
        let mut word_wise_glosses = None;
        if options.word_wise {
            if let Some(ref cb) = progress {
                cb(0.75, "Generating Word Wise lemma sidecar database...");
            }
            let mut ww_builder = WordWiseBuilder::new(
                &content_id,
                &container_id,
                "en",
                "en",
                "1",
            );
            let gloss_count = ww_builder.generate_from_book(&ir);
            word_wise_glosses = Some(gloss_count);

            if let Ok(tmp_kll) = tempfile::NamedTempFile::new() {
                if ww_builder.write_to_file(tmp_kll.path()).is_ok() {
                    use std::io::Read;
                    let mut buf = Vec::new();
                    if let Ok(mut f) = tmp_kll.reopen() {
                        let _ = f.read_to_end(&mut buf);
                        kll_bytes = Some(buf);
                    }
                }
            }
        }

        // 5. Automatic Device Sync if enabled
        let mut synced_to_device = false;
        let mut synced_device_path = None;
        let mut sync_error = None;

        if options.auto_deploy {
            if let Some(ref cb) = progress {
                cb(0.9, "Synchronizing to Kindle storage...");
            }
            let target_root = options
                .device_root
                .clone()
                .or_else(KindleSync::detect_kindle_root);

            if let Some(device_root) = target_root {
                match KindleSync::sync_to_device(
                    &device_root,
                    &output_path,
                    &ir.metadata.title,
                    &content_id,
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
}
