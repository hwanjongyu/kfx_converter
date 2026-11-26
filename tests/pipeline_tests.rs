//! Integration tests for `ConversionPipeline` facade (`src/pipeline.rs`).

mod common;

use kfx_converter::{ConversionOptions, ConversionPipeline, KfxContainer, PipelineError};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn test_pipeline_end_to_end_with_cover_and_word_wise() {
    let temp = common::temp_dir();
    let epub_path = common::create_minimal_epub(
        temp.path(),
        "complete_book.epub",
        "Complete Test Volume",
        "en",
        true,
    );
    let kfx_path = temp.path().join("complete_book.kfx");
    let log_path = temp.path().join("pipeline.log");

    let progress_events = Arc::new(AtomicUsize::new(0));
    let progress_events_clone = Arc::clone(&progress_events);

    let last_pct = Arc::new(std::sync::Mutex::new(0.0f32));
    let last_pct_clone = Arc::clone(&last_pct);
    let options = ConversionOptions {
        content_type: "EBOK".to_string(),
        word_wise: true,
        auto_deploy: false,
        device_root: None,
    };

    let report = ConversionPipeline::run_with_log_path(
        &epub_path,
        Some(&kfx_path),
        &options,
        Some(&log_path),
        Some(|pct: f32, msg: &str| {
            let mut prev = last_pct_clone.lock().unwrap();
            assert!(
                pct >= *prev,
                "Progress must be monotonic: got {} after {}",
                pct,
                *prev
            );
            assert!(
                (0.0..=1.0).contains(&pct),
                "Progress must be between 0.0 and 1.0: got {}",
                pct
            );
            assert!(!msg.is_empty(), "Progress message must not be empty");
            *prev = pct;
            progress_events_clone.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .expect("Conversion pipeline must succeed");

    assert!(
        progress_events.load(Ordering::SeqCst) >= 5,
        "Should trigger multiple progress steps"
    );
    assert_eq!(report.book_title, "Complete Test Volume");
    assert_eq!(report.creators, vec!["Test Author".to_string()]);
    assert_eq!(
        report.sections_count, 2,
        "Cover section + chapter 1 should produce 2 sections"
    );
    assert!(report.resources_count >= 1);
    assert!(report.total_bytes > 0);
    assert!(report.cover_optimized, "Cover should be optimized");
    assert!(
        report.word_wise_glosses.is_some_and(|g| g > 0),
        "Word Wise glosses should be found for vocabulary words"
    );

    // Verify output KFX container opens and is valid
    let container = KfxContainer::open(&kfx_path).expect("Output KFX must be a valid container");
    assert_eq!(container.header.version, 2);
    assert!(!container.chunks.is_empty());

    // Verify companion .sdr folder and sidecars
    let sdr_path = temp.path().join("complete_book.sdr");
    assert!(sdr_path.is_dir(), "Companion .sdr folder must be created");

    let mut found_kll = false;
    for entry in std::fs::read_dir(&sdr_path).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".kll") {
            found_kll = true;
            let data = std::fs::read(entry.path()).unwrap();
            assert!(data.starts_with(b"SQLite format 3\0"));
        }
    }
    assert!(
        found_kll,
        "A .kll database sidecar must exist in the .sdr folder"
    );
}

#[test]
fn test_pipeline_default_output_path_resolution() {
    let temp = common::temp_dir();
    let epub_path = common::create_minimal_epub(
        temp.path(),
        "auto_named.epub",
        "Auto Named Book",
        "en",
        false,
    );

    let options = ConversionOptions {
        content_type: "EBOK".to_string(),
        word_wise: false,
        auto_deploy: false,
        device_root: None,
    };

    let log_path = temp.path().join("pipeline.log");
    let report = ConversionPipeline::run_with_log_path(
        &epub_path,
        None, // None specifies default `<input_path>.kfx`
        &options,
        Some(&log_path),
        None::<fn(f32, &str)>,
    )
    .expect("Pipeline with default output path must succeed");

    let expected_kfx = temp.path().join("auto_named.kfx");
    assert_eq!(report.output_path, expected_kfx);
    assert!(expected_kfx.is_file());
}

#[test]
fn test_pipeline_word_wise_disabled_skips_kll_emission() {
    let temp = common::temp_dir();
    let epub_path = common::create_minimal_epub(
        temp.path(),
        "without_ww.epub",
        "No Word Wise Book",
        "en",
        false,
    );
    let kfx_path = temp.path().join("without_ww.kfx");

    let options = ConversionOptions {
        content_type: "EBOK".to_string(),
        word_wise: false,
        auto_deploy: false,
        device_root: None,
    };

    let log_path = temp.path().join("pipeline.log");
    let report = ConversionPipeline::run_with_log_path(
        &epub_path,
        Some(&kfx_path),
        &options,
        Some(&log_path),
        None::<fn(f32, &str)>,
    )
    .expect("Pipeline should succeed");

    assert!(report.word_wise_glosses.is_none());

    let sdr_path = temp.path().join("without_ww.sdr");
    assert!(sdr_path.is_dir());

    for entry in std::fs::read_dir(&sdr_path).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.ends_with(".kll"),
            "No .kll file should exist when word_wise is disabled: found {}",
            name
        );
    }
}

#[test]
fn test_pipeline_non_english_book_conversion() {
    let temp = common::temp_dir();
    let korean_xhtml = r#"<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>제1장</title></head>
<body>
  <h1>제1장: 시작</h1>
  <p>안녕하세요. 이것은 한국어 전자책 본문입니다.</p>
  <p>고즈넉한 새벽 공기를 마시며 새로운 여정을 시작했습니다.</p>
</body>
</html>"#;
    let epub_path = common::create_epub_with_xhtml(
        temp.path(),
        "korean_book.epub",
        "한국어 전자책 테스트",
        "ko",
        false,
        korean_xhtml,
    );
    let kfx_path = temp.path().join("korean_book.kfx");

    let options = ConversionOptions {
        content_type: "PDOC".to_string(),
        word_wise: true, // Should gracefully handle non-English by producing 0 glosses
        auto_deploy: false,
        device_root: None,
    };

    let log_path = temp.path().join("pipeline.log");
    let report = ConversionPipeline::run_with_log_path(
        &epub_path,
        Some(&kfx_path),
        &options,
        Some(&log_path),
        None::<fn(f32, &str)>,
    )
    .expect("Non-English EPUB conversion should succeed");

    assert_eq!(report.book_title, "한국어 전자책 테스트");
    assert!(kfx_path.is_file());
    assert_eq!(report.word_wise_glosses, Some(0));
    assert!(
        report.word_wise_error.is_none(),
        "Zero glosses on non-English book must not be reported as an error: {:?}",
        report.word_wise_error
    );

    let sdr_path = temp.path().join("korean_book.sdr");
    assert!(sdr_path.is_dir(), "Companion .sdr folder must be created");
    let kll_files: Vec<String> = std::fs::read_dir(&sdr_path)
        .unwrap()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".kll") {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    assert!(
        kll_files.is_empty(),
        "Must emit 0 .kll files when gloss_count == 0, found: {:?}",
        kll_files
    );

    let log_text = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log_text.contains("INFO Word Wise skipped for non-English book"),
        "Conversion log must record non-English Word Wise skip: {}",
        log_text
    );
    assert!(
        !log_text.contains("WARN Word Wise enabled but 0 glosses matched"),
        "Non-English book must skip English dictionary scan without emitting 0-gloss warning: {}",
        log_text
    );
}

#[test]
fn test_pipeline_english_book_zero_glosses_logs_warning() {
    let temp = common::temp_dir();
    let simple_xhtml = r#"<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Simple</title></head>
<body>
  <p>The cat sat on the mat.</p>
</body>
</html>"#;
    let epub_path = common::create_epub_with_xhtml(
        temp.path(),
        "simple_english.epub",
        "Simple English Reader",
        "en",
        false,
        simple_xhtml,
    );
    let kfx_path = temp.path().join("simple_english.kfx");

    let options = ConversionOptions {
        content_type: "EBOK".to_string(),
        word_wise: true,
        auto_deploy: false,
        device_root: None,
    };

    let log_path = temp.path().join("pipeline.log");
    let report = ConversionPipeline::run_with_log_path(
        &epub_path,
        Some(&kfx_path),
        &options,
        Some(&log_path),
        None::<fn(f32, &str)>,
    )
    .expect("Simple English EPUB conversion should succeed");

    assert_eq!(report.word_wise_glosses, Some(0));
    assert!(report.word_wise_error.is_none());

    let sdr_path = temp.path().join("simple_english.sdr");
    assert!(sdr_path.is_dir());
    let has_kll = std::fs::read_dir(&sdr_path)
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().ends_with(".kll"));
    assert!(!has_kll, "0-gloss English book must not emit .kll files");

    let log_text = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log_text.contains("WARN Word Wise enabled but 0 glosses matched"),
        "English book with 0 matched glosses must still emit WARN log: {}",
        log_text
    );
}

#[test]
fn test_pipeline_device_subfolder_output_deploys_sdr_once_without_duplicate_copy() {
    let temp = common::temp_dir();
    let kindle_root = temp.path().join("mock_kindle");
    let docs_dir = kindle_root.join("documents");
    let author_dir = docs_dir.join("Author");
    std::fs::create_dir_all(&author_dir).unwrap();
    std::fs::create_dir_all(kindle_root.join("system")).unwrap();

    let epub_path = common::create_minimal_epub(
        temp.path(),
        "subfolder_novel.epub",
        "Subfolder Novel",
        "en",
        true,
    );
    let output_kfx = author_dir.join("subfolder_novel.kfx");
    let log_path = temp.path().join("subfolder_sync.log");

    let options = ConversionOptions {
        content_type: "EBOK".to_string(),
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
    .expect("Subfolder device conversion must succeed");

    assert!(report.synced_to_device);
    assert_eq!(report.synced_device_path, Some(output_kfx.clone()));
    assert!(output_kfx.is_file());
    assert!(author_dir.join("subfolder_novel.sdr").is_dir());
    assert!(
        !docs_dir.join("subfolder_novel.kfx").exists(),
        "In-place subfolder KFX must not create duplicate flat copy in documents/"
    );
    assert!(
        !docs_dir.join("subfolder_novel.sdr").exists(),
        "In-place subfolder SDR must not create duplicate flat .sdr in documents/"
    );

    let log_text = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log_text.contains("SDR deferred to device sync"),
        "Expected single .sdr deployment deferred to Step 5, got:\n{}",
        log_text
    );
    assert!(
        !log_text.contains("SDR deployed path="),
        "Step 4b must be skipped when Step 5 targets the same .sdr:\n{}",
        log_text
    );
    assert!(log_text.contains("SYNC ok synced_kfx_path="));
}

#[test]
fn test_pipeline_missing_input_file_returns_error() {
    let non_existent = std::path::Path::new("/missing/nonexistent/sample.epub");
    let options = ConversionOptions::default();

    let result = ConversionPipeline::run_with_log_path(
        non_existent,
        None,
        &options,
        None,
        None::<fn(f32, &str)>,
    );

    match result {
        Err(PipelineError::Other(msg)) => {
            assert!(msg.contains("Input file not found"));
        }
        res => panic!("Expected PipelineError::Other, got {:?}", res),
    }
}

#[test]
fn test_pipeline_empty_file_fails_without_kfx_residue() {
    let temp = common::temp_dir();
    let empty_epub = temp.path().join("empty.epub");
    std::fs::write(&empty_epub, b"").unwrap();
    let target_kfx = temp.path().join("empty.kfx");

    let log_path = temp.path().join("pipeline.log");
    let options = ConversionOptions::default();
    let result = ConversionPipeline::run_with_log_path(
        &empty_epub,
        Some(&target_kfx),
        &options,
        Some(&log_path),
        None::<fn(f32, &str)>,
    );

    assert!(result.is_err(), "Empty file must return error");
    assert!(
        !target_kfx.exists(),
        "Failed conversion must not leave behind partial KFX file"
    );
}

#[test]
fn test_pipeline_corrupted_zip_fails_cleanly() {
    let temp = common::temp_dir();
    let corrupt_epub = temp.path().join("corrupt.epub");
    std::fs::write(&corrupt_epub, b"PK\x03\x04INVALID_TRUNCATED_ZIP_DATA").unwrap();
    let target_kfx = temp.path().join("corrupt.kfx");

    let log_path = temp.path().join("pipeline.log");
    let options = ConversionOptions::default();
    let result = ConversionPipeline::run_with_log_path(
        &corrupt_epub,
        Some(&target_kfx),
        &options,
        Some(&log_path),
        None::<fn(f32, &str)>,
    );

    assert!(result.is_err(), "Corrupt zip must fail");
    match result {
        Err(PipelineError::Epub(_)) => {}
        res => panic!("Expected PipelineError::Epub, got {:?}", res),
    }
    assert!(!target_kfx.exists(), "Must not leave partial KFX residue");
}

#[test]
fn test_pipeline_missing_container_xml_fails_cleanly() {
    use std::io::Cursor;
    use zip::write::{SimpleFileOptions, ZipWriter};

    let temp = common::temp_dir();
    let invalid_epub = temp.path().join("no_container.epub");
    let target_kfx = temp.path().join("no_container.kfx");

    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default();
        zip.start_file("mimetype", opts).unwrap();
        std::io::Write::write_all(&mut zip, b"application/epub+zip").unwrap();
        // Omit META-INF/container.xml
        zip.finish().unwrap();
    }
    std::fs::write(&invalid_epub, buf.into_inner()).unwrap();

    let log_path = temp.path().join("pipeline.log");
    let options = ConversionOptions::default();
    let result = ConversionPipeline::run_with_log_path(
        &invalid_epub,
        Some(&target_kfx),
        &options,
        Some(&log_path),
        None::<fn(f32, &str)>,
    );

    assert!(result.is_err());
    assert!(!target_kfx.exists(), "Must not leave target file");
}

#[test]
fn test_pipeline_end_to_end_with_links_lists_and_footnote_popups() {
    use kfx_converter::{IonReader, IonValue, KfxEntity, KfxPreflightValidator, symbol_id};
    use std::io::{Cursor, Write};
    use zip::write::{SimpleFileOptions, ZipWriter};

    let temp = common::temp_dir();
    let epub_path = temp.path().join("linked_book.epub");
    let kfx_path = temp.path().join("linked_book.kfx");

    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default();
        zip.start_file("mimetype", opts).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/content.opf", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Linked Integration Volume</dc:title>
    <dc:language>en</dc:language>
    <dc:creator>Test Author</dc:creator>
  </metadata>
  <manifest>
    <item id="ch1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="Text/ch2.xhtml" media-type="application/xhtml+xml"/>
    <item id="notes" href="Notes/endnotes.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
    <itemref idref="notes"/>
  </spine>
</package>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/Text/ch1.xhtml", opts).unwrap();
        zip.write_all(
            br##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <h1 id="ch1_top">Chapter One</h1>
  <p>An abstruse and chimerical <a href="ch2.xhtml#ch2_mid">forward <b>link</b></a> with note<a id="fnref1" href="../Notes/endnotes.xhtml#fn1" epub:type="noteref">1</a> and <a href="https://example.com/ref?q=1">external</a> plus <a href="javascript:alert(1)">safe text</a>.</p>
  <ul>
    <li id="li_a"><p>List <a href="#ch1_top">back to top</a></p></li>
  </ul>
</body>
</html>"##,
        )
        .unwrap();
        zip.start_file("OEBPS/Text/ch2.xhtml", opts).unwrap();
        zip.write_all(
            br##"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <p id="ch2_mid">Second chapter <a href="ch1.xhtml#ch1_top">back</a> and <a href="missing.xhtml#bad">broken</a>.</p>
</body>
</html>"##,
        )
        .unwrap();
        zip.start_file("OEBPS/Notes/endnotes.xhtml", opts).unwrap();
        zip.write_all(
            br##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <section epub:type="footnotes" id="footnotes">
    <ol>
      <li id="fn1"><p>Ephemeral gloss for note one. <a href="../Text/ch1.xhtml#fnref1" role="doc-backlink">Back</a></p></li>
    </ol>
  </section>
</body>
</html>"##,
        )
        .unwrap();
        zip.finish().unwrap();
    }
    std::fs::write(&epub_path, buf.into_inner()).unwrap();

    let options = ConversionOptions {
        content_type: "EBOK".to_string(),
        word_wise: true,
        auto_deploy: false,
        device_root: None,
    };

    let log_path = temp.path().join("pipeline.log");
    let report = ConversionPipeline::run_with_log_path(
        &epub_path,
        Some(&kfx_path),
        &options,
        Some(&log_path),
        None::<fn(f32, &str)>,
    )
    .expect("Linked pipeline run must succeed");
    assert_eq!(report.sections_count, 3);
    assert!(report.total_bytes > 0);

    let mut container = KfxContainer::open(&kfx_path).expect("Output KFX must open cleanly");
    let sym_anchor = symbol_id("anchor").unwrap_or(266);
    let sym_aux = symbol_id("auxiliary_data").unwrap_or(597);
    let sym_style = symbol_id("style").unwrap_or(157);
    let sym_yj_note = symbol_id("yj.note").unwrap_or(617);

    assert!(
        container.chunks.iter().any(|c| c.chunk_type == sym_anchor),
        "Container must include $266 anchor chunks"
    );
    assert!(
        container
            .chunks
            .iter()
            .any(|c| c.chunk_type == sym_style && c.entity_id == 890),
        "Container must emit style_link (SID 890)"
    );

    let mut saw_yj_note = false;
    let mut entities = Vec::with_capacity(container.chunks.len());
    for chunk in container.chunks.clone() {
        let data = container.read_chunk(&chunk).unwrap();
        if chunk.chunk_type == sym_aux {
            let vals = IonReader::new(&data).decode_all().unwrap();
            if let IonValue::Struct(fields) = &vals[0]
                && fields.iter().any(|(k, _)| *k == sym_yj_note)
            {
                saw_yj_note = true;
            }
        }
        entities.push(KfxEntity {
            entity_id: chunk.entity_id,
            chunk_type: chunk.chunk_type,
            data,
        });
    }
    assert!(
        saw_yj_note,
        "Container must emit $617 yj.note popup metadata"
    );

    KfxPreflightValidator::new(&entities, None)
        .validate_container()
        .expect("Packaged KFX container must pass preflight validation");
}

#[test]
fn test_pipeline_p1_nov24_end_to_end_assets_collisions_utf16_list_images_and_nav() {
    use kfx_converter::{IonReader, IonValue, KfxEntity, KfxPreflightValidator, symbol_id};
    use std::io::{Cursor, Write};
    use zip::write::{SimpleFileOptions, ZipWriter};

    let temp = common::temp_dir();
    let epub_path = temp.path().join("p1_nov24_e2e.epub");
    let kfx_path = temp.path().join("p1_nov24_e2e.kfx");

    // Minimal valid GIF89a (64x48)
    let mut gif89 = b"GIF89a".to_vec();
    gif89.extend_from_slice(&64u16.to_le_bytes());
    gif89.extend_from_slice(&48u16.to_le_bytes());
    gif89.extend_from_slice(&[0x00, 0x00, 0x00, 0x3B]);

    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default();
        zip.start_file("mimetype", opts).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("META-INF/container.xml", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/content.opf", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>P1 End-to-End Verification Volume</dc:title>
    <dc:language>en</dc:language>
    <dc:creator>QA Suite</dc:creator>
    <meta name="cover" content="style_p"/>
  </metadata>
  <manifest>
    <item id="nav" href="Text/nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ch1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="Part2/CH2.XHTML" media-type="application/xhtml+xml"/>
    <item id="style_p" href="Images/empty_cover.jpg" media-type="image/jpeg"/>
    <item id="content_1" href="Images/anim.gif" media-type="image/gif"/>
    <item id="toc_nav" href="Images/vector.svg" media-type="image/svg+xml"/>
    <item id="page_list_nav" href="Fonts/book.ttf" media-type="font/ttf"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
  </spine>
</package>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/Text/nav.xhtml", opts).unwrap();
        zip.write_all(
            br##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <nav epub:type="toc">
    <ol>
      <li><a href="ch1.xhtml">Chapter One</a></li>
      <li><a href="../Part2/ch2.xhtml?v=1#sec%202">Chapter Two</a></li>
      <li><a href="javascript%3Aalert(1)">Blocked Nav</a></li>
    </ol>
  </nav>
</body>
</html>"##,
        )
        .unwrap();
        zip.start_file("OEBPS/Text/ch1.xhtml", opts).unwrap();
        zip.write_all(
            r##"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <h1>Chapter One</h1>
  <p>😀 <a href="../Part2/ch2.xhtml#sec%202">Forward</a> and <b>abstruse</b> <a href="javascript%3Aalert(1)">blocked</a> <a href="\\evil.example\share">unc</a>.</p>
  <ul>
    <li><p>Before list figure</p></li>
    <li><img src="../Images/anim.gif" alt="Animated diagram"/></li>
    <li><img src="../Images/vector.svg" alt="SVG fallback"/></li>
    <li><p>📖 To elucidate further.</p></li>
  </ul>
</body>
</html>"##
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("OEBPS/Part2/CH2.XHTML", opts).unwrap();
        zip.write_all(
            br#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <h1 id="sec 2">Chapter Two</h1>
  <p>An ephemeral conclusion.</p>
</body>
</html>"#,
        )
        .unwrap();
        // 0-byte JPEG cover (P1-#5), valid GIF89a (P1-#5), SVG & TTF (must be excluded)
        zip.start_file("OEBPS/Images/empty_cover.jpg", opts)
            .unwrap();
        zip.write_all(b"").unwrap();
        zip.start_file("OEBPS/Images/anim.gif", opts).unwrap();
        zip.write_all(&gif89).unwrap();
        zip.start_file("OEBPS/Images/vector.svg", opts).unwrap();
        zip.write_all(b"<svg xmlns='http://www.w3.org/2000/svg'/>")
            .unwrap();
        zip.start_file("OEBPS/Fonts/book.ttf", opts).unwrap();
        zip.write_all(b"\x00\x01\x00\x00FAKEFONT").unwrap();
        zip.finish().unwrap();
    }
    std::fs::write(&epub_path, buf.into_inner()).unwrap();

    let options = ConversionOptions {
        content_type: "EBOK".to_string(),
        word_wise: true,
        auto_deploy: false,
        device_root: None,
    };
    let log_path = temp.path().join("p1_e2e.log");
    let report = ConversionPipeline::run_with_log_path(
        &epub_path,
        Some(&kfx_path),
        &options,
        Some(&log_path),
        None::<fn(f32, &str)>,
    )
    .expect("End-to-end P1 pipeline run must succeed");

    assert!(report.total_bytes > 0);
    assert!(report.sdr_error.is_none());
    assert!(report.word_wise_error.is_none());
    assert!(
        report.word_wise_glosses.is_some_and(|g| g >= 2),
        "Expected Word Wise glosses for abstruse/elucidate/ephemeral, got {:?}",
        report.word_wise_glosses
    );

    let mut container = KfxContainer::open(&kfx_path).expect("Output KFX must open cleanly");
    let sym_ext_res = symbol_id("external_resource").unwrap_or(164);
    let sym_raw_media = symbol_id("bcRawMedia").unwrap_or(417);
    let sym_storyline = symbol_id("storyline").unwrap_or(259);
    let sym_content_list = symbol_id("content_list").unwrap_or(146);
    let sym_style_events = symbol_id("style_events").unwrap_or(142);
    let sym_offset = symbol_id("offset").unwrap_or(143);
    let sym_length = symbol_id("length").unwrap_or(144);
    let sym_type = symbol_id("type").unwrap_or(159);
    let sym_image = symbol_id("image").unwrap_or(271);
    let sym_format = symbol_id("format").unwrap_or(161);
    let sym_gif = symbol_id("gif").unwrap_or(286);
    let sym_w = symbol_id("resource_width").unwrap_or(422);
    let sym_h = symbol_id("resource_height").unwrap_or(423);

    let mut ext_res_count = 0usize;
    let mut raw_media_count = 0usize;
    let mut saw_gif_64x48 = false;
    let mut saw_list_271_image = false;
    let mut entities = Vec::with_capacity(container.chunks.len());

    for chunk in container.chunks.clone() {
        let data = container.read_chunk(&chunk).unwrap();
        if chunk.chunk_type == sym_ext_res {
            ext_res_count += 1;
            let vals = IonReader::new(&data).decode_all().unwrap();
            if let IonValue::Struct(fields) = &vals[0] {
                let fmt = fields
                    .iter()
                    .find(|(k, _)| *k == sym_format)
                    .map(|(_, v)| v);
                let w = fields.iter().find(|(k, _)| *k == sym_w).map(|(_, v)| v);
                let h = fields.iter().find(|(k, _)| *k == sym_h).map(|(_, v)| v);
                if fmt == Some(&IonValue::Symbol(sym_gif))
                    && w == Some(&IonValue::Int(64))
                    && h == Some(&IonValue::Int(48))
                {
                    saw_gif_64x48 = true;
                }
            }
        } else if chunk.chunk_type == sym_raw_media {
            raw_media_count += 1;
        } else if chunk.chunk_type == sym_storyline {
            let vals = IonReader::new(&data).decode_all().unwrap();
            if let IonValue::Struct(root) = &vals[0]
                && let Some((_, IonValue::List(elems))) =
                    root.iter().find(|(k, _)| *k == sym_content_list)
            {
                for elem in elems {
                    if let IonValue::Struct(ef) = elem {
                        if ef
                            .iter()
                            .any(|(k, v)| *k == sym_type && *v == IonValue::Symbol(sym_image))
                        {
                            saw_list_271_image = true;
                        }
                        if let Some((_, IonValue::List(events))) =
                            ef.iter().find(|(k, _)| *k == sym_style_events)
                        {
                            let mut prev = (i64::MIN, i64::MIN);
                            for ev in events {
                                if let IonValue::Struct(sf) = ev {
                                    let off = sf
                                        .iter()
                                        .find(|(k, _)| *k == sym_offset)
                                        .and_then(|(_, v)| match v {
                                            IonValue::Int(n) => Some(*n),
                                            _ => None,
                                        })
                                        .unwrap_or(0);
                                    let len = sf
                                        .iter()
                                        .find(|(k, _)| *k == sym_length)
                                        .and_then(|(_, v)| match v {
                                            IonValue::Int(n) => Some(*n),
                                            _ => None,
                                        })
                                        .unwrap_or(0);
                                    assert!(
                                        (off, len) >= prev,
                                        "$142 style_events must be sorted monotonically: {:?} < {:?}",
                                        (off, len),
                                        prev
                                    );
                                    prev = (off, len);
                                }
                            }
                        }
                    }
                }
            }
        }
        entities.push(KfxEntity {
            entity_id: chunk.entity_id,
            chunk_type: chunk.chunk_type,
            data,
        });
    }

    assert_eq!(
        ext_res_count, 2,
        "Only 0-byte cover placeholder and GIF89a should emit $164 (SVG & TTF excluded)"
    );
    assert_eq!(raw_media_count, 2);
    assert!(saw_gif_64x48, "GIF89a must emit $422=64, $423=48");
    assert!(saw_list_271_image, "List-nested GIF must emit $271 image");

    // Reconstruct LocalSymbols from the written KFX file's ContainerInfo -> doc_symbols ($ion_symbol_table)
    let raw_kfx = std::fs::read(&kfx_path).expect("Read KFX bytes");
    let ci_start = container.header.info_offset as usize;
    let ci_end = ci_start + container.header.info_length as usize;
    let ci_vals = IonReader::new(&raw_kfx[ci_start..ci_end])
        .decode_all()
        .expect("Decode ContainerInfo");
    let sym_doc_off = kfx_converter::KfxSymbol::Bcdocsymboloffset as u32;
    let sym_doc_len = kfx_converter::KfxSymbol::Bcdocsymbollength as u32;
    let mut local_symbols = kfx_converter::symbols::LocalSymbols::new();
    if let IonValue::Struct(ci_fields) = &ci_vals[0] {
        let doc_off = ci_fields
            .iter()
            .find(|(k, _)| *k == sym_doc_off)
            .and_then(|(_, v)| match v {
                IonValue::Int(n) => Some(*n as usize),
                _ => None,
            })
            .expect("bcDocSymbolOffset present");
        let doc_len = ci_fields
            .iter()
            .find(|(k, _)| *k == sym_doc_len)
            .and_then(|(_, v)| match v {
                IonValue::Int(n) => Some(*n as usize),
                _ => None,
            })
            .expect("bcDocSymbolLength present");
        let doc_vals = IonReader::new(&raw_kfx[doc_off..doc_off + doc_len])
            .decode_all()
            .expect("Decode $ion_symbol_table");
        let sym_table_struct = match &doc_vals[0] {
            IonValue::Annotated(_, inner) => inner.as_ref(),
            other => other,
        };
        if let IonValue::Struct(st_fields) = sym_table_struct
            && let Some((_, IonValue::List(syms))) = st_fields.iter().find(|(k, _)| *k == 7)
        {
            for sym in syms {
                if let IonValue::String(name) = sym {
                    local_symbols.intern(name);
                }
            }
        }
    }

    KfxPreflightValidator::new(&entities, Some(&local_symbols))
        .validate_container()
        .expect("End-to-end KFX container must pass full preflight validation");
}
