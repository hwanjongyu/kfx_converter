//! Amazon KFX Container (CONT) Reader and Writer
//! Implements binary packing/unpacking of the KFX container format compliant with Amazon Kindle OS.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use thiserror::Error;

use crate::ion::{IonValue, IonWriter};
use crate::symbols::symbol_id;

pub const KFX_MAGIC: &[u8; 4] = b"CONT";
pub const KFX_ENTITY_MAGIC: &[u8; 4] = b"ENTY";

#[derive(Error, Debug)]
pub enum ContainerError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Invalid KFX container magic bytes: expected CONT, got {0:?}")]
    InvalidMagic([u8; 4]),
    #[error("DRM-protected KFX containers are not supported")]
    DrmProtected,
    #[error("Corrupt container: offset {0} exceeds file size")]
    CorruptOffset(u64),
    #[error("Invalid chunk header")]
    InvalidChunkHeader,
    #[error("Corrupted container header: {0}")]
    CorruptedHeader(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub header_length: u32,
    pub info_offset: u32,
    pub info_length: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkEntry {
    pub entity_id: u32,
    pub chunk_type: u32,
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone)]
pub struct KfxEntity {
    pub entity_id: u32,
    pub chunk_type: u32,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct KfxContainer {
    pub header: ContainerHeader,
    pub chunks: Vec<ChunkEntry>,
    reader: File,
}

impl KfxContainer {
    /// Opens and validates a KFX container from disk.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ContainerError> {
        let mut file = File::open(path)?;
        let file_len = file.metadata()?.len();
        if file_len < 18 {
            return Err(ContainerError::CorruptedHeader(format!(
                "File too small to be a KFX container ({} bytes)",
                file_len
            )));
        }

        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)?;
        if &magic == b"\xeaDRM" || &magic == b"\xa8DRM" {
            return Err(ContainerError::DrmProtected);
        }
        if &magic != KFX_MAGIC {
            return Err(ContainerError::InvalidMagic(magic));
        }

        let version = file.read_u16::<LittleEndian>()?;
        let header_length = file.read_u32::<LittleEndian>()?;
        let info_offset = file.read_u32::<LittleEndian>()?;
        let info_length = file.read_u32::<LittleEndian>()?;

        if (header_length as u64) > file_len || (info_offset as u64) > file_len {
            return Err(ContainerError::CorruptedHeader(format!(
                "Header length ({}) or info_offset ({}) exceeds file size ({})",
                header_length, info_offset, file_len
            )));
        }
        if (info_offset as u64) + (info_length as u64) > file_len {
            return Err(ContainerError::CorruptedHeader(format!(
                "ContainerInfo bounds ({} + {}) exceed file size ({})",
                info_offset, info_length, file_len
            )));
        }

        let header = ContainerHeader {
            magic,
            version,
            header_length,
            info_offset,
            info_length,
        };

        // Read chunk index table (bcIndexTab at offset 18 until info_offset)
        let mut index_tab_len = if info_offset >= 18 {
            (info_offset - 18) as usize
        } else {
            0
        };

        // If ContainerInfo is available at info_offset, check for exact bcIndexTabLength ($414: [0x03, 0x9e])
        if (info_offset as u64) + (info_length as u64) <= file_len && info_length > 0 {
            file.seek(SeekFrom::Start(info_offset as u64))?;
            let mut ci_bytes = vec![0u8; info_length as usize];
            if file.read_exact(&mut ci_bytes).is_ok() {
                if let Some(pos) = ci_bytes.windows(2).position(|w| w == [0x03, 0x9e]) {
                    let val_bytes = &ci_bytes[pos + 2..];
                    if !val_bytes.is_empty() {
                        let type_desc = val_bytes[0];
                        if (type_desc & 0xf0) == 0x20 {
                            let len = (type_desc & 0x0f) as usize;
                            if len > 0 && len <= 8 && val_bytes.len() >= 1 + len {
                                let mut tab_len: u64 = 0;
                                for b in &val_bytes[1..1 + len] {
                                    tab_len = (tab_len << 8) | (*b as u64);
                                }
                                if tab_len > 0 && tab_len < index_tab_len as u64 {
                                    index_tab_len = tab_len as usize;
                                }
                            }
                        }
                    }
                }
            }
        }

        let chunk_count = index_tab_len / 24;
        if chunk_count > 50_000 {
            return Err(ContainerError::CorruptedHeader(format!(
                "Chunk count {} exceeds safety limit of 50,000",
                chunk_count
            )));
        }

        if (18 + chunk_count * 24) as u64 > file_len {
            return Err(ContainerError::CorruptedHeader(
                "bcIndexTab table exceeds file size".into(),
            ));
        }

        file.seek(SeekFrom::Start(18))?;
        let mut chunks = Vec::with_capacity(chunk_count);
        for _ in 0..chunk_count {
            let entity_id = file.read_u32::<LittleEndian>()?;
            let chunk_type = file.read_u32::<LittleEndian>()?;
            let offset = file.read_u64::<LittleEndian>()?;
            let length = file.read_u64::<LittleEndian>()?;

            chunks.push(ChunkEntry {
                entity_id,
                chunk_type,
                offset: (header_length as u64) + offset,
                length,
            });
        }

        Ok(Self {
            header,
            chunks,
            reader: file,
        })
    }

    /// Reads the raw payload bytes for a specific chunk entry, stripping any ENTY envelope if present.
    pub fn read_chunk(&mut self, chunk: &ChunkEntry) -> Result<Vec<u8>, ContainerError> {
        let file_len = self.reader.metadata()?.len();
        if chunk.offset.checked_add(chunk.length).map_or(true, |end| end > file_len) {
            return Err(ContainerError::CorruptedHeader(format!(
                "Chunk offset ({}) + length ({}) exceeds file size ({})",
                chunk.offset, chunk.length, file_len
            )));
        }
        if chunk.length > 500 * 1024 * 1024 {
            return Err(ContainerError::CorruptedHeader(format!(
                "Chunk length ({}) exceeds 500MB safety limit",
                chunk.length
            )));
        }

        self.reader.seek(SeekFrom::Start(chunk.offset))?;
        let mut buf = vec![0u8; chunk.length as usize];
        self.reader.read_exact(&mut buf)?;

        // If enveloped by ENTY header, unwrap it dynamically using header_length (bytes 6..10)
        if buf.len() >= 10 && &buf[0..4] == KFX_ENTITY_MAGIC {
            let header_len = (&buf[6..10]).read_u32::<LittleEndian>()? as usize;
            if header_len <= buf.len() {
                return Ok(buf[header_len..].to_vec());
            }
        }

        Ok(buf)
    }

    /// Creates and writes a new Amazon KFX container to a file or stream.
    pub fn write_to<W: Write + Seek>(
        entities: &[KfxEntity],
        container_id: &str,
        writer: W,
    ) -> Result<u64, ContainerError> {
        Self::write_to_with_symbols(entities, container_id, None, None, writer)
    }

    /// Creates and writes a new Amazon KFX container with optional Document Symbols ($ion_symbol_table)
    /// and optional Format Capabilities ($593).
    pub fn write_to_with_symbols<W: Write + Seek>(
        entities: &[KfxEntity],
        container_id: &str,
        doc_symbols_data: Option<&[u8]>,
        format_caps_data: Option<&[u8]>,
        mut writer: W,
    ) -> Result<u64, ContainerError> {
        let mut entity_table_entries = Vec::with_capacity(entities.len() * 24);
        let mut current_raw_offset: u64 = 0;

        for ent in entities {
            // Yellowjacket spec assertion: non-singleton entity_id must be >= 852
            // 348 is SINGLETON_ID ($null)
            if ent.entity_id != 348 && ent.entity_id < 852 {
                debug_assert!(
                    false,
                    "Yellowjacket spec violation: non-singleton entity_id {} must be >= 852",
                    ent.entity_id
                );
            }

            let payload_len = ent.data.len() as u64 + crate::ion::ENTY_HEADER_LEN as u64;

            // Record: entity_id (u32), chunk_type (u32), raw_offset (u64), raw_len (u64)
            entity_table_entries.write_u32::<LittleEndian>(ent.entity_id)?;
            entity_table_entries.write_u32::<LittleEndian>(ent.chunk_type)?;
            entity_table_entries.write_u64::<LittleEndian>(current_raw_offset)?;
            entity_table_entries.write_u64::<LittleEndian>(payload_len)?;

            current_raw_offset += payload_len;
        }

        let index_tab_offset: u32 = 18;
        let index_tab_length: u32 = entity_table_entries.len() as u32;

        let doc_sym_bytes = doc_symbols_data.unwrap_or(&[]);
        let doc_sym_offset = index_tab_offset + index_tab_length;
        let doc_sym_length = doc_sym_bytes.len() as u32;

        let format_caps_bytes = format_caps_data.unwrap_or(&[]);
        let format_caps_offset = doc_sym_offset + doc_sym_length;
        let format_caps_length = format_caps_bytes.len() as u32;

        let ci_offset = format_caps_offset + format_caps_length;

        // Build ContainerInfo Ion structure
        let sym_bc_cont_id = symbol_id("bcContId").unwrap_or(409);
        let sym_bc_compr_type = symbol_id("bcComprType").unwrap_or(410);
        let sym_bc_drm_scheme = symbol_id("bcDRMScheme").unwrap_or(411);
        let sym_bc_chunk_size = symbol_id("bcChunkSize").unwrap_or(412);
        let sym_bc_index_tab_offset = symbol_id("bcIndexTabOffset").unwrap_or(413);
        let sym_bc_index_tab_length = symbol_id("bcIndexTabLength").unwrap_or(414);

        let mut ci_fields_vec = vec![
            (sym_bc_cont_id, IonValue::String(container_id.to_string())),
            (sym_bc_compr_type, IonValue::Int(0)),
            (sym_bc_drm_scheme, IonValue::Int(0)),
            (sym_bc_chunk_size, IonValue::Int(4096)),
            (sym_bc_index_tab_offset, IonValue::Int(index_tab_offset as i64)),
            (sym_bc_index_tab_length, IonValue::Int(index_tab_length as i64)),
        ];

        if doc_sym_length > 0 {
            let sym_doc_sym_offset = symbol_id("bcDocSymbolOffset").unwrap_or(415);
            let sym_doc_sym_length = symbol_id("bcDocSymbolLength").unwrap_or(416);
            ci_fields_vec.push((sym_doc_sym_offset, IonValue::Int(doc_sym_offset as i64)));
            ci_fields_vec.push((sym_doc_sym_length, IonValue::Int(doc_sym_length as i64)));
        }

        if format_caps_length > 0 {
            let sym_fcap_offset = symbol_id("bcFCapabilitiesOffset").unwrap_or(594);
            let sym_fcap_length = symbol_id("bcFCapabilitiesLength").unwrap_or(595);
            ci_fields_vec.push((sym_fcap_offset, IonValue::Int(format_caps_offset as i64)));
            ci_fields_vec.push((sym_fcap_length, IonValue::Int(format_caps_length as i64)));
        }

        let ci_fields = IonValue::Struct(ci_fields_vec);

        let mut ci_writer = IonWriter::new();
        ci_writer.write_bvm();
        ci_writer.write_value(&ci_fields);
        let ci_bytes = ci_writer.into_bytes();
        let ci_length = ci_bytes.len() as u32;

        let kfxgen_info = b"[]";
        let header_length = ci_offset + ci_length + kfxgen_info.len() as u32;

        // Write Fixed 18-byte CONT Header (<4sHIII)
        writer.write_all(KFX_MAGIC)?;
        writer.write_u16::<LittleEndian>(2)?; // version 2
        writer.write_u32::<LittleEndian>(header_length)?;
        writer.write_u32::<LittleEndian>(ci_offset)?;
        writer.write_u32::<LittleEndian>(ci_length)?;

        // Write entity index table (bcIndexTab)
        writer.write_all(&entity_table_entries)?;

        // Write Document Symbols if present
        if doc_sym_length > 0 {
            writer.write_all(doc_sym_bytes)?;
        }

        // Write Format Capabilities if present
        if format_caps_length > 0 {
            writer.write_all(format_caps_bytes)?;
        }

        // Write ContainerInfo Ion
        writer.write_all(&ci_bytes)?;

        // Write kfxgen info trailer so standard tools (Calibre kfxlib, etc.) can parse cleanly
        writer.write_all(kfxgen_info)?;

        // Write all wrapped entity payloads directly to writer (zero intermediate allocations)
        for ent in entities {
            writer.write_all(crate::ion::MAGIC_ENTY)?;
            writer.write_u16::<LittleEndian>(1)?;
            writer.write_u32::<LittleEndian>(crate::ion::ENTY_HEADER_LEN)?;
            writer.write_all(&crate::ion::ENTY_HEADER_ION)?;
            writer.write_all(&ent.data)?;
        }

        let total_size = writer.seek(SeekFrom::Current(0))?;
        Ok(total_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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
            KfxContainer::write_to(&entities, "CR!MOCKCONTAINERID123456789", &mut buffer)
                .expect("Write failed");
        assert!(total_size > 0);

        let data = buffer.into_inner();
        assert_eq!(&data[0..4], b"CONT");
        assert_eq!(data.len(), total_size as usize);

        let mut cursor = Cursor::new(data);
        cursor.seek(SeekFrom::Start(0)).unwrap();
        // Verify we can read it
        let mut tmp_file = tempfile::NamedTempFile::new().unwrap();
        tmp_file.write_all(cursor.get_ref()).unwrap();

        let mut opened = KfxContainer::open(tmp_file.path()).expect("Open failed");
        assert_eq!(opened.chunks.len(), 2);
        assert_eq!(opened.chunks[0].chunk_type, 409);
        assert_eq!(opened.chunks[1].chunk_type, 260);

        let payload0 = opened.read_chunk(&opened.chunks[0].clone()).unwrap();
        assert_eq!(payload0, b"MOCK_METADATA_CHUNK");
    }

    #[test]
    fn test_container_rejects_huge_info_offset_oom_bomb() {
        let mut tmp_file = tempfile::NamedTempFile::new().unwrap();
        // Construct malformed header: CONT + ver 2 + header_len 18 + info_offset 0xFFFFFFFF + info_length 100
        let mut malformed = Vec::new();
        malformed.extend_from_slice(b"CONT");
        malformed.write_u16::<LittleEndian>(2).unwrap();
        malformed.write_u32::<LittleEndian>(18).unwrap();
        malformed.write_u32::<LittleEndian>(0xFFFFFFFF).unwrap(); // 4GB offset bomb!
        malformed.write_u32::<LittleEndian>(100).unwrap();
        tmp_file.write_all(&malformed).unwrap();

        let result = KfxContainer::open(tmp_file.path());
        assert!(result.is_err());
        match result.err().unwrap() {
            ContainerError::CorruptedHeader(msg) => {
                assert!(msg.contains("exceeds file size"));
            }
            other => panic!("Expected CorruptedHeader error, got {:?}", other),
        }
    }

    #[test]
    fn test_container_rejects_drm_magic() {
        let mut tmp_file = tempfile::NamedTempFile::new().unwrap();
        let mut drm_data = Vec::new();
        drm_data.extend_from_slice(b"\xeaDRM");
        drm_data.extend_from_slice(&[0u8; 32]);
        tmp_file.write_all(&drm_data).unwrap();

        let result = KfxContainer::open(tmp_file.path());
        assert!(matches!(result, Err(ContainerError::DrmProtected)));
    }

    #[test]
    fn test_container_rejects_out_of_bounds_chunk_length() {
        let entities = vec![KfxEntity {
            entity_id: 852,
            chunk_type: 409,
            data: b"CHUNK_DATA".to_vec(),
        }];

        let mut buffer = Cursor::new(Vec::new());
        KfxContainer::write_to(&entities, "CR!MOCKCONTAINERID", &mut buffer).unwrap();

        let mut tmp_file = tempfile::NamedTempFile::new().unwrap();
        tmp_file.write_all(buffer.get_ref()).unwrap();

        let mut opened = KfxContainer::open(tmp_file.path()).unwrap();
        // Tamper with chunk length to exceed file bounds
        let mut corrupted_chunk = opened.chunks[0].clone();
        corrupted_chunk.length = 10_000_000; // Exceeds small file size

        let result = opened.read_chunk(&corrupted_chunk);
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), ContainerError::CorruptedHeader(_)));
    }

    #[test]
    fn test_yj_format_capabilities_header_layout() {
        let entities = vec![KfxEntity {
            entity_id: 852,
            chunk_type: 145,
            data: b"CONTENT_BYTES".to_vec(),
        }];

        let doc_sym = b"DOC_SYMBOLS_MOCK";
        let format_caps = b"FORMAT_CAPS_MOCK";

        let mut buffer = Cursor::new(Vec::new());
        let total_size = KfxContainer::write_to_with_symbols(
            &entities,
            "CR!TESTCONTAINER1234567890123",
            Some(doc_sym),
            Some(format_caps),
            &mut buffer,
        )
        .expect("write_to_with_symbols failed");
        assert!(total_size > 0);

        let data = buffer.into_inner();
        // Check CONT header fields
        assert_eq!(&data[0..4], b"CONT");
        let version = (&data[4..6]).read_u16::<LittleEndian>().unwrap();
        assert_eq!(version, 2);

        let header_length = (&data[6..10]).read_u32::<LittleEndian>().unwrap();
        let ci_offset = (&data[10..14]).read_u32::<LittleEndian>().unwrap();
        let ci_length = (&data[14..18]).read_u32::<LittleEndian>().unwrap();

        assert_eq!(header_length, ci_offset + ci_length + 2);
        assert_eq!(&data[(ci_offset + ci_length) as usize..header_length as usize], b"[]");

        // index_tab is 1 entry * 24 bytes = 24 bytes, starts at 18
        // doc_sym starts at 18 + 24 = 42
        assert_eq!(&data[42..42 + doc_sym.len()], doc_sym);

        // format_caps starts immediately after doc_sym: 42 + doc_sym.len()
        let format_caps_offset = 42 + doc_sym.len();
        assert_eq!(&data[format_caps_offset..format_caps_offset + format_caps.len()], format_caps);

        // ContainerInfo starts after format_caps
        assert_eq!(ci_offset as usize, format_caps_offset + format_caps.len());
    }
}
