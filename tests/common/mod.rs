#![allow(dead_code)]

use kfx_converter::{KfxContainer, KfxEntity};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use zip::write::{SimpleFileOptions, ZipWriter};

/// Creates a minimal valid EPUB file in the given directory or a new temp dir.
pub fn create_minimal_epub(
    dir: &Path,
    filename: &str,
    title: &str,
    language: &str,
    include_cover: bool,
) -> PathBuf {
    let epub_path = dir.join(filename);
    let mut buf = Cursor::new(Vec::new());

    {
        let mut zip = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default();

        // 1. mimetype (must be uncompressed according to EPUB spec)
        zip.start_file("mimetype", opts).unwrap();
        std::io::Write::write_all(&mut zip, b"application/epub+zip").unwrap();

        // 2. META-INF/container.xml
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

        // 3. content.opf
        zip.start_file("content.opf", opts).unwrap();
        let cover_item = if include_cover {
            r#"<item id="cover-img" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>"#
        } else {
            ""
        };
        let cover_meta = if include_cover {
            r#"<meta name="cover" content="cover-img"/>"#
        } else {
            ""
        };

        let opf_content = format!(
            r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>{}</dc:title>
    <dc:language>{}</dc:language>
    <dc:creator>Test Author</dc:creator>
    {}
  </metadata>
  <manifest>
    <item id="c1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    {}
  </manifest>
  <spine toc="ncx">
    <itemref idref="c1"/>
  </spine>
</package>"#,
            title, language, cover_meta, cover_item
        );
        std::io::Write::write_all(&mut zip, opf_content.as_bytes()).unwrap();

        // 4. toc.ncx
        zip.start_file("toc.ncx", opts).unwrap();
        let ncx_content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head><meta name="dtb:uid" content="urn:uuid:test"/></head>
  <docTitle><text>{}</text></docTitle>
  <navMap>
    <navPoint id="np-1" playOrder="1">
      <navLabel><text>Chapter 1</text></navLabel>
      <content src="chapter1.xhtml"/>
    </navPoint>
  </navMap>
</ncx>"#,
            title
        );
        std::io::Write::write_all(&mut zip, ncx_content.as_bytes()).unwrap();

        // 5. chapter1.xhtml with some vocabulary to trigger Word Wise
        zip.start_file("chapter1.xhtml", opts).unwrap();
        let xhtml_content = r#"<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Chapter 1</title></head>
<body>
  <h1>Chapter 1: The Beginning</h1>
  <p>It was an abstruse and chimerical morning when the serendipity of the ephemeral dawn appeared.</p>
  <p>The luminous journey continued through the idiosyncratic landscape with meticulous observation.</p>
</body>
</html>"#;
        std::io::Write::write_all(&mut zip, xhtml_content.as_bytes()).unwrap();

        // 6. Optional cover image
        if include_cover {
            zip.start_file("cover.jpg", opts).unwrap();
            // A minimal 1x1 valid baseline JPEG
            let dummy_jpeg = create_minimal_jpeg();
            std::io::Write::write_all(&mut zip, &dummy_jpeg).unwrap();
        }

        zip.finish().unwrap();
    }

    std::fs::write(&epub_path, buf.into_inner()).unwrap();
    epub_path
}

/// Generates a valid minimal 2x2 baseline JPEG image in memory.
pub fn create_minimal_jpeg() -> Vec<u8> {
    let img = image::RgbImage::new(2, 2);
    let mut jpeg_buf = Cursor::new(Vec::new());
    img.write_to(&mut jpeg_buf, image::ImageFormat::Jpeg)
        .unwrap();
    jpeg_buf.into_inner()
}

/// Creates a valid KFX container in memory and writes to a file.
pub fn create_valid_kfx(path: &Path) {
    let entities = vec![
        KfxEntity {
            entity_id: 852,
            chunk_type: 409, // bcContId
            data: b"MOCK_METADATA_CHUNK".to_vec(),
        },
        KfxEntity {
            entity_id: 853,
            chunk_type: 260, // section
            data: b"MOCK_STORYLINE_CHUNK".to_vec(),
        },
    ];

    let mut buf = Cursor::new(Vec::new());
    KfxContainer::write_to(&entities, "CR!MOCKCONTAINERID123", &mut buf)
        .expect("write_to must succeed");
    std::fs::write(path, buf.into_inner()).unwrap();
}

/// Creates a DRM-protected KFX mock file.
pub fn create_drm_kfx(path: &Path, magic: &[u8; 4]) {
    let mut data = Vec::new();
    data.extend_from_slice(magic);
    data.extend_from_slice(&[0u8; 64]);
    std::fs::write(path, data).unwrap();
}

/// Creates a temporary directory wrapper helper.
pub fn temp_dir() -> TempDir {
    tempfile::tempdir().unwrap()
}
