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

    /// Generate Word Wise sidecar (.kll) [default: true]
    #[arg(short, long, default_value_t = true, action = clap::ArgAction::Set)]
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
        println!("-> Converting EPUB to KFX...");

        let options = kfx_converter::ConversionOptions {
            content_type: "EBOK".to_string(),
            word_wise: args.word_wise,
            auto_deploy: args.sync,
            device_root: None,
        };

        match kfx_converter::ConversionPipeline::run(
            &args.input,
            args.output.as_deref(),
            &options,
            Some(|pct: f32, msg: &str| {
                println!("  [{:.0}%] {}", pct * 100.0, msg);
            }),
        ) {
            Ok(report) => {
                println!(
                    "  [EPUB] Title: '{}', Author(s): '{}', Sections: {}, Resources: {}",
                    report.book_title,
                    report.creators.join(", "),
                    report.sections_count,
                    report.resources_count
                );
                println!(
                    "-> [SUCCESS] KFX Created: {:?} ({:.2} KB in {:.2?})",
                    report.output_path,
                    (report.total_bytes as f64) / 1024.0,
                    report.duration
                );
                if report.cover_optimized {
                    println!("  [ENHANCEMENT] Cover normalized to baseline JPEG");
                }
                if let Some(gloss_count) = report.word_wise_glosses {
                    println!("  [ENHANCEMENT] Word Wise sidecar generated ({} glosses)", gloss_count);
                }
                if args.sync {
                    if report.synced_to_device {
                        if let Some(dest) = report.synced_device_path {
                            println!("-> [SYNC SUCCESS] Sideloaded to Kindle: {:?}", dest);
                        }
                    } else if let Some(err) = report.sync_error {
                        eprintln!("-> [SYNC WARNING] Failed to transfer to device: {}", err);
                    } else {
                        println!("-> [SYNC NOTICE] No Kindle device detected (USB / MTP).");
                    }
                }
            }
            Err(e) => {
                eprintln!("[ERROR] Conversion failed: {}", e);
                std::process::exit(1);
            }
        }
    }
}
