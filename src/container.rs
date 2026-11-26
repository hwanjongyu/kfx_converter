//! Amazon KFX Container (CONT) Reader and Writer
//! Implements binary packing/unpacking of the KFX container format compliant with Amazon Kindle OS.

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use thiserror::Error;

use crate::ion::{IonValue, IonWriter};
use crate::symbols::KfxSymbol;

pub const KFX_MAGIC: &[u8; 4] = b"CONT";
pub const KFX_ENTITY_MAGIC: &[u8; 4] = b"ENTY";
pub const MAX_CONTAINER_INFO_BYTES: u32 = 64 * 1024;

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

        if header_length < 18 {
            return Err(ContainerError::CorruptedHeader(format!(
                "Container header_length ({}) must be >= 18",
                header_length
            )));
        }
        if (header_length as u64) > file_len || (info_offset as u64) > file_len {
            return Err(ContainerError::CorruptedHeader(format!(
                "Header length ({}) or info_offset ({}) exceeds file size ({})",
                header_length, info_offset, file_len
            )));
        }
        if (info_offset as u64)
            .checked_add(info_length as u64)
            .is_none_or(|end| end > file_len)
        {
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

        let mut index_tab_offset: u64 = 18;
        let mut explicit_index_tab_len: Option<usize> = None;

        // If ContainerInfo is present, decode and validate compression, DRM, and index table metadata
        if info_length > 0 {
            if info_length > MAX_CONTAINER_INFO_BYTES {
                return Err(ContainerError::CorruptedHeader(format!(
                    "ContainerInfo length ({}) exceeds safety limit ({})",
                    info_length, MAX_CONTAINER_INFO_BYTES
                )));
            }
            if info_offset < 18 {
                return Err(ContainerError::CorruptedHeader(format!(
                    "ContainerInfo offset ({}) must be >= 18",
                    info_offset
                )));
            }

            file.seek(SeekFrom::Start(info_offset as u64))?;
            let mut ci_bytes = vec![0u8; info_length as usize];
            file.read_exact(&mut ci_bytes)?;

            let mut reader = crate::ion::IonReader::new(&ci_bytes);
            let ci_val = reader.decode_value().map_err(|e| {
                ContainerError::CorruptedHeader(format!("Malformed ContainerInfo Ion: {}", e))
            })?;
            let fields = match ci_val {
                IonValue::Struct(fields) => fields,
                IonValue::Annotated(_, inner) => match *inner {
                    IonValue::Struct(fields) => fields,
                    _ => {
                        return Err(ContainerError::CorruptedHeader(
                            "ContainerInfo payload must be an Ion struct".into(),
                        ));
                    }
                },
                _ => {
                    return Err(ContainerError::CorruptedHeader(
                        "ContainerInfo payload must be an Ion struct".into(),
                    ));
                }
            };

            let mut has_index_offset = false;
            for (sid, val) in fields {
                match sid {
                    // $410 = bcComprType
                    410 => match val {
                        IonValue::Int(0) => {}
                        IonValue::Int(c) => {
                            return Err(ContainerError::CorruptedHeader(format!(
                                "Unsupported KFX container compression type: {}",
                                c
                            )));
                        }
                        other => {
                            return Err(ContainerError::CorruptedHeader(format!(
                                "Invalid bcComprType ($410) value: {:?}",
                                other
                            )));
                        }
                    },
                    // $411 = bcDRMScheme
                    411 => match val {
                        IonValue::Int(0) => {}
                        _ => return Err(ContainerError::DrmProtected),
                    },
                    // $413 = bcIndexTabOffset
                    413 => {
                        let off = match val {
                            IonValue::Int(v) if v >= 18 => u64::try_from(v).map_err(|_| {
                                ContainerError::CorruptedHeader(
                                    "Invalid bcIndexTabOffset ($413)".into(),
                                )
                            })?,
                            _ => {
                                return Err(ContainerError::CorruptedHeader(format!(
                                    "Invalid bcIndexTabOffset ($413): {:?}",
                                    val
                                )));
                            }
                        };
                        if off > file_len {
                            return Err(ContainerError::CorruptedHeader(format!(
                                "bcIndexTabOffset ({}) exceeds file size ({})",
                                off, file_len
                            )));
                        }
                        index_tab_offset = off;
                        has_index_offset = true;
                    }
                    // $414 = bcIndexTabLength
                    414 => {
                        let len_u64 = match val {
                            IonValue::Int(v) if v >= 0 => u64::try_from(v).map_err(|_| {
                                ContainerError::CorruptedHeader(
                                    "Invalid bcIndexTabLength ($414)".into(),
                                )
                            })?,
                            _ => {
                                return Err(ContainerError::CorruptedHeader(format!(
                                    "Invalid bcIndexTabLength ($414): {:?}",
                                    val
                                )));
                            }
                        };
                        if len_u64 % 24 != 0 {
                            return Err(ContainerError::CorruptedHeader(format!(
                                "bcIndexTabLength ({}) is not a multiple of 24",
                                len_u64
                            )));
                        }
                        let len_usize = usize::try_from(len_u64).map_err(|_| {
                            ContainerError::CorruptedHeader(
                                "bcIndexTabLength ($414) exceeds usize::MAX".into(),
                            )
                        })?;
                        explicit_index_tab_len = Some(len_usize);
                    }
                    _ => {}
                }
            }

            if let Some(len) = explicit_index_tab_len {
                if index_tab_offset
                    .checked_add(len as u64)
                    .is_none_or(|end| end > file_len)
                {
                    return Err(ContainerError::CorruptedHeader(format!(
                        "bcIndexTab bounds ({} + {}) exceed file size ({})",
                        index_tab_offset, len, file_len
                    )));
                }
                if has_index_offset && len == 0 && (header_length as u64) < file_len {
                    // Explicit index offset with 0-length table on non-empty payload container is suspicious
                    return Err(ContainerError::CorruptedHeader(
                        "bcIndexTabLength ($414) cannot be 0 when entity payload bytes exist"
                            .into(),
                    ));
                }
            }
        }

        let index_tab_len = match explicit_index_tab_len {
            Some(len) => len,
            None => {
                if (info_offset as u64) >= index_tab_offset {
                    ((info_offset as u64) - index_tab_offset) as usize
                } else {
                    0
                }
            }
        };

        let chunk_count = index_tab_len / 24;
        if chunk_count > 50_000 {
            return Err(ContainerError::CorruptedHeader(format!(
                "Chunk count {} exceeds safety limit of 50,000",
                chunk_count
            )));
        }

        let table_byte_len = (chunk_count as u64).checked_mul(24).ok_or_else(|| {
            ContainerError::CorruptedHeader("bcIndexTab table byte length overflow".into())
        })?;
        if index_tab_offset
            .checked_add(table_byte_len)
            .is_none_or(|end| end > file_len)
        {
            return Err(ContainerError::CorruptedHeader(
                "bcIndexTab table exceeds file size".into(),
            ));
        }

        file.seek(SeekFrom::Start(index_tab_offset))?;
        let mut chunks = Vec::with_capacity(chunk_count);
        for _ in 0..chunk_count {
            let entity_id = file.read_u32::<LittleEndian>()?;
            let chunk_type = file.read_u32::<LittleEndian>()?;
            let offset = file.read_u64::<LittleEndian>()?;
            let length = file.read_u64::<LittleEndian>()?;

            let abs_offset = (header_length as u64).checked_add(offset).ok_or_else(|| {
                ContainerError::CorruptedHeader(format!(
                    "Chunk offset arithmetic overflow: {}",
                    offset
                ))
            })?;

            chunks.push(ChunkEntry {
                entity_id,
                chunk_type,
                offset: abs_offset,
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
        if chunk
            .offset
            .checked_add(chunk.length)
            .is_none_or(|end| end > file_len)
        {
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

        let chunk_len = chunk.length as usize;
        if chunk_len == 0 {
            return Ok(Vec::new());
        }

        self.reader.seek(SeekFrom::Start(chunk.offset))?;
        let peek_len = chunk_len.min(10);
        let mut prefix = [0u8; 10];
        self.reader.read_exact(&mut prefix[..peek_len])?;

        // If enveloped by ENTY header, validate header_len and Ion metadata BEFORE allocating payload
        if peek_len >= 4 && &prefix[0..4] == KFX_ENTITY_MAGIC {
            if peek_len < 10 {
                return Err(ContainerError::InvalidChunkHeader);
            }
            let header_len = usize::try_from((&prefix[6..10]).read_u32::<LittleEndian>()?)
                .map_err(|_| ContainerError::InvalidChunkHeader)?;
            if header_len < 10 {
                return Err(ContainerError::CorruptedHeader(format!(
                    "ENTY chunk header length ({}) must be >= 10",
                    header_len
                )));
            }
            if header_len > chunk_len {
                return Err(ContainerError::CorruptedHeader(format!(
                    "ENTY chunk header length ({}) exceeds chunk buffer length ({})",
                    header_len, chunk_len
                )));
            }
            if header_len > 10 {
                let mut hdr_buf = vec![0u8; header_len - 10];
                self.reader.read_exact(&mut hdr_buf)?;
                let mut reader = crate::ion::IonReader::new(&hdr_buf);
                let hdr_val = reader.decode_value().map_err(|e| {
                    ContainerError::CorruptedHeader(format!("Malformed ENTY Ion header: {}", e))
                })?;
                let fields = match hdr_val {
                    IonValue::Struct(fields) => fields,
                    IonValue::Annotated(_, inner) => match *inner {
                        IonValue::Struct(fields) => fields,
                        _ => {
                            return Err(ContainerError::CorruptedHeader(
                                "ENTY header Ion payload must be a struct".into(),
                            ));
                        }
                    },
                    _ => {
                        return Err(ContainerError::CorruptedHeader(
                            "ENTY header Ion payload must be a struct".into(),
                        ));
                    }
                };
                for (sid, val) in fields {
                    match sid {
                        410 if val != IonValue::Int(0) => {
                            return Err(ContainerError::CorruptedHeader(format!(
                                "Unsupported ENTY compression type: {:?}",
                                val
                            )));
                        }
                        411 if val != IonValue::Int(0) => {
                            return Err(ContainerError::DrmProtected);
                        }
                        _ => {}
                    }
                }
            }
            let payload_len = chunk_len - header_len;
            let mut payload = vec![0u8; payload_len];
            self.reader.read_exact(&mut payload)?;
            return Ok(payload);
        }

        let mut buf = vec![0u8; chunk_len];
        buf[..peek_len].copy_from_slice(&prefix[..peek_len]);
        if chunk_len > peek_len {
            self.reader.read_exact(&mut buf[peek_len..])?;
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
            // Yellowjacket spec requirement: non-singleton entity_id must be >= FIRST_LOCAL_SYMBOL_ID (852)
            if ent.entity_id != crate::symbols::SINGLETON_ID
                && ent.entity_id < crate::symbols::FIRST_LOCAL_SYMBOL_ID
            {
                return Err(ContainerError::CorruptedHeader(format!(
                    "Yellowjacket spec violation: non-singleton entity_id {} must be >= {}",
                    ent.entity_id,
                    crate::symbols::FIRST_LOCAL_SYMBOL_ID
                )));
            }

            let payload_len = ent.data.len() as u64 + crate::ion::ENTY_HEADER_LEN as u64;

            // Record: entity_id (u32), chunk_type (u32), raw_offset (u64), raw_len (u64)
            entity_table_entries.write_u32::<LittleEndian>(ent.entity_id)?;
            entity_table_entries.write_u32::<LittleEndian>(ent.chunk_type)?;
            entity_table_entries.write_u64::<LittleEndian>(current_raw_offset)?;
            entity_table_entries.write_u64::<LittleEndian>(payload_len)?;

            current_raw_offset += payload_len;
        }

        if entity_table_entries.len() > u32::MAX as usize {
            return Err(ContainerError::CorruptedHeader(
                "Entity table size exceeds 4GB limit".to_string(),
            ));
        }
        let index_tab_offset: u32 = 18;
        let index_tab_length: u32 = u32::try_from(entity_table_entries.len())
            .map_err(|_| ContainerError::CorruptedHeader("index_tab_length overflow".into()))?;

        let doc_sym_bytes = doc_symbols_data.unwrap_or(&[]);
        let doc_sym_offset = index_tab_offset
            .checked_add(index_tab_length)
            .ok_or_else(|| {
                ContainerError::CorruptedHeader("Container doc_sym_offset overflow".into())
            })?;
        let doc_sym_length = u32::try_from(doc_sym_bytes.len())
            .map_err(|_| ContainerError::CorruptedHeader("doc_sym_length overflow".into()))?;

        let format_caps_bytes = format_caps_data.unwrap_or(&[]);
        let format_caps_offset = doc_sym_offset.checked_add(doc_sym_length).ok_or_else(|| {
            ContainerError::CorruptedHeader("Container format_caps_offset overflow".into())
        })?;
        let format_caps_length = u32::try_from(format_caps_bytes.len())
            .map_err(|_| ContainerError::CorruptedHeader("format_caps_length overflow".into()))?;

        let ci_offset = format_caps_offset
            .checked_add(format_caps_length)
            .ok_or_else(|| {
                ContainerError::CorruptedHeader("Container ci_offset overflow".into())
            })?;

        // Build ContainerInfo Ion structure
        let sym_bc_cont_id = KfxSymbol::Bccontid as u32;
        let sym_bc_compr_type = KfxSymbol::Bccomprtype as u32;
        let sym_bc_drm_scheme = KfxSymbol::Bcdrmscheme as u32;
        let sym_bc_chunk_size = KfxSymbol::Bcchunksize as u32;
        let sym_bc_index_tab_offset = KfxSymbol::Bcindextaboffset as u32;
        let sym_bc_index_tab_length = KfxSymbol::Bcindextablength as u32;

        let mut ci_fields_vec = vec![
            (sym_bc_cont_id, IonValue::String(container_id.to_string())),
            (sym_bc_compr_type, IonValue::Int(0)),
            (sym_bc_drm_scheme, IonValue::Int(0)),
            (sym_bc_chunk_size, IonValue::Int(4096)),
            (
                sym_bc_index_tab_offset,
                IonValue::Int(index_tab_offset as i64),
            ),
            (
                sym_bc_index_tab_length,
                IonValue::Int(index_tab_length as i64),
            ),
        ];

        if doc_sym_length > 0 {
            let sym_doc_sym_offset = KfxSymbol::Bcdocsymboloffset as u32;
            let sym_doc_sym_length = KfxSymbol::Bcdocsymbollength as u32;
            ci_fields_vec.push((sym_doc_sym_offset, IonValue::Int(doc_sym_offset as i64)));
            ci_fields_vec.push((sym_doc_sym_length, IonValue::Int(doc_sym_length as i64)));
        }

        if format_caps_length > 0 {
            let sym_fcap_offset = KfxSymbol::Bcfcapabilitiesoffset as u32;
            let sym_fcap_length = KfxSymbol::Bcfcapabilitieslength as u32;
            ci_fields_vec.push((sym_fcap_offset, IonValue::Int(format_caps_offset as i64)));
            ci_fields_vec.push((sym_fcap_length, IonValue::Int(format_caps_length as i64)));
        }

        let ci_fields = IonValue::Struct(ci_fields_vec);

        let mut ci_writer = IonWriter::new();
        ci_writer.write_bvm();
        ci_writer.write_value(&ci_fields);
        let ci_bytes = ci_writer.into_bytes();
        let ci_length = u32::try_from(ci_bytes.len())
            .map_err(|_| ContainerError::CorruptedHeader("ci_length overflow".into()))?;

        let kfxgen_info = b"[]";
        let header_length = ci_offset
            .checked_add(ci_length)
            .and_then(|v| v.checked_add(kfxgen_info.len() as u32))
            .ok_or_else(|| {
                ContainerError::CorruptedHeader("Container header_length overflow".into())
            })?;

        let start_pos = writer.stream_position()?;

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

        let end_pos = writer.stream_position()?;
        Ok(end_pos.saturating_sub(start_pos))
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
        assert!(matches!(
            result.err().unwrap(),
            ContainerError::CorruptedHeader(_)
        ));
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
        assert_eq!(
            &data[(ci_offset + ci_length) as usize..header_length as usize],
            b"[]"
        );

        // index_tab is 1 entry * 24 bytes = 24 bytes, starts at 18
        // doc_sym starts at 18 + 24 = 42
        assert_eq!(&data[42..42 + doc_sym.len()], doc_sym);

        // format_caps starts immediately after doc_sym: 42 + doc_sym.len()
        let format_caps_offset = 42 + doc_sym.len();
        assert_eq!(
            &data[format_caps_offset..format_caps_offset + format_caps.len()],
            format_caps
        );

        // ContainerInfo starts after format_caps
        assert_eq!(ci_offset as usize, format_caps_offset + format_caps.len());
    }

    #[test]
    fn test_container_open_structured_info_ignores_embedded_marker() {
        // Construct a synthetic ContainerInfo Ion payload containing an embedded [0x03, 0x9e, 0x21, 0x01]
        // in a preceding blob, followed by the true field 414 (bcIndexTabLength = 24).
        let mut w_ci = IonWriter::new();
        w_ci.write_bvm();
        w_ci.write_value(&IonValue::Struct(vec![
            (999, IonValue::Blob(vec![0x03, 0x9e, 0x21, 0x01])), // False-positive marker
            (414, IonValue::Int(24)), // True bcIndexTabLength = 24 bytes (1 chunk)
        ]));
        let ci_bytes = w_ci.into_bytes();

        let mut data = Vec::new();
        data.extend_from_slice(b"CONT");
        data.write_u16::<LittleEndian>(2).unwrap(); // version
        let header_len = 18 + 24 + ci_bytes.len() as u32 + 2;
        data.write_u32::<LittleEndian>(header_len).unwrap();
        data.write_u32::<LittleEndian>(18 + 24).unwrap(); // ci_offset
        data.write_u32::<LittleEndian>(ci_bytes.len() as u32)
            .unwrap(); // ci_length

        // bcIndexTab (24 bytes for 1 chunk: id u32, type u32, relative offset u64, length u64)
        let mut chunk_entry = vec![0u8; 24];
        chunk_entry[0..4].copy_from_slice(&852u32.to_le_bytes()); // entity_id
        chunk_entry[4..8].copy_from_slice(&145u32.to_le_bytes()); // chunk_type
        chunk_entry[8..16].copy_from_slice(&0u64.to_le_bytes()); // relative offset from header_len
        chunk_entry[16..24].copy_from_slice(&10u64.to_le_bytes()); // payload length
        data.extend_from_slice(&chunk_entry);

        // ContainerInfo
        data.extend_from_slice(&ci_bytes);
        data.extend_from_slice(b"[]");
        // Payload (10 dummy bytes)
        data.extend_from_slice(b"0123456789");

        let mut tmp_file = tempfile::NamedTempFile::new().unwrap();
        tmp_file.write_all(&data).unwrap();

        let mut opened = KfxContainer::open(tmp_file.path()).unwrap();
        assert_eq!(
            opened.chunks.len(),
            1,
            "Must parse 1 chunk from field 414 despite preceding [0x03, 0x9e]"
        );
        let chunk = opened.chunks[0].clone();
        assert_eq!(chunk.entity_id, 852);
        assert_eq!(chunk.chunk_type, 145);
        assert_eq!(opened.read_chunk(&chunk).unwrap(), b"0123456789");
    }

    #[test]
    fn test_container_rejects_small_header_and_huge_info_length() {
        // 1. header_length < 18 rejected
        let mut bad_hdr = Vec::new();
        bad_hdr.extend_from_slice(b"CONT");
        bad_hdr.write_u16::<LittleEndian>(2).unwrap();
        bad_hdr.write_u32::<LittleEndian>(12).unwrap(); // < 18
        bad_hdr.write_u32::<LittleEndian>(18).unwrap();
        bad_hdr.write_u32::<LittleEndian>(0).unwrap();
        let mut f1 = tempfile::NamedTempFile::new().unwrap();
        f1.write_all(&bad_hdr).unwrap();
        assert!(matches!(
            KfxContainer::open(f1.path()),
            Err(ContainerError::CorruptedHeader(_))
        ));

        // 2. info_length > MAX_CONTAINER_INFO_BYTES rejected
        let info_len = MAX_CONTAINER_INFO_BYTES + 1;
        let info_off = 18u32;
        let f2 = tempfile::NamedTempFile::new().unwrap();
        f2.as_file().set_len((info_off + info_len) as u64).unwrap();
        {
            let mut file = f2.as_file();
            file.seek(SeekFrom::Start(0)).unwrap();
            file.write_all(b"CONT").unwrap();
            file.write_u16::<LittleEndian>(2).unwrap();
            file.write_u32::<LittleEndian>(info_off + info_len).unwrap();
            file.write_u32::<LittleEndian>(info_off).unwrap();
            file.write_u32::<LittleEndian>(info_len).unwrap();
        }
        assert!(matches!(
            KfxContainer::open(f2.path()),
            Err(ContainerError::CorruptedHeader(_))
        ));
    }

    #[test]
    fn test_container_rejects_compressed_or_drm_info_and_honors_413() {
        let make_with_ci = |fields: Vec<(u32, IonValue)>, extra_after_ci: &[u8]| {
            let mut w_ci = IonWriter::new();
            w_ci.write_bvm();
            w_ci.write_value(&IonValue::Struct(fields));
            let ci_bytes = w_ci.into_bytes();

            let ci_offset = 18u32;
            let header_len = ci_offset + ci_bytes.len() as u32 + extra_after_ci.len() as u32;
            let mut data = Vec::new();
            data.extend_from_slice(b"CONT");
            data.write_u16::<LittleEndian>(2).unwrap();
            data.write_u32::<LittleEndian>(header_len).unwrap();
            data.write_u32::<LittleEndian>(ci_offset).unwrap();
            data.write_u32::<LittleEndian>(ci_bytes.len() as u32)
                .unwrap();
            data.extend_from_slice(&ci_bytes);
            data.extend_from_slice(extra_after_ci);
            let mut tmp = tempfile::NamedTempFile::new().unwrap();
            tmp.write_all(&data).unwrap();
            tmp
        };

        // Non-zero $410 compression rejected
        let f_comp = make_with_ci(vec![(410, IonValue::Int(1))], &[]);
        assert!(matches!(
            KfxContainer::open(f_comp.path()),
            Err(ContainerError::CorruptedHeader(_))
        ));

        // Non-zero $411 DRM rejected
        let f_drm = make_with_ci(vec![(411, IonValue::Int(2))], &[]);
        assert!(matches!(
            KfxContainer::open(f_drm.path()),
            Err(ContainerError::DrmProtected)
        ));

        // $413 bcIndexTabOffset honored when placed after ContainerInfo
        let mut w_ci = IonWriter::new();
        w_ci.write_bvm();
        w_ci.write_value(&IonValue::Struct(vec![
            (410, IonValue::Int(0)),
            (411, IonValue::Int(0)),
        ]));
        let dummy_ci_len = w_ci.into_bytes().len() as u32;
        // Build exact CI with $413 = 18 + actual_ci_len
        let mut ci_with_413 = IonWriter::new();
        ci_with_413.write_bvm();
        ci_with_413.write_value(&IonValue::Struct(vec![
            (410, IonValue::Int(0)),
            (411, IonValue::Int(0)),
            (413, IonValue::Int((18 + dummy_ci_len + 2) as i64)),
            (414, IonValue::Int(24)),
        ]));
        let ci_bytes = ci_with_413.into_bytes();
        let tab_offset = 18 + ci_bytes.len() as u32;
        // Re-encode with exact tab_offset matching ci_bytes.len()
        let mut ci_exact = IonWriter::new();
        ci_exact.write_bvm();
        ci_exact.write_value(&IonValue::Struct(vec![
            (410, IonValue::Int(0)),
            (411, IonValue::Int(0)),
            (413, IonValue::Int(tab_offset as i64)),
            (414, IonValue::Int(24)),
        ]));
        let ci_exact_bytes = ci_exact.into_bytes();
        assert_eq!(ci_exact_bytes.len(), ci_bytes.len());

        let header_len = tab_offset + 24;
        let mut data = Vec::new();
        data.extend_from_slice(b"CONT");
        data.write_u16::<LittleEndian>(2).unwrap();
        data.write_u32::<LittleEndian>(header_len).unwrap();
        data.write_u32::<LittleEndian>(18).unwrap();
        data.write_u32::<LittleEndian>(ci_exact_bytes.len() as u32)
            .unwrap();
        data.extend_from_slice(&ci_exact_bytes);
        let mut entry = vec![0u8; 24];
        entry[0..4].copy_from_slice(&900u32.to_le_bytes());
        entry[4..8].copy_from_slice(&260u32.to_le_bytes());
        entry[8..16].copy_from_slice(&0u64.to_le_bytes());
        entry[16..24].copy_from_slice(&5u64.to_le_bytes());
        data.extend_from_slice(&entry);
        data.extend_from_slice(b"HELLO");

        let mut tmp_413 = tempfile::NamedTempFile::new().unwrap();
        tmp_413.write_all(&data).unwrap();
        let mut opened = KfxContainer::open(tmp_413.path()).unwrap();
        assert_eq!(opened.chunks.len(), 1);
        assert_eq!(opened.chunks[0].entity_id, 900);
        assert_eq!(
            opened.read_chunk(&opened.chunks[0].clone()).unwrap(),
            b"HELLO"
        );
    }

    #[test]
    fn test_container_rejects_invalid_enty_header_and_low_entity_id() {
        // 1. write_to_with_symbols rejects entity_id < 852 (and != 348) at runtime
        let bad_entities = vec![KfxEntity {
            entity_id: 100,
            chunk_type: 260,
            data: b"DATA".to_vec(),
        }];
        let mut cursor = Cursor::new(Vec::new());
        assert!(matches!(
            KfxContainer::write_to_with_symbols(&bad_entities, "CR!TEST", None, None, &mut cursor),
            Err(ContainerError::CorruptedHeader(_))
        ));

        // 2. read_chunk rejects ENTY header_len < 10
        let mut bad_enty = Vec::new();
        bad_enty.extend_from_slice(b"ENTY");
        bad_enty.write_u16::<LittleEndian>(1).unwrap();
        bad_enty.write_u32::<LittleEndian>(4).unwrap(); // < 10!
        bad_enty.extend_from_slice(b"PAYLOAD");

        let entities = [KfxEntity {
            entity_id: 852,
            chunk_type: 260,
            data: Vec::new(),
        }];
        // Write raw CONT with bad_enty as chunk payload
        let mut raw = Vec::new();
        raw.extend_from_slice(b"CONT");
        raw.write_u16::<LittleEndian>(2).unwrap();
        raw.write_u32::<LittleEndian>(18 + 24).unwrap();
        raw.write_u32::<LittleEndian>(18 + 24).unwrap();
        raw.write_u32::<LittleEndian>(0).unwrap();
        raw.write_u32::<LittleEndian>(entities[0].entity_id)
            .unwrap();
        raw.write_u32::<LittleEndian>(entities[0].chunk_type)
            .unwrap();
        raw.write_u64::<LittleEndian>(0).unwrap();
        raw.write_u64::<LittleEndian>(bad_enty.len() as u64)
            .unwrap();
        raw.extend_from_slice(&bad_enty);

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&raw).unwrap();
        let mut opened = KfxContainer::open(tmp.path()).unwrap();
        assert!(matches!(
            opened.read_chunk(&opened.chunks[0].clone()),
            Err(ContainerError::CorruptedHeader(_))
        ));
    }
}
