//! CLI interface for the Rust KFX Converter (`kfx-conv`)

use clap::Parser;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "UPPER")]
enum CliContentType {
    Ebok,
    Pdoc,
}

impl CliContentType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ebok => "EBOK",
            Self::Pdoc => "PDOC",
        }
    }
}

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
    #[arg(
        short = 'w',
        long = "word-wise",
        num_args = 0..=1,
        default_value_t = true,
        default_missing_value = "true",
        require_equals = true,
        action = clap::ArgAction::Set
    )]
    word_wise: bool,

    /// Kindle content definition environment type (EBOK for books/lockscreen, PDOC for personal documents) [default: EBOK]
    #[arg(
        short = 'c',
        long = "content-type",
        value_enum,
        ignore_case = true,
        default_value_t = CliContentType::Ebok
    )]
    content_type: CliContentType,
}

/// Folds space-separated `-w true|false` and `--word-wise true|false` pairs into `=...`
/// before Clap parsing while leaving `-w book.epub` and arguments after `--` untouched.
fn normalize_cli_args<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let raw: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0usize;
    let mut after_double_dash = false;

    while i < raw.len() {
        let cur = &raw[i];
        if i == 0 || after_double_dash {
            out.push(cur.clone());
            i += 1;
            continue;
        }
        if cur == "--" {
            after_double_dash = true;
            out.push(cur.clone());
            i += 1;
            continue;
        }

        if let Some(flag_str) = cur.to_str()
            && (flag_str == "-w" || flag_str == "--word-wise")
            && let Some(next) = raw.get(i + 1).and_then(|s| s.to_str())
            && (next.eq_ignore_ascii_case("true") || next.eq_ignore_ascii_case("false"))
        {
            let mut combined = OsString::from(flag_str);
            combined.push("=");
            combined.push(next.to_ascii_lowercase());
            out.push(combined);
            i += 2;
            continue;
        }

        out.push(cur.clone());
        i += 1;
    }

    out
}

fn main() {
    let args = Args::parse_from(normalize_cli_args(std::env::args_os()));

    println!("=== Rust KFX Converter v0.1.0 ===");
    println!("Target: {:?}", args.input);

    if args.inspect {
        match kfx_converter::KfxContainer::open(&args.input) {
            Ok(container) => {
                println!("Valid KFX Container:");
                println!("  Version: {}", container.header.version);
                println!("  Header Length: {} bytes", container.header.header_length);
                println!(
                    "  ContainerInfo Offset: {} bytes",
                    container.header.info_offset
                );
                println!(
                    "  ContainerInfo Length: {} bytes",
                    container.header.info_length
                );
                println!("  Total Chunks: {}", container.chunks.len());
                for (i, c) in container.chunks.iter().take(10).enumerate() {
                    let type_name =
                        kfx_converter::symbol_name(c.chunk_type as u64).unwrap_or("unknown");
                    println!(
                        "    [{}] ID: {}, Type: {} ($), Offset: {}, Length: {} bytes",
                        i, c.entity_id, type_name, c.offset, c.length
                    );
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
            content_type: args.content_type.as_str().to_string(),
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
                if let Some(err) = &report.word_wise_error {
                    eprintln!("Warning: Word Wise generation failed: {err}");
                } else if let Some(gloss_count) = report.word_wise_glosses {
                    println!(
                        "  [ENHANCEMENT] Word Wise sidecar generated ({} glosses)",
                        gloss_count
                    );
                }
                if report.synced_to_device {
                    if let Some(dest) = report.synced_device_path {
                        println!(
                            "-> [SYNC SUCCESS] Sideloaded to Kindle with lockscreen cover: {:?}",
                            dest
                        );
                    }
                } else if args.sync {
                    if let Some(err) = report.sync_error {
                        eprintln!("-> [SYNC WARNING] Failed to transfer to device: {}", err);
                    } else if let Some(usb) =
                        kfx_converter::KindleSync::probe_unmounted_usb_kindle()
                    {
                        eprintln!(
                            "-> [SYNC NOTICE] Amazon Kindle USB hardware (1949:{}, '{}') detected, but no active filesystem mount was found.",
                            usb.product_id, usb.product_name
                        );
                        eprintln!(
                            "-> [TIP] In headless Linux or Docker, mount with 'jmtpfs /media/Kindle' or specify 'KINDLE_ROOT=/path/to/mount'."
                        );
                    } else {
                        println!("-> [SYNC NOTICE] No Kindle device detected (USB / MTP).");
                    }
                } else if let Some(root) = kfx_converter::KindleSync::detect_kindle_root() {
                    println!(
                        "-> [TIP] Connected Kindle detected at {:?}. Use '--sync' ('-s') to automatically install with lockscreen cover thumbnails.",
                        root
                    );
                }
            }
            Err(e) => {
                eprintln!("[ERROR] Conversion failed: {}", e);
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_cli_args_word_wise_and_double_dash() {
        // 1. `-w book.epub` leaves book.epub untouched as positional input
        let args = Args::try_parse_from(normalize_cli_args(["kfx-conv", "-w", "book.epub"]))
            .expect("-w book.epub should parse cleanly");
        assert!(args.word_wise);
        assert_eq!(args.input, PathBuf::from("book.epub"));
        assert_eq!(args.content_type, CliContentType::Ebok);

        // 2. `--word-wise false book.epub` folds into `--word-wise=false`
        let args_false = Args::try_parse_from(normalize_cli_args([
            "kfx-conv",
            "--word-wise",
            "false",
            "book.epub",
        ]))
        .expect("--word-wise false should parse cleanly");
        assert!(!args_false.word_wise);
        assert_eq!(args_false.input, PathBuf::from("book.epub"));

        // 3. `-w FALSE` case-insensitive folding
        let args_short_false =
            Args::try_parse_from(normalize_cli_args(["kfx-conv", "book.epub", "-w", "FALSE"]))
                .expect("-w FALSE should parse cleanly");
        assert!(!args_short_false.word_wise);
        assert_eq!(args_short_false.input, PathBuf::from("book.epub"));

        // 4. `--word-wise=false` explicit equals form
        let args_eq = Args::try_parse_from(normalize_cli_args([
            "kfx-conv",
            "book.epub",
            "--word-wise=false",
        ]))
        .expect("--word-wise=false should parse cleanly");
        assert!(!args_eq.word_wise);

        // 5. Stops normalization at `--`
        let norm_dash = normalize_cli_args(["kfx-conv", "--", "-w", "false"]);
        assert_eq!(
            norm_dash,
            vec![
                OsString::from("kfx-conv"),
                OsString::from("--"),
                OsString::from("-w"),
                OsString::from("false")
            ]
        );
    }

    #[test]
    fn test_cli_content_type_validation() {
        let pdoc =
            Args::try_parse_from(normalize_cli_args(["kfx-conv", "book.epub", "-c", "pdoc"]))
                .expect("-c pdoc should parse cleanly");
        assert_eq!(pdoc.content_type, CliContentType::Pdoc);
        assert_eq!(pdoc.content_type.as_str(), "PDOC");

        let ebok = Args::try_parse_from(normalize_cli_args([
            "kfx-conv",
            "book.epub",
            "--content-type",
            "EBOK",
        ]))
        .expect("--content-type EBOK should parse cleanly");
        assert_eq!(ebok.content_type, CliContentType::Ebok);
        assert_eq!(ebok.content_type.as_str(), "EBOK");

        assert!(
            Args::try_parse_from(normalize_cli_args([
                "kfx-conv",
                "book.epub",
                "-c",
                "INVALID",
            ]))
            .is_err(),
            "Invalid content type must fail at parse time"
        );
    }
}
