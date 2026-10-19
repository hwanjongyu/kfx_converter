//! CLI interface for the Rust KFX Converter (`kfx-conv`)

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "kfx-conv")]
#[command(author = "hwanjongyu")]
#[command(version = "0.1.0")]
#[command(about = "High-performance native EPUB to Amazon KFX converter", long_about = None)]
struct Args {
    /// Input EPUB or KFX file path
    #[arg(required = true)]
    input: PathBuf,

    /// Output destination path
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Inspect / dump KFX container metadata and chunks
    #[arg(short, long)]
    inspect: bool,

    /// Automatically synchronize converted book to connected Kindle device
    #[arg(short, long)]
    sync: bool,

    /// Generate Word Wise sidecar (.kll)
    #[arg(short, long)]
    word_wise: bool,
}

fn main() {
    let args = Args::parse();

    println!("=== Rust KFX Converter v0.1.0 ===");
    println!("Target: {:?}", args.input);

    if args.inspect {
        match kfx_converter::KfxContainer::open(&args.input) {
            Ok(container) => {
                println!("Valid KFX Container:");
                println!("  Version: {}", container.header.version);
                println!("  Header Length: {} bytes", container.header.header_length);
                println!("  ContainerInfo Offset: {} bytes", container.header.info_offset);
                println!("  ContainerInfo Length: {} bytes", container.header.info_length);
                println!("  Total Chunks: {}", container.chunks.len());
                for (i, c) in container.chunks.iter().take(10).enumerate() {
                    let type_name = kfx_converter::symbol_name(c.chunk_type as u64).unwrap_or("unknown");
                    println!("    [{}] ID: {}, Type: {} ($), Offset: {}, Length: {} bytes",
                        i, c.entity_id, type_name, c.offset, c.length);
                }
                if container.chunks.len() > 10 {
                    println!("    ... and {} more chunks", container.chunks.len() - 10);
                }
            }
            Err(e) => {
                eprintln!("[ERROR] Failed to inspect KFX container: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        let output_path = match args.output {
            Some(p) => p,
            None => {
                let mut out = args.input.clone();
                out.set_extension("kfx");
                out
            }
        };

        println!("-> Converting EPUB to KFX...");
        let start = std::time::Instant::now();

        let ir = match kfx_converter::EpubReader::read_from_file(&args.input) {
            Ok(book) => {
                println!("  [EPUB] Title: '{}', Author(s): '{}', Sections: {}, Resources: {}",
                    book.metadata.title,
                    book.metadata.creators.join(", "),
                    book.sections.len(),
                    book.resources.len());
                book
            }
            Err(e) => {
                eprintln!("[ERROR] Failed to read EPUB: {}", e);
                std::process::exit(1);
            }
        };

        let mut packager = kfx_converter::KfxPackager::new(&ir, "EBOK");
        let content_id = kfx_converter::packager::generate_content_id(
            if !ir.metadata.identifier.is_empty() {
                &ir.metadata.identifier
            } else {
                &ir.metadata.title
            },
        );
        let container_id = kfx_converter::packager::generate_container_id(
            if !ir.metadata.identifier.is_empty() {
                &ir.metadata.identifier
            } else {
                &ir.metadata.title
            },
        );

        match packager.package_to_file(&output_path) {
            Ok(total_bytes) => {
                let elapsed = start.elapsed();
                println!("-> [SUCCESS] KFX Created: {:?} ({:.2} KB in {:.2?})",
                    output_path,
                    (total_bytes as f64) / 1024.0,
                    elapsed);

                // 1. Optimize Cover Thumbnail
                let mut cover_jpeg = None;
                if let Some(cover_id) = &ir.metadata.cover_image_id {
                    if let Some(asset) = ir.resources.get(cover_id) {
                        if let Ok(optimized) = kfx_converter::CoverOptimizer::optimize_thumbnail(&asset.data) {
                            println!("  [ENHANCEMENT] Cover normalized to baseline JPEG ({} bytes)", optimized.len());
                            cover_jpeg = Some(optimized);
                        }
                    }
                }

                // 2. Generate Word Wise Sidecar (.kll) if requested
                let mut kll_bytes = None;
                if args.word_wise {
                    let mut ww_builder = kfx_converter::WordWiseBuilder::new(
                        &content_id,
                        &container_id,
                        "en",
                        "en",
                        "1",
                    );
                    let gloss_count = ww_builder.generate_from_book(&ir);
                    let mut tmp_kll = tempfile::NamedTempFile::new().expect("Create tmp kll");
                    if ww_builder.write_to_file(tmp_kll.path()).is_ok() {
                        use std::io::Read;
                        let mut buf = Vec::new();
                        let _ = tmp_kll.read_to_end(&mut buf);
                        println!(
                            "  [ENHANCEMENT] Word Wise sidecar generated ({} glosses, {} bytes)",
                            gloss_count,
                            buf.len()
                        );
                        kll_bytes = Some(buf);
                    }
                }

                // 3. Kindle Device Synchronization
                if args.sync {
                    match kfx_converter::KindleSync::detect_kindle_root() {
                        Some(device_root) => {
                            println!("-> [SYNC] Detected Kindle at {:?}", device_root);
                            match kfx_converter::KindleSync::sync_to_device(
                                &device_root,
                                &output_path,
                                &ir.metadata.title,
                                &content_id,
                                cover_jpeg.as_deref(),
                                kll_bytes.as_deref(),
                            ) {
                                Ok(dest) => {
                                    println!("-> [SYNC SUCCESS] Sideloaded to Kindle: {:?}", dest);
                                }
                                Err(e) => {
                                    eprintln!("-> [SYNC WARNING] Failed to transfer to device: {}", e);
                                }
                            }
                        }
                        None => {
                            println!("-> [SYNC NOTICE] No Kindle device detected (USB / MTP).");
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[ERROR] Failed to package KFX: {}", e);
                std::process::exit(1);
            }
        }
    }
}
