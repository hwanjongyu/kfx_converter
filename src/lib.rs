//! # KFX Converter
//! High-performance native EPUB to Amazon KFX converter in Rust.

pub mod symbols;
pub mod container;
pub mod ir;
pub mod ion;
pub mod epub;
pub mod packager;
pub mod enhancements;
pub mod sync;
pub mod pipeline;

pub use container::{KfxContainer, ContainerError, ContainerHeader, KfxEntity, ChunkEntry};
pub use symbols::{symbol_name, symbol_id, KfxSymbol, KFX_SYMBOL_TABLE};
pub use ir::BookIR;
pub use ion::{IonWriter, IonValue, encode_varuint, wrap_enty_header};
pub use epub::{EpubReader, EpubError};
pub use packager::{KfxPackager, PackagerError};
pub use enhancements::{CoverOptimizer, CoverError, WordWiseBuilder, WordWiseGloss};
pub use sync::{KindleSync, SyncError};
pub use pipeline::{ConversionPipeline, ConversionOptions, ConversionReport, PipelineError};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_symbols_lookup() {
        assert_eq!(symbol_name(1), Some("$ion"));
        assert_eq!(symbol_name(409), Some("bcContId"));
        assert_eq!(symbol_name(851), Some("vertex_list"));
        assert_eq!(symbol_name(9999), None);
    }

    #[test]
    fn test_container_roundtrip() {
        let entities = vec![
            KfxEntity {
                entity_id: 852,
                chunk_type: 409,
                data: b"MOCK_METADATA_CHUNK".to_vec(),
            },
            KfxEntity {
                entity_id: 853,
                chunk_type: 260,
                data: b"MOCK_STORYLINE_CHUNK".to_vec(),
            },
        ];

        let mut buffer = Cursor::new(Vec::new());
        let total_size =
            KfxContainer::write_to(&entities, "CR!MOCKCONTAINERID123", &mut buffer)
                .expect("Write failed");
        assert!(total_size > 0);

        let data = buffer.into_inner();
        assert_eq!(&data[0..4], b"CONT");
        assert_eq!(data.len(), total_size as usize);
    }
}
