//! Core packaging pipeline lifecycle coordinator.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{Seek, Write};
use std::path::Path;

use super::error::PackagerError;
use super::symbols::*;
use super::types::AllocatedResourceSym;
use crate::container::{KfxContainer, KfxEntity};
use crate::ir::{BookIR, ContentType, Section};

/// Packages a `BookIR` into an Amazon KFX container.
pub struct KfxPackager<'a> {
    pub(crate) book: &'a BookIR,
    pub(crate) sections: Cow<'a, [Section]>,
    pub(crate) toc: Vec<crate::ir::TocItem>,
    pub(crate) page_list: Vec<crate::ir::PageEntry>,
    pub(crate) content_type: ContentType,
    pub(crate) container_id: String,
    pub(crate) content_id: String,
    pub(crate) asin: String,
    pub book_id: String,
    pub entities: Vec<KfxEntity>,
    pub(crate) next_eid: u32,
    pub(crate) image_block_eids: HashSet<u32>,
    pub(crate) anchor_targets: HashMap<(String, String), (u32, usize, usize)>,
    pub(crate) global_anchor_targets: HashMap<String, (u32, usize, usize)>,
    pub(crate) section_pid_starts: HashMap<u32, usize>,
    pub(crate) resource_symbols: HashMap<String, AllocatedResourceSym>,
    pub(crate) has_native_tables: bool,
}

impl<'a> KfxPackager<'a> {
    pub fn new(book: &'a BookIR, content_type: &str) -> Self {
        Self::new_with_content_type(book, ContentType::from_lossy(content_type))
    }

    pub fn new_with_content_type(book: &'a BookIR, content_type: ContentType) -> Self {
        let seed = if !book.metadata.identifier.is_empty() {
            book.metadata.identifier.clone()
        } else {
            format!("{}\n{:?}", book.metadata.title, book.metadata.creators)
        };

        let container_id = generate_container_id(&seed);
        let content_id = generate_content_id(&seed);

        // Kindle Yellowjacket & Calibre requirement:
        // ASIN and content_id MUST be identical (32-character hexadecimal digest),
        // so that sidecar thumbnails named thumbnail_{content_id}_{cde_content_type}_portrait.jpg
        // match both Kindle OS lockscreen queries and Calibre sideloading routines.
        let asin = content_id.clone();

        let book_id = generate_book_id(&seed);

        let sections = book.sections_with_cover();
        let toc = book.toc.clone();
        let page_list = book.page_list.clone();

        Self {
            book,
            sections,
            toc,
            page_list,
            content_type,
            container_id,
            content_id,
            asin,
            book_id,
            entities: Vec::new(),
            next_eid: FIRST_LOCAL_SYMBOL_ID,
            image_block_eids: HashSet::new(),
            anchor_targets: HashMap::new(),
            global_anchor_targets: HashMap::new(),
            section_pid_starts: HashMap::new(),
            resource_symbols: HashMap::new(),
            has_native_tables: false,
        }
    }

    pub(crate) fn normalize_ir_path(raw: &str) -> String {
        let decoded = crate::util::decode_percent(raw.trim());
        crate::util::resolve_zip_path("", &decoded)
    }

    pub(crate) fn resolve_resource_key(&self, resource_id: &str) -> Option<String> {
        self.book.resolve_resource_key(resource_id)
    }

    pub fn container_id(&self) -> &str {
        &self.container_id
    }

    pub fn content_id(&self) -> &str {
        &self.content_id
    }

    pub fn asin(&self) -> &str {
        &self.asin
    }

    pub fn content_type(&self) -> ContentType {
        self.content_type
    }

    pub(crate) fn alloc_eid(&mut self) -> u32 {
        let eid = self.next_eid;
        self.next_eid += 1;
        if eid == SINGLETON_ID {
            let next = self.next_eid;
            self.next_eid += 1;
            next
        } else {
            eid
        }
    }

    /// Assembles all entities and writes the `.kfx` file atomically using a temporary file.
    pub fn package_to_file<P: AsRef<Path>>(&mut self, path: P) -> Result<u64, PackagerError> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        crate::storage::reject_symlink_parent(parent)?;
        crate::storage::reject_symlink_path(path)?;
        let mut tmp_file = tempfile::Builder::new()
            .prefix(".kfx_tmp.")
            .tempfile_in(parent)?;
        let total_size = self.package(&mut tmp_file)?;
        tmp_file.flush()?;
        crate::storage::prepare_writable_non_symlink(path)?;
        crate::storage::reject_symlink_path(path)?;
        tmp_file.persist(path).map_err(|e| e.error)?;
        Ok(total_size)
    }

    /// Assembles all entities and writes to a stream.
    pub fn package<W: Write + Seek>(&mut self, writer: &mut W) -> Result<u64, PackagerError> {
        self.entities.clear();
        self.next_eid = FIRST_LOCAL_SYMBOL_ID;
        self.image_block_eids.clear();
        self.anchor_targets.clear();
        self.global_anchor_targets.clear();
        self.section_pid_starts.clear();
        self.resource_symbols.clear();
        self.has_native_tables = false;
        let mut symbols = LocalSymbols::new();

        // 1. Build typography and text styles ($157) - MUST be first symbol-interning step (SIDs 852..=890)
        let (styles, style_list) = self.build_styles(&mut symbols);

        // 2. Pre-allocate collision-free local symbols for supported raster image resources
        self.allocate_resource_symbols(&mut symbols);

        // 3. Build book metadata ($490) using allocated cover resource symbol name
        self.build_book_metadata();

        // 4. Build section content ($145), storylines ($259), section templates ($260), and auxiliary_data ($597)
        let (section_data, used_styles) = self.build_storylines_and_sections(&mut symbols, &styles);

        // 5. Emit only referenced style entities ($157) to prevent unreferenced fragment errors
        self.emit_used_styles(style_list, &used_styles);

        // 6. Build resource entities ($164 external_resource and $417 bcRawMedia) before content_features ($585)
        self.build_resources(&mut symbols);

        // 7. Build content features ($585) with dynamic section size, table, & emitted image flags
        self.build_content_features(&section_data);

        // 8. Build navigation ($389 book_navigation and $391 nav_container)
        self.build_navigation(&mut symbols, &section_data);

        // 9. Build metadata ($258) with synchronized reading_orders
        self.build_metadata(&section_data);

        // 10. Build Kindle OS position maps ($609, $264, $265, $550, $621)
        self.build_position_maps(&section_data);

        // 11. Build container_entity_map ($419) BEFORE document_data ($538) so max_id covers all symbols
        self.build_container_entity_map(&section_data, &mut symbols);

        // 12. Build document data ($538) with Decimal font_size & line_height
        self.build_document_data(&section_data, &symbols);

        // 13. Build $ion_symbol_table document symbols data
        let doc_sym_data = symbols.to_ion_symbol_table();

        // 14. Build format capabilities ($593) - emitted into container header based on actual post-optimization chunks
        let format_caps_data = self.build_format_capabilities();

        // 15. Pre-flight container and reference validation to ensure 100% integrity
        let validator =
            crate::validator::KfxPreflightValidator::new(&self.entities, Some(&symbols));
        validator.validate_container()?;

        // Sort entities by Yellowjacket preferred fragment type order, then by entity_id
        fn fragment_type_order(chunk_type: u32) -> u32 {
            match chunk_type {
                585 => 1,  // content_features
                490 => 2,  // book_metadata
                258 => 3,  // metadata
                538 => 4,  // document_data
                389 => 5,  // book_navigation
                260 => 6,  // section
                259 => 7,  // storyline
                145 => 8,  // content
                157 => 9,  // style
                391 => 10, // nav_container
                393 => 11, // nav_unit
                266 => 12, // anchor
                264 => 13, // position_map
                265 => 14, // position_id_map
                550 => 15, // location_map
                609 => 16, // section_position_id_map
                597 => 17, // auxiliary_data
                164 => 18, // external_resource
                417 => 19, // bcRawMedia
                419 => 20, // container_entity_map
                621 => 21, // location_pid_map
                _ => 100,
            }
        }

        self.entities
            .sort_by_key(|e| (fragment_type_order(e.chunk_type), e.entity_id));

        let total_size = KfxContainer::write_to_with_symbols(
            &self.entities,
            &self.container_id,
            Some(&doc_sym_data),
            Some(&format_caps_data),
            writer,
        )?;
        Ok(total_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Block, BookMetadata, TextSpan};

    #[test]
    fn test_package_to_file_bare_relative_filename() {
        if std::env::var_os("KFX_CORE_BARE_FILE_CHILD").is_some() {
            let book = BookIR {
                metadata: BookMetadata {
                    title: "Bare Path Book".to_string(),
                    identifier: "urn:uuid:bare-path-test".to_string(),
                    language: "en".to_string(),
                    ..Default::default()
                },
                sections: vec![Section {
                    id: "sec1".to_string(),
                    title: "Chapter 1".to_string(),
                    blocks: vec![Block::Paragraph {
                        id: "p1".to_string(),
                        spans: vec![TextSpan {
                            text: "Bare relative packaging test.".to_string(),
                            ..Default::default()
                        }],
                        style: None,
                    }],
                }],
                ..Default::default()
            };
            let mut packager = KfxPackager::new(&book, "EBOK");
            let out = Path::new("bare_output.kfx");
            assert_eq!(out.parent(), Some(Path::new("")));
            let written = packager
                .package_to_file(out)
                .expect("package_to_file must succeed on bare relative filename");
            assert!(written > 0);
            let container = KfxContainer::open(out).expect("Valid KFX output");
            assert!(!container.chunks.is_empty());
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let output = std::process::Command::new(exe)
            .args([
                "--exact",
                "packager::core::tests::test_package_to_file_bare_relative_filename",
            ])
            .current_dir(temp.path())
            .env("KFX_CORE_BARE_FILE_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Bare filename child test failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(temp.path().join("bare_output.kfx").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn test_package_to_file_rejects_symlink_leaf_and_parent() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let real_dir = temp.path().join("real_dir");
        std::fs::create_dir_all(&real_dir).unwrap();
        let link_dir = temp.path().join("link_dir");
        symlink(&real_dir, &link_dir).unwrap();

        let victim = temp.path().join("victim.kfx");
        std::fs::write(&victim, b"SAFE_VICTIM").unwrap();
        let leaf_link = real_dir.join("out.kfx");
        symlink(&victim, &leaf_link).unwrap();

        let book = BookIR {
            metadata: BookMetadata {
                title: "Symlink Test".to_string(),
                identifier: "urn:uuid:symlink-test".to_string(),
                language: "en".to_string(),
                ..Default::default()
            },
            sections: vec![Section {
                id: "sec1".to_string(),
                title: "Chapter 1".to_string(),
                blocks: vec![Block::Paragraph {
                    id: "p1".to_string(),
                    spans: vec![TextSpan {
                        text: "Hello.".to_string(),
                        ..Default::default()
                    }],
                    style: None,
                }],
            }],
            ..Default::default()
        };

        let mut packager = KfxPackager::new(&book, "EBOK");
        let leaf_err = packager.package_to_file(&leaf_link).unwrap_err();
        assert!(
            matches!(leaf_err, PackagerError::Io(ref e) if e.kind() == std::io::ErrorKind::PermissionDenied),
            "Expected PermissionDenied for symlink leaf, got {:?}",
            leaf_err
        );
        assert_eq!(std::fs::read(&victim).unwrap(), b"SAFE_VICTIM");

        let parent_err = packager
            .package_to_file(link_dir.join("child.kfx"))
            .unwrap_err();
        assert!(
            matches!(parent_err, PackagerError::Io(ref e) if e.kind() == std::io::ErrorKind::PermissionDenied),
            "Expected PermissionDenied for symlink parent, got {:?}",
            parent_err
        );
        assert!(!real_dir.join("child.kfx").exists());
    }

    #[test]
    fn test_package_to_file_overwrites_readonly_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("readonly_book.kfx");
        std::fs::write(&out, b"OLD_READONLY").unwrap();
        let mut perms = std::fs::metadata(&out).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&out, perms).unwrap();

        let book = BookIR {
            metadata: BookMetadata {
                title: "Readonly Overwrite Test".to_string(),
                identifier: "urn:uuid:readonly-overwrite-test".to_string(),
                language: "en".to_string(),
                ..Default::default()
            },
            sections: vec![Section {
                id: "sec1".to_string(),
                title: "Chapter 1".to_string(),
                blocks: vec![Block::Paragraph {
                    id: "p1".to_string(),
                    spans: vec![TextSpan {
                        text: "Overwriting read-only regular file.".to_string(),
                        ..Default::default()
                    }],
                    style: None,
                }],
            }],
            ..Default::default()
        };

        let mut packager = KfxPackager::new(&book, "EBOK");
        let written = packager
            .package_to_file(&out)
            .expect("package_to_file must overwrite existing read-only regular file");
        assert!(written > 0);

        let container = KfxContainer::open(&out).expect("Valid overwritten KFX output");
        assert!(!container.chunks.is_empty());

        let meta = std::fs::symlink_metadata(&out).unwrap();
        assert!(!meta.file_type().is_symlink());
        assert!(meta.file_type().is_file());
        assert!(!meta.permissions().readonly());
    }

    #[test]
    fn test_packager_content_type_constructors_and_lossy_sanitization() {
        let book = BookIR::default();
        assert_eq!(
            KfxPackager::new(&book, "pdoc").content_type(),
            ContentType::Pdoc
        );
        assert_eq!(
            KfxPackager::new(&book, "  PDOC  ").content_type(),
            ContentType::Pdoc
        );
        assert_eq!(
            KfxPackager::new(&book, "../../evil").content_type(),
            ContentType::Ebok
        );
        assert_eq!(
            KfxPackager::new_with_content_type(&book, ContentType::Pdoc).content_type(),
            ContentType::Pdoc
        );
        assert_eq!(
            KfxPackager::new_with_content_type(&book, ContentType::Ebok).content_type(),
            ContentType::Ebok
        );
    }
}
