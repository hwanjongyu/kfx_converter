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

/// Errors that can occur during binary Ion deserialization.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum IonError {
    #[error("Unexpected end of Ion buffer at offset {0}")]
    UnexpectedEof(usize),
    #[error("Invalid Ion binary marker")]
    InvalidBvm,
    #[error("Unsupported or invalid Ion type descriptor 0x{0:02x} at offset {1}")]
    InvalidTypeDescriptor(u8, usize),
    #[error("Malformed VarUInt at offset {0}")]
    MalformedVarUInt(usize),
    #[error("Malformed UTF-8 string at offset {0}")]
    MalformedUtf8(usize),
    #[error("Malformed Ion payload: {0}")]
    MalformedIon(String),
}

/// Decodes an unsigned integer from Ion 7-bit variable-length unsigned integer (VarUInt).
pub fn decode_varuint(buf: &[u8], offset: &mut usize) -> Result<u64, IonError> {
    let mut val: u64 = 0;
    let mut count = 0;
    while *offset < buf.len() {
        let b = buf[*offset];
        *offset += 1;
        count += 1;
        if count > 10 {
            return Err(IonError::MalformedIon(format!(
                "VarUInt sequence exceeded 10 bytes at offset {}",
                *offset
            )));
        }
        val = (val << 7) | ((b & 0x7F) as u64);
        if (b & 0x80) != 0 {
            return Ok(val);
        }
    }
    Err(IonError::UnexpectedEof(*offset))
}

/// Decodes a signed integer from Ion variable-length signed integer (VarInt).
pub fn decode_varint_slice(buf: &[u8], offset: &mut usize) -> Result<i64, IonError> {
    if *offset >= buf.len() {
        return Err(IonError::UnexpectedEof(*offset));
    }
    let first = buf[*offset];
    *offset += 1;
    let is_neg = (first & 0x40) != 0;
    let mut val: i64 = (first & 0x3F) as i64;
    if (first & 0x80) != 0 {
        return Ok(if is_neg { -val } else { val });
    }
    while *offset < buf.len() {
        let b = buf[*offset];
        *offset += 1;
        val = (val << 7) | ((b & 0x7F) as i64);
        if (b & 0x80) != 0 {
            return Ok(if is_neg { -val } else { val });
        }
    }
    Err(IonError::UnexpectedEof(*offset))
}

/// Binary Amazon Ion deserializer.
pub struct IonReader<'a> {
    buf: &'a [u8],
    offset: usize,
}

impl<'a> IonReader<'a> {
    /// Creates a new `IonReader`. Automatically skips the Ion BVM header if present.
    pub fn new(buf: &'a [u8]) -> Self {
        let mut offset = 0;
        if buf.starts_with(&ION_BVM) {
            offset = 4;
        }
        Self { buf, offset }
    }

    pub fn is_empty(&self) -> bool {
        self.offset >= self.buf.len()
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Decodes all top-level values until end of buffer.
    pub fn decode_all(&mut self) -> Result<Vec<IonValue>, IonError> {
        let mut values = Vec::new();
        while self.offset < self.buf.len() {
            values.push(self.decode_value()?);
        }
        Ok(values)
    }

    /// Decodes a single Ion value at current offset.
    pub fn decode_value(&mut self) -> Result<IonValue, IonError> {
        if self.offset >= self.buf.len() {
            return Err(IonError::UnexpectedEof(self.offset));
        }

        let start_offset = self.offset;
        let td = self.buf[self.offset];
        self.offset += 1;

        let type_code = td >> 4;
        let len_nibble = td & 0x0F;

        let content_len = if type_code == 0x0 || type_code == 0x1 {
            0
        } else if len_nibble == 0x0E {
            decode_varuint(self.buf, &mut self.offset)? as usize
        } else if len_nibble == 0x0F {
            0
        } else {
            len_nibble as usize
        };

        if len_nibble == 0x0F && type_code != 0x01 {
            return Ok(IonValue::Null);
        }

        let payload_end = self
            .offset
            .checked_add(content_len)
            .filter(|&end| end <= self.buf.len())
            .ok_or(IonError::UnexpectedEof(self.buf.len()))?;

        let payload_start = self.offset;

        let val = match type_code {
            0x0 => IonValue::Null,
            0x1 => {
                if len_nibble == 0x0F {
                    IonValue::Null
                } else {
                    IonValue::Bool(len_nibble == 1)
                }
            }
            0x2 => {
                if content_len > 8 {
                    return Err(IonError::MalformedIon(format!(
                        "Integer payload exceeds 64 bits (length {})",
                        content_len
                    )));
                }
                let mut int_val: u64 = 0;
                for i in 0..content_len {
                    int_val = (int_val << 8) | (self.buf[payload_start + i] as u64);
                }
                self.offset = payload_end;
                let signed = if int_val > i64::MAX as u64 {
                    i64::MAX
                } else {
                    int_val as i64
                };
                IonValue::Int(signed)
            }
            0x3 => {
                if content_len > 8 {
                    return Err(IonError::MalformedIon(format!(
                        "Negative integer payload exceeds 64 bits (length {})",
                        content_len
                    )));
                }
                let mut int_val: u64 = 0;
                for i in 0..content_len {
                    int_val = (int_val << 8) | (self.buf[payload_start + i] as u64);
                }
                self.offset = payload_end;
                let signed = if int_val >= (i64::MAX as u64) + 1 {
                    i64::MIN
                } else {
                    -(int_val as i64)
                };
                IonValue::Int(signed)
            }
            0x4 => {
                let f_val = if content_len == 0 {
                    0.0
                } else if content_len == 4 {
                    let mut b = [0u8; 4];
                    b.copy_from_slice(&self.buf[payload_start..payload_end]);
                    f32::from_be_bytes(b) as f64
                } else if content_len == 8 {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&self.buf[payload_start..payload_end]);
                    f64::from_be_bytes(b)
                } else {
                    0.0
                };
                self.offset = payload_end;
                IonValue::Float(f_val)
            }
            0x5 => {
                if content_len == 0 {
                    self.offset = payload_end;
                    IonValue::Decimal("0".to_string())
                } else {
                    let mut cur = payload_start;
                    let exp = decode_varint_slice(self.buf, &mut cur)?;
                    let mantissa_bytes = &self.buf[cur..payload_end];
                    let mut mantissa: i64 = 0;
                    if !mantissa_bytes.is_empty() {
                        let is_neg = (mantissa_bytes[0] & 0x80) != 0;
                        let first = mantissa_bytes[0] & 0x7F;
                        mantissa = first as i64;
                        for &b in &mantissa_bytes[1..] {
                            mantissa = (mantissa << 8) | (b as i64);
                        }
                        if is_neg {
                            mantissa = -mantissa;
                        }
                    }
                    self.offset = payload_end;
                    if exp == 0 {
                        IonValue::Decimal(format!("{}.0", mantissa))
                    } else if exp < 0 {
                        let shift = (-exp) as usize;
                        let m_str = format!("{}", mantissa.abs());
                        let sign = if mantissa < 0 { "-" } else { "" };
                        if m_str.len() <= shift {
                            let zeros = "0".repeat(shift - m_str.len());
                            IonValue::Decimal(format!("{}0.{}{}", sign, zeros, m_str))
                        } else {
                            let split_pos = m_str.len() - shift;
                            IonValue::Decimal(format!("{}{}.{}", sign, &m_str[..split_pos], &m_str[split_pos..]))
                        }
                    } else {
                        IonValue::Decimal(format!("{}e{}", mantissa, exp))
                    }
                }
            }
            0x7 => {
                let mut sid: u32 = 0;
                for i in 0..content_len {
                    sid = (sid << 8) | (self.buf[payload_start + i] as u32);
                }
                self.offset = payload_end;
                IonValue::Symbol(sid)
            }
            0x8 => {
                let s = std::str::from_utf8(&self.buf[payload_start..payload_end])
                    .map_err(|_| IonError::MalformedUtf8(payload_start))?
                    .to_string();
                self.offset = payload_end;
                IonValue::String(s)
            }
            0x9 | 0xA => {
                let b = self.buf[payload_start..payload_end].to_vec();
                self.offset = payload_end;
                IonValue::Blob(b)
            }
            0xB | 0xC => {
                self.offset = payload_start;
                let mut items = Vec::new();
                while self.offset < payload_end {
                    items.push(self.decode_value()?);
                }
                self.offset = payload_end;
                IonValue::List(items)
            }
            0xD => {
                self.offset = payload_start;
                let mut fields = Vec::new();
                while self.offset < payload_end {
                    let field_id = decode_varuint(self.buf, &mut self.offset)? as u32;
                    let field_val = self.decode_value()?;
                    fields.push((field_id, field_val));
                }
                self.offset = payload_end;
                IonValue::Struct(fields)
            }
            0xE => {
                self.offset = payload_start;
                let annot_length = decode_varuint(self.buf, &mut self.offset)? as usize;
                let annot_end = self.offset + annot_length;
                let mut annotations = Vec::new();
                while self.offset < annot_end {
                    let annot_id = decode_varuint(self.buf, &mut self.offset)? as u32;
                    annotations.push(annot_id);
                }
                let wrapped_val = self.decode_value()?;
                self.offset = payload_end;
                IonValue::Annotated(annotations, Box::new(wrapped_val))
            }
            _ => {
                return Err(IonError::InvalidTypeDescriptor(td, start_offset));
            }
        };

        Ok(val)
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

    #[test]
    fn test_ion_reader_roundtrip() {
        let mut writer = IonWriter::new();
        writer.write_bvm();

        let original_struct = IonValue::Struct(vec![
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

        writer.write_value(&original_struct);
        let raw = writer.to_bytes();

        let mut reader = IonReader::new(raw);
        let decoded = reader.decode_value().expect("Decode failed");
        assert_eq!(decoded, original_struct);

        // Test annotated value roundtrip
        let mut w2 = IonWriter::new();
        let ann = IonValue::Annotated(
            vec![593],
            Box::new(IonValue::List(vec![IonValue::Int(1), IonValue::Int(2)])),
        );
        w2.write_value(&ann);
        let raw2 = w2.to_bytes();

        let mut r2 = IonReader::new(raw2);
        let decoded2 = r2.decode_value().expect("Decode annotated failed");
        assert_eq!(decoded2, ann);

        let underline_val = IonValue::Struct(vec![
            (173, IonValue::Symbol(863)),
            (23, IonValue::Bool(true)),
        ]);
        let mut w = IonWriter::new();
        w.write_bvm();
        w.write_value(&underline_val);
        let raw = w.into_bytes();
        println!("underline_val raw bytes: {:?}", raw);
        let mut r = IonReader::new(&raw);
        let dec = r.decode_value().expect("Decode underline failed");
        assert_eq!(dec, underline_val);
    }

    #[test]
    fn test_decode_varuint_length_bound() {
        // 12 bytes with no high bit set (all 0x01)
        let endless_varuint = [0x01u8; 12];
        let mut offset = 0;
        let err = decode_varuint(&endless_varuint, &mut offset).unwrap_err();
        match err {
            IonError::MalformedIon(msg) => {
                assert!(msg.contains("VarUInt sequence exceeded 10 bytes"));
            }
            _ => panic!("Expected MalformedIon, got {:?}", err),
        }
    }

    #[test]
    fn test_ion_reader_integer_overflow_and_negation() {
        // Type 0x2 (positive int) with length 9 (> 8 bytes)
        let invalid_pos_int = [0x2E, 0x89, 0, 0, 0, 0, 0, 0, 0, 0, 0]; // len 9 via 0x0E VarUInt
        let mut r1 = IonReader::new(&invalid_pos_int);
        assert!(matches!(r1.decode_value(), Err(IonError::MalformedIon(_))));

        // Type 0x3 (negative int) with length 9 (> 8 bytes)
        let invalid_neg_int = [0x3E, 0x89, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut r2 = IonReader::new(&invalid_neg_int);
        assert!(matches!(r2.decode_value(), Err(IonError::MalformedIon(_))));

        // Type 0x3 with magnitude 2^63 (0x80 0x00 0x00 0x00 0x00 0x00 0x00 0x00), which represents -2^63 = i64::MIN
        // Must not panic with debug integer negation overflow!
        let min_neg_int = [0x38, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let mut r3 = IonReader::new(&min_neg_int);
        let val = r3.decode_value().expect("Should decode i64::MIN without negation panic");
        assert_eq!(val, IonValue::Int(i64::MIN));
    }
}
