//! Boundary defense and Ion serializer edge-case integration tests.
//! Covers `src/container.rs` and `src/ion.rs`.

mod common;

use byteorder::{LittleEndian, WriteBytesExt};
use kfx_converter::ion::decode_varuint;
use kfx_converter::{
    ContainerError, IonError, IonReader, KfxContainer, KfxEntity, encode_varuint, wrap_enty_header,
};
use std::fs::File;
use std::io::{Cursor, Write};

// =========================================================================
// 1. KFX Container Boundary & Tamper Defense Tests
// =========================================================================

#[test]
fn test_container_file_too_small_less_than_18_bytes() {
    let temp = common::temp_dir();
    let truncated_path = temp.path().join("short.kfx");
    std::fs::write(&truncated_path, b"CONT_TINY_123").unwrap();

    let err = KfxContainer::open(&truncated_path).unwrap_err();
    match err {
        ContainerError::CorruptedHeader(msg) => {
            assert!(msg.contains("File too small to be a KFX container"));
        }
        other => panic!("Expected CorruptedHeader, got {:?}", other),
    }
}

#[test]
fn test_container_drm_magic_rejection() {
    let temp = common::temp_dir();

    // Secondary DRM magic \xeaDRM
    let drm1 = temp.path().join("drm1.kfx");
    common::create_drm_kfx(&drm1, b"\xeaDRM");
    let err1 = KfxContainer::open(&drm1).unwrap_err();
    assert!(matches!(err1, ContainerError::DrmProtected));

    // Secondary DRM magic \xa8DRM
    let drm2 = temp.path().join("drm2.kfx");
    common::create_drm_kfx(&drm2, b"\xa8DRM");
    let err2 = KfxContainer::open(&drm2).unwrap_err();
    assert!(matches!(err2, ContainerError::DrmProtected));
}

#[test]
fn test_container_invalid_magic_bytes() {
    let temp = common::temp_dir();
    let invalid = temp.path().join("invalid_magic.kfx");
    let mut data = vec![b'P', b'K', 0x03, 0x04];
    data.extend_from_slice(&[0u8; 32]);
    std::fs::write(&invalid, &data).unwrap();

    let err = KfxContainer::open(&invalid).unwrap_err();
    match err {
        ContainerError::InvalidMagic(m) => {
            assert_eq!(&m, b"PK\x03\x04");
        }
        other => panic!("Expected InvalidMagic, got {:?}", other),
    }
}

#[test]
fn test_container_header_length_exceeds_file_size() {
    let temp = common::temp_dir();
    let path = temp.path().join("huge_header_len.kfx");

    let mut buf = Vec::new();
    buf.extend_from_slice(b"CONT");
    buf.write_u16::<LittleEndian>(2).unwrap(); // version
    buf.write_u32::<LittleEndian>(500).unwrap(); // header_length: 500 > file size 24
    buf.write_u32::<LittleEndian>(18).unwrap(); // info_offset
    buf.write_u32::<LittleEndian>(0).unwrap(); // info_length
    buf.extend_from_slice(&[0u8; 6]); // total size: 24 bytes

    std::fs::write(&path, &buf).unwrap();

    let err = KfxContainer::open(&path).unwrap_err();
    match err {
        ContainerError::CorruptedHeader(msg) => {
            assert!(msg.contains("exceeds file size"));
        }
        other => panic!("Expected CorruptedHeader, got {:?}", other),
    }
}

#[test]
fn test_container_info_bounds_exceed_file_size() {
    let temp = common::temp_dir();
    let path = temp.path().join("info_bounds_overflow.kfx");

    let mut buf = Vec::new();
    buf.extend_from_slice(b"CONT");
    buf.write_u16::<LittleEndian>(2).unwrap();
    buf.write_u32::<LittleEndian>(18).unwrap();
    buf.write_u32::<LittleEndian>(20).unwrap(); // info_offset = 20
    buf.write_u32::<LittleEndian>(50).unwrap(); // info_length = 50 (sum = 70 > 30 bytes)
    buf.extend_from_slice(&[0u8; 12]); // total size: 30 bytes

    std::fs::write(&path, &buf).unwrap();

    let err = KfxContainer::open(&path).unwrap_err();
    match err {
        ContainerError::CorruptedHeader(msg) => {
            assert!(msg.contains("ContainerInfo bounds"));
        }
        other => panic!("Expected CorruptedHeader, got {:?}", other),
    }
}

#[test]
fn test_container_chunk_count_exceeds_safety_limit_50000() {
    let temp = common::temp_dir();
    let path = temp.path().join("chunk_count_bomb.kfx");

    // Safety limit is 50,000 chunks.
    // 50,001 chunks * 24 bytes = 1,200,024 bytes
    // info_offset = 18 + 1,200,024 = 1,200,042
    let info_offset: u32 = 18 + 50_001 * 24;

    let file = File::create(&path).unwrap();
    // Allocate sparse file so tests run instantly without disk usage
    file.set_len(info_offset as u64 + 100).unwrap();

    let mut file = File::options().write(true).open(&path).unwrap();
    file.write_all(b"CONT").unwrap();
    file.write_u16::<LittleEndian>(2).unwrap();
    file.write_u32::<LittleEndian>(18).unwrap();
    file.write_u32::<LittleEndian>(info_offset).unwrap();
    file.write_u32::<LittleEndian>(0).unwrap();

    let err = KfxContainer::open(&path).unwrap_err();
    match err {
        ContainerError::CorruptedHeader(msg) => {
            assert!(
                msg.contains("exceeds safety limit of 50,000"),
                "Should reject >50k chunks: {}",
                msg
            );
        }
        other => panic!("Expected CorruptedHeader, got {:?}", other),
    }
}

#[test]
fn test_container_read_chunk_exceeds_500mb_safety_limit() {
    let temp = common::temp_dir();
    let path = temp.path().join("huge_chunk.kfx");

    let chunk_len = 501 * 1024 * 1024u64; // 501 MB > 500 MB limit
    let total_file_size = 600 * 1024 * 1024u64;

    // Create sparse file
    let file = File::create(&path).unwrap();
    file.set_len(total_file_size).unwrap();

    let mut file = File::options().write(true).open(&path).unwrap();
    file.write_all(b"CONT").unwrap();
    file.write_u16::<LittleEndian>(2).unwrap();
    file.write_u32::<LittleEndian>(18 + 24).unwrap(); // header_length
    file.write_u32::<LittleEndian>(18 + 24).unwrap(); // info_offset
    file.write_u32::<LittleEndian>(0).unwrap(); // info_length

    // Write 1 chunk index entry (24 bytes)
    file.write_u32::<LittleEndian>(852).unwrap(); // entity_id
    file.write_u32::<LittleEndian>(260).unwrap(); // chunk_type
    file.write_u64::<LittleEndian>(0).unwrap(); // offset (relative to header_length)
    file.write_u64::<LittleEndian>(chunk_len).unwrap(); // length = 501MB

    let mut container = KfxContainer::open(&path).expect("Container header should open");
    assert_eq!(container.chunks.len(), 1);

    let err = container
        .read_chunk(&container.chunks[0].clone())
        .unwrap_err();
    match err {
        ContainerError::CorruptedHeader(msg) => {
            assert!(msg.contains("exceeds 500MB safety limit"));
        }
        other => panic!("Expected CorruptedHeader for 500MB limit, got {:?}", other),
    }
}

#[test]
fn test_container_read_chunk_offset_overflow_defense() {
    let temp = common::temp_dir();
    let path = temp.path().join("offset_overflow.kfx");

    let mut buf = Vec::new();
    buf.extend_from_slice(b"CONT");
    buf.write_u16::<LittleEndian>(2).unwrap();
    buf.write_u32::<LittleEndian>(18 + 24).unwrap();
    buf.write_u32::<LittleEndian>(18 + 24).unwrap();
    buf.write_u32::<LittleEndian>(0).unwrap();

    // 1 chunk entry where offset + length > file_size
    buf.write_u32::<LittleEndian>(852).unwrap();
    buf.write_u32::<LittleEndian>(260).unwrap();
    buf.write_u64::<LittleEndian>(1000).unwrap(); // offset 1000
    buf.write_u64::<LittleEndian>(500).unwrap(); // length 500 (total > file size)
    std::fs::write(&path, &buf).unwrap();

    let mut container = KfxContainer::open(&path).expect("Header should parse");
    let err = container
        .read_chunk(&container.chunks[0].clone())
        .unwrap_err();
    match err {
        ContainerError::CorruptedHeader(msg) => {
            assert!(msg.contains("exceeds file size"));
        }
        other => panic!("Expected CorruptedHeader, got {:?}", other),
    }
}

#[test]
fn test_container_read_chunk_corrupted_enty_header_length() {
    let temp = common::temp_dir();
    let path = temp.path().join("corrupted_enty.kfx");

    // Payload starts with ENTY but declares header_len = 500 when chunk length is only 30 bytes
    let mut bad_enty = Vec::new();
    bad_enty.extend_from_slice(b"ENTY");
    bad_enty.write_u16::<LittleEndian>(1).unwrap();
    bad_enty.write_u32::<LittleEndian>(500).unwrap(); // header_length = 500 > chunk buffer length 30
    bad_enty.extend_from_slice(&[0u8; 20]); // total chunk length: 30 bytes

    let chunk_offset = 18 + 24;
    let mut file_buf = Vec::new();
    file_buf.extend_from_slice(b"CONT");
    file_buf.write_u16::<LittleEndian>(2).unwrap();
    file_buf
        .write_u32::<LittleEndian>(chunk_offset as u32)
        .unwrap();
    file_buf
        .write_u32::<LittleEndian>(chunk_offset as u32)
        .unwrap();
    file_buf.write_u32::<LittleEndian>(0).unwrap();

    file_buf.write_u32::<LittleEndian>(852).unwrap();
    file_buf.write_u32::<LittleEndian>(260).unwrap();
    file_buf.write_u64::<LittleEndian>(0).unwrap();
    file_buf
        .write_u64::<LittleEndian>(bad_enty.len() as u64)
        .unwrap();
    file_buf.extend_from_slice(&bad_enty);

    std::fs::write(&path, &file_buf).unwrap();

    let mut container = KfxContainer::open(&path).unwrap();
    let err = container
        .read_chunk(&container.chunks[0].clone())
        .unwrap_err();
    match err {
        ContainerError::CorruptedHeader(msg) => {
            assert!(msg.contains("ENTY chunk header length (500) exceeds chunk buffer length"));
        }
        other => panic!(
            "Expected CorruptedHeader for ENTY length mismatch, got {:?}",
            other
        ),
    }
}

#[test]
fn test_container_write_to_with_symbols_and_format_caps() {
    let entities = vec![KfxEntity {
        entity_id: 852,
        chunk_type: 260,
        data: b"STORY_PAYLOAD".to_vec(),
    }];

    let doc_symbols = b"DUMMY_DOC_SYMBOLS";
    let format_caps = b"DUMMY_FORMAT_CAPS";

    let mut cursor = Cursor::new(Vec::new());
    let written = KfxContainer::write_to_with_symbols(
        &entities,
        "CR!TESTCONTAINER",
        Some(doc_symbols),
        Some(format_caps),
        &mut cursor,
    )
    .expect("write_to_with_symbols should succeed");

    assert!(written > 0);
    let bytes = cursor.into_inner();
    assert_eq!(&bytes[0..4], b"CONT");
}

// =========================================================================
// 2. Ion Serializer & Deserializer Edge-Case Tests
// =========================================================================

#[test]
fn test_ion_varuint_boundaries_and_overflow() {
    // Test 0 -> [0x80]
    let enc0 = encode_varuint(0);
    assert_eq!(enc0, vec![0x80]);
    let mut offset = 0;
    assert_eq!(decode_varuint(&enc0, &mut offset).unwrap(), 0);

    // Test 127 -> [0xFF]
    let enc127 = encode_varuint(127);
    assert_eq!(enc127, vec![0xFF]);
    let mut offset = 0;
    assert_eq!(decode_varuint(&enc127, &mut offset).unwrap(), 127);

    // Test 128 -> [0x01, 0x80]
    let enc128 = encode_varuint(128);
    assert_eq!(enc128, vec![0x01, 0x80]);
    let mut offset = 0;
    assert_eq!(decode_varuint(&enc128, &mut offset).unwrap(), 128);

    // Test u64::MAX roundtrip
    let enc_max = encode_varuint(u64::MAX);
    let mut offset = 0;
    assert_eq!(decode_varuint(&enc_max, &mut offset).unwrap(), u64::MAX);

    // Test sequence exceeding 10 bytes without terminal bit
    let overflow_bytes = [0x01u8; 11];
    let mut offset = 0;
    let err = decode_varuint(&overflow_bytes, &mut offset).unwrap_err();
    assert!(matches!(err, IonError::MalformedIon(_)));
}

#[test]
fn test_ion_varint_i64_min_and_boundaries() {
    use kfx_converter::ion::{decode_varint_slice, encode_varint};

    // Test i64::MIN roundtrip
    let enc_min = encode_varint(i64::MIN);
    let mut offset = 0;
    let dec_min = decode_varint_slice(&enc_min, &mut offset).expect("i64::MIN must decode cleanly");
    assert_eq!(dec_min, i64::MIN);

    // Test i64::MAX roundtrip
    let enc_max = encode_varint(i64::MAX);
    let mut offset = 0;
    let dec_max = decode_varint_slice(&enc_max, &mut offset).expect("i64::MAX must decode cleanly");
    assert_eq!(dec_max, i64::MAX);

    // Test -1, 0, 1
    for val in [-1i64, 0, 1, -63, 63, -64, 64] {
        let enc = encode_varint(val);
        let mut offset = 0;
        let dec = decode_varint_slice(&enc, &mut offset).unwrap();
        assert_eq!(dec, val);
    }

    // Test VarInt exceeding 10 bytes
    let overflow_varint = [0x01u8; 11];
    let mut offset = 0;
    let err = decode_varint_slice(&overflow_varint, &mut offset).unwrap_err();
    assert!(matches!(err, IonError::MalformedIon(_)));
}

#[test]
fn test_ion_recursion_depth_limit_128() {
    use kfx_converter::IonValue;
    use kfx_converter::IonWriter;

    // Construct Ion payload nested deeper than MAX_RECURSION_DEPTH (128)
    let mut val = IonValue::Int(1);
    for _ in 0..130 {
        val = IonValue::List(vec![val]);
    }
    let mut writer = IonWriter::new();
    writer.write_value(&val);
    let payload = writer.to_bytes();

    let mut reader = IonReader::new(payload);
    let err = reader.decode_value().unwrap_err();
    match err {
        IonError::MalformedIon(msg) => {
            assert!(
                msg.contains("Recursion depth limit exceeded"),
                "Expected recursion depth limit error: {}",
                msg
            );
        }
        other => panic!("Expected MalformedIon for recursion limit, got {:?}", other),
    }
}

#[test]
fn test_ion_non_utf8_string_returns_malformed_utf8() {
    // String type descriptor: 0x80 | length
    // Type descriptor 0x83 (String of length 3) followed by invalid UTF-8 bytes [0xFF, 0xFE, 0xFD]
    let bad_utf8 = vec![0x83, 0xFF, 0xFE, 0xFD];
    let mut reader = IonReader::new(&bad_utf8);

    let err = reader.decode_value().unwrap_err();
    match err {
        IonError::MalformedUtf8(offset) => {
            assert_eq!(offset, 1);
        }
        other => panic!("Expected MalformedUtf8, got {:?}", other),
    }
}

#[test]
fn test_ion_unexpected_eof_on_truncated_buffer() {
    // String descriptor declaring length 10, but only 2 bytes provided
    let truncated = vec![0x8A, b'a', b'b'];
    let mut reader = IonReader::new(&truncated);
    let err = reader.decode_value().unwrap_err();
    assert!(matches!(err, IonError::UnexpectedEof(_)));
}

#[test]
fn test_wrap_enty_header_roundtrip() {
    let payload = b"ION_PAYLOAD_HERE";
    let wrapped = wrap_enty_header(payload, 1);

    assert_eq!(&wrapped[0..4], b"ENTY");
    assert_eq!(wrapped.len(), 21 + payload.len());
    assert_eq!(&wrapped[21..], payload);
}
