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

    let report = ConversionPipeline::run(
        &epub_path,
        Some(&kfx_path),
        &options,
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

    let report = ConversionPipeline::run(
        &epub_path,
        None, // None specifies default `<input_path>.kfx`
        &options,
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

    let report =
        ConversionPipeline::run(&epub_path, Some(&kfx_path), &options, None::<fn(f32, &str)>)
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
    let epub_path = common::create_minimal_epub(
        temp.path(),
        "korean_book.epub",
        "한국어 전자책 테스트",
        "ko",
        false,
    );
    let kfx_path = temp.path().join("korean_book.kfx");

    let options = ConversionOptions {
        content_type: "PDOC".to_string(),
        word_wise: true, // Should gracefully handle non-English by producing 0 glosses
        auto_deploy: false,
        device_root: None,
    };

    let report =
        ConversionPipeline::run(&epub_path, Some(&kfx_path), &options, None::<fn(f32, &str)>)
            .expect("Non-English EPUB conversion should succeed");

    assert_eq!(report.book_title, "한국어 전자책 테스트");
    assert!(kfx_path.is_file());
}

#[test]
fn test_pipeline_missing_input_file_returns_error() {
    let non_existent = std::path::Path::new("/missing/nonexistent/sample.epub");
    let options = ConversionOptions::default();

    let result = ConversionPipeline::run(non_existent, None, &options, None::<fn(f32, &str)>);

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

    let options = ConversionOptions::default();
    let result = ConversionPipeline::run(
        &empty_epub,
        Some(&target_kfx),
        &options,
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

    let options = ConversionOptions::default();
    let result = ConversionPipeline::run(
        &corrupt_epub,
        Some(&target_kfx),
        &options,
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

    let options = ConversionOptions::default();
    let result = ConversionPipeline::run(
        &invalid_epub,
        Some(&target_kfx),
        &options,
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

    let report =
        ConversionPipeline::run(&epub_path, Some(&kfx_path), &options, None::<fn(f32, &str)>)
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
