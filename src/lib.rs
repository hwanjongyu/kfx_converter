//! # KFX Converter
//! High-performance native EPUB to Amazon KFX converter in Rust.

pub mod container;
pub mod css;
pub mod enhancements;
pub mod epub;
pub mod ion;
pub mod ir;
pub mod packager;
pub mod pipeline;
pub mod symbols;
pub mod sync;
pub mod validator;

pub use container::{ChunkEntry, ContainerError, ContainerHeader, KfxContainer, KfxEntity};
pub use enhancements::{CoverError, CoverOptimizer, WordWiseBuilder, WordWiseGloss};
pub use epub::{EpubError, EpubReader};
pub use ion::{IonError, IonReader, IonValue, IonWriter, encode_varuint, wrap_enty_header};
pub use ir::{
    BlockStyle, BookIR, DropCapStyle, FontSizeBucket, SpanStyles, TextAlignment, TextFormatFlags,
};
pub use packager::{KfxPackager, PackagerError};
pub use pipeline::{
    ConversionOptions, ConversionPipeline, ConversionReport, PipelineError, conversion_log_path,
};
pub use symbols::{KFX_SYMBOL_TABLE, KfxSymbol, symbol_id, symbol_name};
pub use sync::{KindleBookFile, KindleSync, SyncError};
pub use validator::{KfxPreflightValidator, ValidationError, ValidationReport};

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
        let total_size = KfxContainer::write_to(&entities, "CR!MOCKCONTAINERID123", &mut buffer)
            .expect("Write failed");
        assert!(total_size > 0);

        let data = buffer.into_inner();
        assert_eq!(&data[0..4], b"CONT");
        assert_eq!(data.len(), total_size as usize);
    }
}
