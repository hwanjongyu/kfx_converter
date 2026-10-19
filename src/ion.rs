//! Amazon Ion Binary Serializer and ENTY Packager
//!
//! Provides zero-dependency binary Amazon Ion serialization (BVM + Type Descriptors)
//! and Amazon ENTY envelope wrapping for KFX entity payloads.

use byteorder::{LittleEndian, WriteBytesExt};

/// 4-byte Ion Binary Version Marker (Ion 1.0)
pub const ION_BVM: [u8; 4] = [0xe0, 0x01, 0x00, 0xea];

/// Magic 4-byte header for KFX entities
pub const MAGIC_ENTY: &[u8; 4] = b"ENTY";

/// Pre-serialized Ion Header for ENTY: BVM + Struct {$410: 0, $411: 0}
/// 410 = bcComprType (0 = none), 411 = bcDRMScheme (0 = none)
pub const ENTY_HEADER_ION: [u8; 11] = [0xe0, 0x01, 0x00, 0xea, 0xd6, 0x03, 0x9a, 0x20, 0x03, 0x9b, 0x20];

/// Fixed total header length for ENTY wrapper (10 bytes header + 11 bytes Ion = 21 bytes)
pub const ENTY_HEADER_LEN: u32 = 10 + ENTY_HEADER_ION.len() as u32;

/// Encodes an unsigned integer into Ion 7-bit variable-length unsigned integer (VarUInt)
/// using a zero-allocation fixed stack array `([u8; 10], usize)`.
#[inline]
pub fn encode_varuint_buf(mut val: u64) -> ([u8; 10], usize) {
    if val == 0 {
        return ([0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0], 1);
    }

    let mut tmp = [0u8; 10];
    let mut count = 0;
    while val > 0 {
        tmp[count] = (val & 0x7F) as u8;
        val >>= 7;
        count += 1;
    }

    let mut out = [0u8; 10];
    for i in 0..count {
        out[i] = tmp[count - 1 - i];
    }
    out[count - 1] |= 0x80; // Terminal byte has high bit set
    (out, count)
}

/// Encodes an unsigned integer into Ion 7-bit variable-length unsigned integer (VarUInt).
pub fn encode_varuint(val: u64) -> Vec<u8> {
    let (buf, len) = encode_varuint_buf(val);
    buf[..len].to_vec()
}

/// Encodes a signed integer into Ion variable-length signed integer (VarInt).
pub fn encode_varint(val: i64) -> Vec<u8> {
    let is_neg = val < 0;
    let mut mag = val.unsigned_abs();
    if mag < 64 {
        let b = 0x80 | (if is_neg { 0x40 } else { 0x00 }) | ((mag as u8) & 0x3F);
        return vec![b];
    }

    let mut chunks = Vec::new();
    while mag >= 64 {
        chunks.push((mag & 0x7F) as u8);
        mag >>= 7;
    }
    let high_byte = (if is_neg { 0x40 } else { 0x00 }) | ((mag as u8) & 0x3F);
    chunks.push(high_byte);
    chunks.reverse();
    if let Some(last) = chunks.last_mut() {
        *last |= 0x80;
    }
    chunks
}

/// Wraps an entity payload inside a valid 21-byte ENTY header.
/// Format:
///   0..3: b"ENTY" (4 bytes)
///   4..5: version (uint16_le, standard 1)
///   6..9: header_length (uint32_le, exactly 21)
///   10..20: header_ion (11 bytes: Ion BVM + {$410: 0, $411: 0})
///   21..: payload
pub fn wrap_enty_header(payload: &[u8], version: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(ENTY_HEADER_LEN as usize + payload.len());
    out.extend_from_slice(MAGIC_ENTY);
    out.write_u16::<LittleEndian>(version).expect("write u16");
    out.write_u32::<LittleEndian>(ENTY_HEADER_LEN).expect("write u32");
    out.extend_from_slice(&ENTY_HEADER_ION);
    out.extend_from_slice(payload);
    out
}

/// In-memory representation of an Ion value.
#[derive(Debug, Clone, PartialEq)]
pub enum IonValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Decimal(String),
    String(String),
    Symbol(u32),
    Blob(Vec<u8>),
    List(Vec<IonValue>),
    Struct(Vec<(u32, IonValue)>),
    Annotated(Vec<u32>, Box<IonValue>),
}

/// Binary Amazon Ion serializer.
#[derive(Debug, Default, Clone)]
pub struct IonWriter {
    buf: Vec<u8>,
}

impl IonWriter {
    /// Creates a new empty `IonWriter`.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Creates an `IonWriter` with pre-allocated buffer capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
        }
    }

    /// Returns the accumulated raw bytes.
    pub fn to_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Consumes the writer and returns the internal byte vector.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Writes the 4-byte Ion Binary Version Marker (1.0).
    pub fn write_bvm(&mut self) {
        self.buf.extend_from_slice(&ION_BVM);
    }

    /// Serializes an `IonValue` into binary Ion.
    pub fn write_value(&mut self, val: &IonValue) {
        match val {
            IonValue::Null => self.buf.push(0x0F),
            IonValue::Bool(b) => self.buf.push(if *b { 0x11 } else { 0x10 }),
            IonValue::Int(n) => {
                if *n >= 0 {
                    self.write_posint(*n as u64);
                } else {
                    let mag = n.unsigned_abs();
                    self.write_negint(mag);
                }
            }
            IonValue::Float(f) => self.write_float(*f),
            IonValue::Decimal(s) => self.write_decimal(s),
            IonValue::String(s) => self.write_string(s),
            IonValue::Symbol(sid) => self.write_symbol(*sid),
            IonValue::Blob(b) => self.write_blob(b),
            IonValue::List(items) => self.write_list(items),
            IonValue::Struct(fields) => self.write_struct(fields),
            IonValue::Annotated(ann, inner) => self.write_annotated_value(ann, inner),
        }
    }

    /// Serializes an Ion Decimal (type 5) with VarInt exponent and sign-magnitude coefficient.
    pub fn write_decimal(&mut self, s: &str) {
        let val: f64 = s.parse().unwrap_or(0.0);
        if val == 0.0 {
            // Zero: single byte with exponent 0
            self.write_typed_bytes(0x50, &[0x80]);
            return;
        }

        // Convert to coefficient and exponent with precision 6
        let mut coef = ((val * 1_000_000.0).round() as i64).max(i64::MIN + 1);
        let mut exp: i8 = -6;

        // Normalize trailing zeros
        while coef != 0 && coef % 10 == 0 && exp < 0 {
            coef /= 10;
            exp += 1;
        }

        let mut payload = Vec::new();

        // 1. Exponent as VarInt (signed)
        let exp_bytes = encode_varint(exp as i64);
        payload.extend_from_slice(&exp_bytes);

        // 2. Coefficient as signed Int (magnitude encoding)
        if coef != 0 {
            let is_neg_coef = coef < 0;
            let mag = coef.unsigned_abs();
            let byte_len = ((64 - mag.leading_zeros() + 7) / 8).max(1) as usize;
            let full_bytes = mag.to_be_bytes();
            let mag_bytes = &full_bytes[8 - byte_len..];

            if (mag_bytes[0] & 0x80) != 0 {
                payload.push(if is_neg_coef { 0x80 } else { 0x00 });
                payload.extend_from_slice(mag_bytes);
            } else {
                let first = if is_neg_coef { mag_bytes[0] | 0x80 } else { mag_bytes[0] };
                payload.push(first);
                payload.extend_from_slice(&mag_bytes[1..]);
            }
        }

        self.write_typed_bytes(0x50, &payload);
    }

    /// Serializes an annotated value (Type descriptor 0xEE: length + ann_length + annotations + value)
    pub fn write_annotated_value(&mut self, annotations: &[u32], val: &IonValue) {
        let mut ann_bytes = Vec::new();
        for &ann in annotations {
            ann_bytes.extend_from_slice(&encode_varuint(ann as u64));
        }
        let ann_len = encode_varuint(ann_bytes.len() as u64);

        let mut val_writer = IonWriter::new();
        val_writer.write_value(val);
        let val_bytes = val_writer.into_bytes();

        let mut payload = Vec::with_capacity(ann_len.len() + ann_bytes.len() + val_bytes.len());
        payload.extend_from_slice(&ann_len);
        payload.extend_from_slice(&ann_bytes);
        payload.extend_from_slice(&val_bytes);

        self.write_typed_bytes(0xE0, &payload);
    }

    fn write_float(&mut self, val: f64) {
        if val == 0.0 && !val.is_sign_negative() {
            self.buf.push(0x40);
            return;
        }
        self.buf.push(0x48);
        self.buf.extend_from_slice(&val.to_be_bytes());
    }

    fn write_posint(&mut self, val: u64) {
        if val == 0 {
            self.buf.push(0x20);
            return;
        }
        let (bytes, len) = Self::int_to_big_endian_buf(val);
        self.write_typed_bytes(0x20, &bytes[..len]);
    }

    fn write_negint(&mut self, magnitude: u64) {
        let (bytes, len) = Self::int_to_big_endian_buf(magnitude);
        self.write_typed_bytes(0x30, &bytes[..len]);
    }

    fn write_string(&mut self, text: &str) {
        self.write_typed_bytes(0x80, text.as_bytes());
    }

    fn write_blob(&mut self, data: &[u8]) {
        self.write_typed_bytes(0xA0, data);
    }

    fn write_symbol(&mut self, sid: u32) {
        if sid == 0 {
            self.buf.push(0x70);
            return;
        }
        let (bytes, len) = Self::int_to_big_endian_buf(sid as u64);
        self.write_typed_bytes(0x70, &bytes[..len]);
    }

    fn write_list(&mut self, items: &[IonValue]) {
        let mut inner = IonWriter::new();
        for item in items {
            inner.write_value(item);
        }
        self.write_typed_bytes(0xB0, inner.to_bytes());
    }

    fn write_struct(&mut self, fields: &[(u32, IonValue)]) {
        let mut inner = Vec::new();
        for (sid, val) in fields {
            // Field SID as VarUInt
            let (sid_bytes, sid_len) = encode_varuint_buf(*sid as u64);
            inner.extend_from_slice(&sid_bytes[..sid_len]);
            // Field Value
            let mut val_writer = IonWriter::new();
            val_writer.write_value(val);
            inner.extend_from_slice(val_writer.to_bytes());
        }
        self.write_typed_bytes(0xD0, &inner);
    }

    fn write_typed_bytes(&mut self, type_nibble: u8, payload: &[u8]) {
        let length = payload.len();
        if length < 14 {
            self.buf.push(type_nibble | (length as u8));
            self.buf.extend_from_slice(payload);
        } else {
            self.buf.push(type_nibble | 0x0E);
            let (len_bytes, len_count) = encode_varuint_buf(length as u64);
            self.buf.extend_from_slice(&len_bytes[..len_count]);
            self.buf.extend_from_slice(payload);
        }
    }

    /// Encodes a u64 into a zero-allocation fixed stack buffer `([u8; 8], usize)` in big-endian order.
    #[inline]
    pub fn int_to_big_endian_buf(val: u64) -> ([u8; 8], usize) {
        let be_bytes = val.to_be_bytes();
        let leading_zeros = be_bytes.iter().take_while(|&&b| b == 0).count();
        if leading_zeros == 8 {
            ([0; 8], 1)
        } else {
            let mut out = [0u8; 8];
            let len = 8 - leading_zeros;
            out[..len].copy_from_slice(&be_bytes[leading_zeros..]);
            (out, len)
        }
    }

    pub fn int_to_big_endian(val: u64) -> Vec<u8> {
        let (buf, len) = Self::int_to_big_endian_buf(val);
        buf[..len].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varuint_zero() {
        assert_eq!(encode_varuint(0), vec![0x80]);
    }

    #[test]
    fn test_varuint_small_integers() {
        assert_eq!(encode_varuint(1), vec![0x81]);
        assert_eq!(encode_varuint(127), vec![0xff]);
    }

    #[test]
    fn test_varuint_multi_byte_boundaries() {
        // 128 = 0x01, 0x00 with terminal bit set on 2nd byte -> [0x01, 0x80]
        assert_eq!(encode_varuint(128), vec![0x01, 0x80]);
        // 16383 (0x3FFF) -> [0x7f, 0xff]
        assert_eq!(encode_varuint(16383), vec![0x7f, 0xff]);
        // 16384 (0x4000) -> 3 bytes: 1, 0, 0 -> [0x01, 0x00, 0x80]
        assert_eq!(encode_varuint(16384), vec![0x01, 0x00, 0x80]);
    }

    #[test]
    fn test_wrap_enty_header_structure() {
        let payload = b"test_payload_12345";
        let wrapped = wrap_enty_header(payload, 1);

        assert_eq!(wrapped.len(), 21 + payload.len());
        assert_eq!(&wrapped[0..4], MAGIC_ENTY);
        // Version 1 (uint16_le)
        assert_eq!(&wrapped[4..6], &[0x01, 0x00]);
        // Header length 21 (uint32_le)
        assert_eq!(&wrapped[6..10], &[0x15, 0x00, 0x00, 0x00]);
        // Header Ion
        assert_eq!(&wrapped[10..21], &ENTY_HEADER_ION);
        // Payload
        assert_eq!(&wrapped[21..], payload);
    }

    #[test]
    fn test_writer_bvm() {
        let mut writer = IonWriter::new();
        writer.write_bvm();
        assert_eq!(writer.to_bytes(), &ION_BVM);
    }

    #[test]
    fn test_writer_primitives_parity() {
        let mut writer = IonWriter::new();
        writer.write_bvm();

        let test_struct = IonValue::Struct(vec![
            (10, IonValue::String("English".to_string())),
            (16, IonValue::Int(42)),
            (17, IonValue::Int(-15)),
            (20, IonValue::Bool(true)),
            (21, IonValue::Bool(false)),
            (22, IonValue::Null),
            (25, IonValue::Blob(b"binary_blob".to_vec())),
            (
                30,
                IonValue::List(vec![
                    IonValue::Int(1),
                    IonValue::Int(2),
                    IonValue::String("item".to_string()),
                    IonValue::Bool(true),
                ]),
            ),
            (40, IonValue::Symbol(123)),
        ]);

        writer.write_value(&test_struct);
        let raw = writer.to_bytes();

        let expected_bytes: &[u8] = &[
            224, 1, 0, 234, 222, 177, 138, 135, 69, 110, 103, 108, 105, 115, 104, 144, 33, 42, 145,
            49, 15, 148, 17, 149, 16, 150, 15, 153, 171, 98, 105, 110, 97, 114, 121, 95, 98, 108,
            111, 98, 158, 186, 33, 1, 33, 2, 132, 105, 116, 101, 109, 17, 168, 113, 123,
        ];
        assert_eq!(raw, expected_bytes);
    }

    #[test]
    fn test_ion_decimal_and_annotated_encoding() {
        let mut w1 = IonWriter::new();
        w1.write_value(&IonValue::Decimal("1.0".to_string()));
        assert_eq!(w1.to_bytes(), &[82, 128, 1]);

        let mut w2 = IonWriter::new();
        w2.write_value(&IonValue::Decimal("1.2".to_string()));
        assert_eq!(w2.to_bytes(), &[82, 193, 12]);

        let mut w3 = IonWriter::new();
        let ann = IonValue::Annotated(
            vec![593],
            Box::new(IonValue::List(vec![IonValue::Int(1), IonValue::Int(2)])),
        );
        w3.write_value(&ann);
        assert_eq!(w3.to_bytes(), &[232, 130, 4, 209, 180, 33, 1, 33, 2]);
    }
}
