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
pub const ENTY_HEADER_ION: [u8; 11] = [
    0xe0, 0x01, 0x00, 0xea, 0xd6, 0x03, 0x9a, 0x20, 0x03, 0x9b, 0x20,
];

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
    out.write_u32::<LittleEndian>(ENTY_HEADER_LEN)
        .expect("write u32");
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

/// Parses a decimal string exactly into (coefficient, exponent) without floating-point precision loss.
pub fn parse_decimal_exact(s: &str) -> Option<(i64, i8)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (is_neg, s) = if let Some(stripped) = s.strip_prefix('-') {
        (true, stripped)
    } else if let Some(stripped) = s.strip_prefix('+') {
        (false, stripped)
    } else {
        (false, s)
    };

    let (base_str, exp_str) = match s.split_once(['e', 'E']) {
        Some((b, e)) => (b, Some(e)),
        None => (s, None),
    };
    let exp_offset: i32 = if let Some(es) = exp_str {
        es.parse::<i32>().ok()?
    } else {
        0
    };

    let (int_part, frac_part) = match base_str.split_once('.') {
        Some((i, f)) => (i, f),
        None => (base_str, ""),
    };

    if !int_part.is_empty() && !int_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !frac_part.is_empty() && !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }

    let combined = format!("{}{}", int_part, frac_part);
    let trimmed = combined.trim_start_matches('0');
    if trimmed.is_empty() {
        return Some((0, 0));
    }

    let mag: u64 = trimmed.parse().ok()?;
    let mut coef: i64 = if is_neg {
        if mag == (i64::MAX as u64) + 1 {
            i64::MIN
        } else {
            i64::try_from(mag).ok()?.checked_neg()?
        }
    } else {
        i64::try_from(mag).ok()?
    };

    let frac_len_i32 = i32::try_from(frac_part.len()).ok()?.checked_neg()?;
    let mut exp: i32 = frac_len_i32.checked_add(exp_offset)?;

    // Normalize trailing zeros: e.g. 1.0 -> coef 10, exp -1 -> coef 1, exp 0
    while coef != 0 && coef % 10 == 0 {
        coef /= 10;
        exp = exp.checked_add(1)?;
    }

    let exp_i8 = i8::try_from(exp).ok()?;
    Some((coef, exp_i8))
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
        let (coef, exp) = if let Some(parsed) = parse_decimal_exact(s) {
            parsed
        } else {
            let val: f64 = s.parse().unwrap_or(0.0);
            if val == 0.0 || !val.is_finite() {
                self.write_typed_bytes(0x50, &[0x80]);
                return;
            }
            let scaled = val * 1_000_000.0;
            let mut c = if scaled >= i64::MAX as f64 {
                i64::MAX
            } else if scaled <= (i64::MIN + 1) as f64 {
                i64::MIN + 1
            } else {
                scaled.round() as i64
            };
            let mut e: i8 = -6;
            while c != 0 && c % 10 == 0 && e < 0 {
                c /= 10;
                e += 1;
            }
            (c, e)
        };

        if coef == 0 {
            // Zero: single byte with exponent 0 (0x80 in VarInt)
            self.write_typed_bytes(0x50, &[0x80]);
            return;
        }

        let mut payload = Vec::new();

        // 1. Exponent as VarInt (signed)
        let exp_bytes = encode_varint(exp as i64);
        payload.extend_from_slice(&exp_bytes);

        // 2. Coefficient as signed Int (magnitude encoding)
        if coef != 0 {
            let is_neg_coef = coef < 0;
            let mag = coef.unsigned_abs();
            let byte_len = (64 - mag.leading_zeros()).div_ceil(8).max(1) as usize;
            let full_bytes = mag.to_be_bytes();
            let mag_bytes = &full_bytes[8 - byte_len..];

            if (mag_bytes[0] & 0x80) != 0 {
                payload.push(if is_neg_coef { 0x80 } else { 0x00 });
                payload.extend_from_slice(mag_bytes);
            } else {
                let first = if is_neg_coef {
                    mag_bytes[0] | 0x80
                } else {
                    mag_bytes[0]
                };
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
        let mut inner = IonWriter::with_capacity(fields.len() * 24);
        for (sid, val) in fields {
            // Field SID as VarUInt
            let (sid_bytes, sid_len) = encode_varuint_buf(*sid as u64);
            inner.buf.extend_from_slice(&sid_bytes[..sid_len]);
            // Field Value serialized directly into inner buffer without per-field heap allocations
            inner.write_value(val);
        }
        self.write_typed_bytes(0xD0, inner.to_bytes());
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
    decode_varuint_bounded(buf, offset, buf.len())
}

fn decode_varuint_bounded(buf: &[u8], offset: &mut usize, limit: usize) -> Result<u64, IonError> {
    let end = limit.min(buf.len());
    let mut val: u64 = 0;
    let mut count = 0usize;
    while *offset < end {
        let b = buf[*offset];
        *offset += 1;
        count += 1;
        if count > 10 {
            return Err(IonError::MalformedIon(format!(
                "VarUInt sequence exceeded 10 bytes or overflowed at offset {}",
                *offset
            )));
        }
        val = val.checked_mul(128).ok_or_else(|| {
            IonError::MalformedIon(format!(
                "VarUInt sequence exceeded 10 bytes or overflowed at offset {}",
                *offset
            ))
        })?;
        val |= u64::from(b & 0x7F);
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
    let mut mag: u64 = u64::from(first & 0x3F);
    if (first & 0x80) != 0 {
        let val =
            i64::try_from(mag).map_err(|_| IonError::MalformedIon("VarInt overflow".into()))?;
        return if is_neg {
            val.checked_neg()
                .ok_or_else(|| IonError::MalformedIon("VarInt underflow".into()))
        } else {
            Ok(val)
        };
    }
    let mut count = 1usize;
    while *offset < buf.len() {
        count += 1;
        if count > 10 {
            return Err(IonError::MalformedIon(
                "VarInt exceeds 10 bytes or overflows".into(),
            ));
        }
        let b = buf[*offset];
        *offset += 1;
        mag = mag
            .checked_mul(128)
            .ok_or_else(|| IonError::MalformedIon("VarInt shift overflow".into()))?;
        mag |= u64::from(b & 0x7F);
        if (b & 0x80) != 0 {
            if is_neg {
                if mag == (i64::MAX as u64) + 1 {
                    return Ok(i64::MIN);
                }
                let pos = i64::try_from(mag)
                    .map_err(|_| IonError::MalformedIon("VarInt underflow".into()))?;
                return pos
                    .checked_neg()
                    .ok_or_else(|| IonError::MalformedIon("VarInt underflow".into()));
            } else {
                return i64::try_from(mag)
                    .map_err(|_| IonError::MalformedIon("VarInt overflow".into()));
            }
        }
    }
    Err(IonError::UnexpectedEof(*offset))
}

#[inline]
fn is_nop_pad(td: u8) -> bool {
    (td >> 4) == 0 && td != 0x0F
}

/// Binary Amazon Ion deserializer.
pub struct IonReader<'a> {
    buf: &'a [u8],
    offset: usize,
    depth: usize,
}

impl<'a> IonReader<'a> {
    pub const MAX_RECURSION_DEPTH: usize = 128;

    /// Creates a new `IonReader`. Automatically skips the Ion BVM header if present.
    pub fn new(buf: &'a [u8]) -> Self {
        let mut offset = 0;
        if buf.starts_with(&ION_BVM) {
            offset = 4;
        }
        Self {
            buf,
            offset,
            depth: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.offset >= self.buf.len()
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    fn skip_single_nop_pad(&mut self, limit: usize) -> Result<bool, IonError> {
        if self.offset >= limit {
            return Ok(false);
        }
        let td = self.buf[self.offset];
        if !is_nop_pad(td) {
            return Ok(false);
        }
        self.offset += 1;
        let pad_len = if td == 0x00 {
            0usize
        } else if td <= 0x0D {
            usize::from(td)
        } else {
            let len_u64 = decode_varuint_bounded(self.buf, &mut self.offset, limit)?;
            usize::try_from(len_u64).map_err(|_| {
                IonError::MalformedIon(format!("NOP pad length {} exceeds usize::MAX", len_u64))
            })?
        };
        let pad_end = self
            .offset
            .checked_add(pad_len)
            .filter(|&end| end <= limit)
            .ok_or(IonError::UnexpectedEof(limit))?;
        self.offset = pad_end;
        Ok(true)
    }

    fn skip_nop_pads(&mut self, limit: usize) -> Result<(), IonError> {
        while self.skip_single_nop_pad(limit)? {}
        Ok(())
    }

    /// Decodes all top-level values until end of buffer.
    pub fn decode_all(&mut self) -> Result<Vec<IonValue>, IonError> {
        let mut values = Vec::new();
        let limit = self.buf.len();
        loop {
            self.skip_nop_pads(limit)?;
            if self.offset >= limit {
                break;
            }
            values.push(self.decode_value()?);
        }
        Ok(values)
    }

    /// Decodes a single Ion value at current offset.
    pub fn decode_value(&mut self) -> Result<IonValue, IonError> {
        self.decode_value_bounded(self.buf.len())
    }

    /// Decodes a single Ion value strictly bounded within `limit` bytes.
    fn decode_value_bounded(&mut self, limit: usize) -> Result<IonValue, IonError> {
        if self.depth > Self::MAX_RECURSION_DEPTH {
            return Err(IonError::MalformedIon(
                "Recursion depth limit exceeded".into(),
            ));
        }
        self.skip_nop_pads(limit)?;
        if self.offset >= limit {
            return Err(IonError::UnexpectedEof(self.offset));
        }

        let start_offset = self.offset;
        let td = self.buf[self.offset];
        self.offset += 1;

        let type_code = td >> 4;
        let len_nibble = td & 0x0F;

        if type_code == 0xF || (type_code == 0xE && len_nibble == 0x0F) {
            return Err(IonError::InvalidTypeDescriptor(td, start_offset));
        }
        if type_code == 0x1 && !matches!(len_nibble, 0x0 | 0x1 | 0xF) {
            return Err(IonError::InvalidTypeDescriptor(td, start_offset));
        }

        let content_len = if type_code == 0x0 || type_code == 0x1 {
            0
        } else if len_nibble == 0x0E || (type_code == 0xD && len_nibble == 0x01) {
            let len_u64 = decode_varuint_bounded(self.buf, &mut self.offset, limit)?;
            usize::try_from(len_u64).map_err(|_| {
                IonError::MalformedIon(format!("Ion value length {} exceeds usize::MAX", len_u64))
            })?
        } else if len_nibble == 0x0F {
            0
        } else {
            usize::from(len_nibble)
        };

        if len_nibble == 0x0F && type_code != 0x01 {
            return Ok(IonValue::Null);
        }

        let payload_end = self
            .offset
            .checked_add(content_len)
            .filter(|&end| end <= limit)
            .ok_or(IonError::UnexpectedEof(limit))?;

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
                    int_val = (int_val << 8) | u64::from(self.buf[payload_start + i]);
                }
                self.offset = payload_end;
                let signed = i64::try_from(int_val).map_err(|_| {
                    IonError::MalformedIon(format!(
                        "Positive integer {} overflows i64::MAX",
                        int_val
                    ))
                })?;
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
                    int_val = (int_val << 8) | u64::from(self.buf[payload_start + i]);
                }
                self.offset = payload_end;
                let signed = if int_val == (i64::MAX as u64) + 1 {
                    i64::MIN
                } else {
                    let pos = i64::try_from(int_val).map_err(|_| {
                        IonError::MalformedIon(format!(
                            "Negative integer -{} overflows i64::MIN",
                            int_val
                        ))
                    })?;
                    pos.checked_neg().ok_or_else(|| {
                        IonError::MalformedIon("Negative integer negation overflow".into())
                    })?
                };
                IonValue::Int(signed)
            }
            0x4 => {
                let f_val = if content_len == 0 {
                    0.0
                } else if content_len == 4 {
                    let mut b = [0u8; 4];
                    b.copy_from_slice(&self.buf[payload_start..payload_end]);
                    f64::from(f32::from_be_bytes(b))
                } else if content_len == 8 {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&self.buf[payload_start..payload_end]);
                    f64::from_be_bytes(b)
                } else {
                    return Err(IonError::MalformedIon(format!(
                        "Invalid Ion float length: {}",
                        content_len
                    )));
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
                    if cur > payload_end {
                        return Err(IonError::MalformedIon(format!(
                            "Decimal exponent VarInt extends beyond payload boundary: cur {} > end {}",
                            cur, payload_end
                        )));
                    }
                    let mantissa_bytes = &self.buf[cur..payload_end];
                    if mantissa_bytes.len() > 8 {
                        return Err(IonError::MalformedIon(
                            "Mantissa length exceeds 8 bytes".into(),
                        ));
                    }
                    let mut mantissa: i64 = 0;
                    if !mantissa_bytes.is_empty() {
                        let is_neg = (mantissa_bytes[0] & 0x80) != 0;
                        let first = mantissa_bytes[0] & 0x7F;
                        let mut mag: u64 = u64::from(first);
                        for &b in &mantissa_bytes[1..] {
                            mag = (mag << 8) | u64::from(b);
                        }
                        if is_neg {
                            if mag == (i64::MAX as u64) + 1 {
                                mantissa = i64::MIN;
                            } else {
                                let pos = i64::try_from(mag).map_err(|_| {
                                    IonError::MalformedIon("Mantissa underflow".into())
                                })?;
                                mantissa = pos.checked_neg().ok_or_else(|| {
                                    IonError::MalformedIon("Mantissa underflow".into())
                                })?;
                            }
                        } else {
                            mantissa = i64::try_from(mag)
                                .map_err(|_| IonError::MalformedIon("Mantissa overflow".into()))?;
                        }
                    }
                    self.offset = payload_end;
                    if exp == 0 {
                        IonValue::Decimal(format!("{}.0", mantissa))
                    } else if exp < 0 {
                        let neg_exp = exp.checked_neg().ok_or_else(|| {
                            IonError::MalformedIon("Decimal exponent negation overflow".into())
                        })?;
                        let shift = usize::try_from(neg_exp).map_err(|_| {
                            IonError::MalformedIon("Decimal shift exceeds usize::MAX".into())
                        })?;
                        if shift > 64 {
                            return Err(IonError::MalformedIon(
                                "Decimal shift exceeds bound".into(),
                            ));
                        }
                        let m_str = format!("{}", mantissa.unsigned_abs());
                        let sign = if mantissa < 0 { "-" } else { "" };
                        if m_str.len() <= shift {
                            let zeros = "0".repeat(shift - m_str.len());
                            IonValue::Decimal(format!("{}0.{}{}", sign, zeros, m_str))
                        } else {
                            let split_pos = m_str.len() - shift;
                            IonValue::Decimal(format!(
                                "{}{}.{}",
                                sign,
                                &m_str[..split_pos],
                                &m_str[split_pos..]
                            ))
                        }
                    } else {
                        if exp > 1000 {
                            return Err(IonError::MalformedIon(
                                "Decimal positive exponent exceeds bound".into(),
                            ));
                        }
                        IonValue::Decimal(format!("{}e{}", mantissa, exp))
                    }
                }
            }
            0x7 => {
                if content_len > 4 {
                    return Err(IonError::MalformedIon(format!(
                        "Symbol ID payload exceeds 4 bytes (length {})",
                        content_len
                    )));
                }
                let mut sid: u32 = 0;
                for i in 0..content_len {
                    sid = (sid << 8) | u32::from(self.buf[payload_start + i]);
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
                self.depth += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_nop_pads(payload_end)?;
                    if self.offset >= payload_end {
                        break;
                    }
                    items.push(self.decode_value_bounded(payload_end)?);
                }
                self.depth -= 1;
                if self.offset != payload_end {
                    return Err(IonError::MalformedIon(format!(
                        "Ion container child values length mismatch: offset {} != end {}",
                        self.offset, payload_end
                    )));
                }
                IonValue::List(items)
            }
            0xD => {
                self.offset = payload_start;
                self.depth += 1;
                let mut fields = Vec::new();
                while self.offset < payload_end {
                    let field_id_u64 =
                        decode_varuint_bounded(self.buf, &mut self.offset, payload_end)?;
                    let field_id = u32::try_from(field_id_u64).map_err(|_| {
                        IonError::MalformedIon(format!(
                            "Struct field SID {} exceeds u32::MAX",
                            field_id_u64
                        ))
                    })?;
                    if self.offset < payload_end && is_nop_pad(self.buf[self.offset]) {
                        self.skip_single_nop_pad(payload_end)?;
                        continue;
                    }
                    let field_val = self.decode_value_bounded(payload_end)?;
                    fields.push((field_id, field_val));
                }
                self.depth -= 1;
                if self.offset != payload_end {
                    return Err(IonError::MalformedIon(format!(
                        "Ion struct child values length mismatch: offset {} != end {}",
                        self.offset, payload_end
                    )));
                }
                IonValue::Struct(fields)
            }
            0xE => {
                self.offset = payload_start;
                let annot_len_u64 =
                    decode_varuint_bounded(self.buf, &mut self.offset, payload_end)?;
                let annot_length = usize::try_from(annot_len_u64).map_err(|_| {
                    IonError::MalformedIon(format!(
                        "Annotation length {} exceeds usize::MAX",
                        annot_len_u64
                    ))
                })?;
                if annot_length == 0 {
                    return Err(IonError::MalformedIon(
                        "Annotation length must be greater than zero".into(),
                    ));
                }
                let annot_end = self
                    .offset
                    .checked_add(annot_length)
                    .filter(|&end| end <= payload_end)
                    .ok_or(IonError::UnexpectedEof(payload_end))?;
                let mut annotations = Vec::new();
                while self.offset < annot_end {
                    let annot_id_u64 =
                        decode_varuint_bounded(self.buf, &mut self.offset, annot_end)?;
                    let annot_id = u32::try_from(annot_id_u64).map_err(|_| {
                        IonError::MalformedIon(format!(
                            "Annotation SID {} exceeds u32::MAX",
                            annot_id_u64
                        ))
                    })?;
                    annotations.push(annot_id);
                }
                if self.offset != annot_end || annotations.is_empty() {
                    return Err(IonError::MalformedIon(
                        "Malformed Ion annotation symbol sequence".into(),
                    ));
                }
                self.depth += 1;
                let wrapped_val = self.decode_value_bounded(payload_end)?;
                self.depth -= 1;
                if matches!(wrapped_val, IonValue::Annotated(_, _)) {
                    return Err(IonError::MalformedIon(
                        "Nested Ion annotation wrappers are not permitted".into(),
                    ));
                }
                if self.offset != payload_end {
                    return Err(IonError::MalformedIon(format!(
                        "Ion annotated child value length mismatch: offset {} != end {}",
                        self.offset, payload_end
                    )));
                }
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

        let mut w4 = IonWriter::new();
        w4.write_value(&IonValue::Decimal("1.35".to_string()));
        let mut reader = IonReader::new(w4.to_bytes());
        let val = reader.decode_value().unwrap();
        assert_eq!(val, IonValue::Decimal("1.35".to_string()));

        let mut w5 = IonWriter::new();
        w5.write_value(&IonValue::Decimal("-0.05".to_string()));
        let mut reader2 = IonReader::new(w5.to_bytes());
        let val2 = reader2.decode_value().unwrap();
        assert_eq!(val2, IonValue::Decimal("-0.05".to_string()));

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
        let val = r3
            .decode_value()
            .expect("Should decode i64::MIN without negation panic");
        assert_eq!(val, IonValue::Int(i64::MIN));
    }

    #[test]
    fn test_ion_recursion_depth_limit() {
        // Construct properly nested Ion lists exceeding MAX_RECURSION_DEPTH (128)
        let mut data = vec![0x20]; // innermost int 0 (1 byte)
        for _ in 0..135 {
            let mut wrapper = Vec::new();
            let len = data.len();
            if len < 14 {
                wrapper.push(0xB0 | (len as u8));
            } else {
                wrapper.push(0xBE);
                let (buf, blen) = encode_varuint_buf(len as u64);
                wrapper.extend_from_slice(&buf[..blen]);
            }
            wrapper.extend_from_slice(&data);
            data = wrapper;
        }

        let mut reader = IonReader::new(&data);
        let err = reader.decode_value().unwrap_err();
        match err {
            IonError::MalformedIon(msg) => {
                assert!(msg.contains("Recursion depth limit exceeded"));
            }
            _ => panic!("Expected MalformedIon for deep recursion, got {:?}", err),
        }
    }

    #[test]
    fn test_ion_decimal_negative_exponent_clamp() {
        // Type 0x5 Decimal: exponent = -100 (VarInt: 0x40 | 100 = 0x64 -> 0x80 | 0x64 = 0xE4? No, VarInt: sign bit is 0x40, stop bit is 0x80)
        // VarInt for -100:
        // is_neg: 1. 100 = 0x64.
        // In 1 byte: first & 0x40 is sign. (first & 0x3F) = 63. 100 > 63, so 2 bytes.
        // byte 0: 0x40 (sign) | (100 >> 7 = 0) = 0x40.
        // byte 1: 0x80 (stop) | (100 & 0x7F = 100) = 0xE4.
        // Total bytes for -100: [0x40, 0xE4].
        // Decimal descriptor: 0x53 (len 3: 2 exp bytes + 1 mantissa byte [0x81])
        let decimal_data = [0x53, 0x40, 0xE4, 0x81];
        let mut reader = IonReader::new(&decimal_data);
        let err = reader.decode_value().unwrap_err();
        match err {
            IonError::MalformedIon(msg) => {
                assert!(msg.contains("Decimal shift exceeds bound"));
            }
            _ => panic!("Expected MalformedIon for decimal shift, got {:?}", err),
        }
    }

    #[test]
    fn test_ion_negation_i64_min() {
        // VarInt decoding with 10 byte limit
        let endless_varint = [0x01u8; 12];
        let mut offset = 0;
        let err = decode_varint_slice(&endless_varint, &mut offset).unwrap_err();
        assert!(matches!(err, IonError::MalformedIon(_)));
    }

    #[test]
    fn test_ion_rejects_truncated_decimal_without_panic() {
        // Decimal descriptor 0x51: declared content_len = 1 byte.
        // VarInt exponent is multi-byte [0x01, 0x81] (2 bytes).
        // decode_varint_slice advances cur to 3, but payload_end is 2.
        // Must return Err instead of slicing out of bounds panic!
        let malformed = [0x51, 0x01, 0x81];
        let mut reader = IonReader::new(&malformed);
        let err = reader.decode_value().unwrap_err();
        assert!(matches!(err, IonError::MalformedIon(_)));
    }

    #[test]
    fn test_ion_varuint_varint_no_shift_overflow_panic() {
        // 10 bytes with all bits set (0x7F): would overflow u64 on 10th shift
        let overflow_bytes = [0x7Fu8; 10];
        let mut offset = 0;
        let err_u = decode_varuint(&overflow_bytes, &mut offset).unwrap_err();
        assert!(matches!(err_u, IonError::MalformedIon(_)));

        let mut offset_s = 0;
        let err_s = decode_varint_slice(&overflow_bytes, &mut offset_s).unwrap_err();
        assert!(matches!(err_s, IonError::MalformedIon(_)));
    }

    #[test]
    fn test_ion_annotation_length_overflow_rejected() {
        // Type 0xEE (Annotated with VarUInt length).
        // declared payload_len = 5 (via VarUInt 0x85).
        // annot_length = 10 (via VarUInt 0x8A > 5).
        let malformed_annot = [0xEE, 0x85, 0x8A, 0x01, 0x02, 0x03, 0x04];
        let mut reader = IonReader::new(&malformed_annot);
        let err = reader.decode_value().unwrap_err();
        assert!(matches!(err, IonError::UnexpectedEof(_)));
    }

    #[test]
    fn test_ion_decimal_checked_scaling() {
        let mut writer = IonWriter::new();
        // Huge exponent or float representation must not panic
        writer.write_decimal("1e100");
        writer.write_decimal("-1e100");
        writer.write_decimal("NaN");
        writer.write_decimal("Infinity");
        writer.write_decimal("0.0000000000001");
        let bytes = writer.into_bytes();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_ion_write_struct_direct_roundtrip() {
        let mut writer = IonWriter::new();
        let fields = vec![
            (10, IonValue::String("hello".to_string())),
            (20, IonValue::Int(42)),
            (30, IonValue::Bool(true)),
            (
                40,
                IonValue::List(vec![
                    IonValue::Int(1),
                    IonValue::Int(2),
                    IonValue::String("sub".to_string()),
                ]),
            ),
            (
                50,
                IonValue::Struct(vec![
                    (1, IonValue::Symbol(150)),
                    (2, IonValue::String("nested".to_string())),
                ]),
            ),
        ];

        writer.write_value(&IonValue::Struct(fields));
        let bytes = writer.to_bytes();

        let mut reader = IonReader::new(bytes);
        let decoded = reader.decode_value().unwrap();

        match decoded {
            IonValue::Struct(decoded_fields) => {
                assert_eq!(decoded_fields.len(), 5);
                assert_eq!(decoded_fields[0].0, 10);
                assert_eq!(decoded_fields[0].1, IonValue::String("hello".to_string()));
                assert_eq!(decoded_fields[1].0, 20);
                assert_eq!(decoded_fields[1].1, IonValue::Int(42));
                assert_eq!(decoded_fields[2].0, 30);
                assert_eq!(decoded_fields[2].1, IonValue::Bool(true));
            }
            _ => panic!("Expected IonValue::Struct"),
        }
    }

    #[test]
    fn test_ion_container_bounds_rejection() {
        // Construct a list of declared length 5, but whose first child claims length 10
        let malformed: [u8; 7] = [
            0xB5, // List declaring 5 bytes of content
            0x8A, // String declaring 10 bytes of content (exceeds parent list bounds!)
            b'a', b'b', b'c', b'd', b'e',
        ];

        let mut reader = IonReader::new(&malformed);
        let result = reader.decode_value();
        assert!(
            result.is_err(),
            "Child element exceeding parent container bounds must return Err"
        );
    }

    #[test]
    fn test_ion_invalid_float_length_rejected() {
        // Ion Type 4 (Float) declaring length 3 (invalid, must be 0, 4, or 8)
        let malformed = [0x43, 0x01, 0x02, 0x03];
        let mut reader = IonReader::new(&malformed);
        let result = reader.decode_value();
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), IonError::MalformedIon(msg) if msg.contains("Invalid Ion float length"))
        );
    }

    #[test]
    fn test_ion_nop_padding_vs_null_and_container_pads() {
        // 0x00 (1-byte pad) + 0x02 AA BB (3-byte pad) + 0x0E 81 CC (VarUInt 1-byte pad) + 0x0F (Null)
        let stream = [0x00, 0x02, 0xAA, 0xBB, 0x0E, 0x81, 0xCC, 0x0F];
        let mut r = IonReader::new(&stream);
        assert_eq!(r.decode_value().unwrap(), IonValue::Null);
        assert!(r.is_empty());

        // Stream consisting solely of NOPs must return UnexpectedEof when a value is required
        let nop_only = [0x00, 0x01, 0xFF, 0x0E, 0x82, 0x11, 0x22];
        let mut r_nop = IonReader::new(&nop_only);
        assert!(matches!(
            r_nop.decode_value(),
            Err(IonError::UnexpectedEof(_))
        ));

        // Truncated NOP pad inside container bounds must return UnexpectedEof
        let truncated_nop_in_list = [0xB2, 0x03, 0xAA]; // List len 2, NOP claims 3 bytes
        let mut r_trunc = IonReader::new(&truncated_nop_in_list);
        assert!(matches!(
            r_trunc.decode_value(),
            Err(IonError::UnexpectedEof(_))
        ));

        // List with leading and trailing NOP pads around Int(7) and Null (0x0F)
        let list_with_nops = [0xB7, 0x00, 0x21, 0x07, 0x0F, 0x02, 0xDE, 0xAD];
        let mut r_list = IonReader::new(&list_with_nops);
        assert_eq!(
            r_list.decode_value().unwrap(),
            IonValue::List(vec![IonValue::Int(7), IonValue::Null])
        );

        // Struct with NOP field ($0: 0x00) followed by SID 175 ([0x01, 0xAF]) and trailing NOP field
        let struct_with_nops = [
            0xD9, // Struct length 9
            0x80, 0x00, // $0: 1-byte NOP
            0x01, 0xAF, 0x21, 0x2A, // $175: 42
            0x80, 0x01, 0x99, // $0: 2-byte NOP
        ];
        let mut r_struct = IonReader::new(&struct_with_nops);
        assert_eq!(
            r_struct.decode_value().unwrap(),
            IonValue::Struct(vec![(175, IonValue::Int(42))])
        );
    }

    #[test]
    fn test_ion_sorted_struct_d1_and_bounded_fields() {
        // Sorted struct 0xD1 with VarUInt length 6 (0x86): {$10: 1, $175: true}
        let sorted_struct = [0xD1, 0x86, 0x8A, 0x21, 0x01, 0x01, 0xAF, 0x11];
        let mut r = IonReader::new(&sorted_struct);
        assert_eq!(
            r.decode_value().unwrap(),
            IonValue::Struct(vec![(10, IonValue::Int(1)), (175, IonValue::Bool(true))])
        );

        // Struct field VarUInt extending past struct payload_end must be rejected
        let truncated_field_sid = [0xD2, 0x01, 0x02, 0xAF, 0x20];
        let mut r_bad = IonReader::new(&truncated_field_sid);
        assert!(r_bad.decode_value().is_err());
    }

    #[test]
    fn test_ion_symbol_length_cap_and_annotation_strictness() {
        // Symbol length 5 (> 4 bytes) must return Err (both inline 0x75 and VarUInt 0x7E)
        let sym_too_long = [0x75, 0x00, 0x00, 0x00, 0x00, 0x01];
        let mut r_sym = IonReader::new(&sym_too_long);
        assert!(matches!(
            r_sym.decode_value(),
            Err(IonError::MalformedIon(_))
        ));

        let sym_varuint_too_long = [0x7E, 0x85, 0x00, 0x00, 0x00, 0x00, 0x01];
        assert!(matches!(
            IonReader::new(&sym_varuint_too_long).decode_value(),
            Err(IonError::MalformedIon(_))
        ));

        let sym_varuint_valid = [0x7E, 0x82, 0x01, 0xAB];
        assert_eq!(
            IonReader::new(&sym_varuint_valid).decode_value().unwrap(),
            IonValue::Symbol(0x01AB)
        );

        // Annotation length == 0 must be rejected
        let empty_annot = [0xE2, 0x80, 0x20];
        assert!(matches!(
            IonReader::new(&empty_annot).decode_value(),
            Err(IonError::MalformedIon(_))
        ));

        // Annotation SID spanning past annot_end must be rejected
        let spanning_sid = [0xE4, 0x81, 0x01, 0x8A, 0x20];
        assert!(IonReader::new(&spanning_sid).decode_value().is_err());

        // Nested 0xE annotation wrapper must be rejected
        let nested_annot = [0xE6, 0x81, 0x8A, 0xE3, 0x81, 0x8B, 0x20];
        assert!(matches!(
            IonReader::new(&nested_annot).decode_value(),
            Err(IonError::MalformedIon(_))
        ));

        // Trailing bytes inside annotation wrapper after wrapped value must be rejected
        let trailing_in_annot = [0xE4, 0x81, 0x8A, 0x20, 0x20];
        assert!(matches!(
            IonReader::new(&trailing_in_annot).decode_value(),
            Err(IonError::MalformedIon(_))
        ));
    }

    #[test]
    fn test_ion_integer_and_decimal_overflow_no_clamping() {
        // PosInt 0x28 with 2^63 (exceeds i64::MAX) must error instead of clamping
        let pos_overflow = [0x28, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(matches!(
            IonReader::new(&pos_overflow).decode_value(),
            Err(IonError::MalformedIon(_))
        ));

        // NegInt 0x38 with 2^63 + 1 (exceeds |i64::MIN|) must error instead of clamping
        let neg_overflow = [0x38, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        assert!(matches!(
            IonReader::new(&neg_overflow).decode_value(),
            Err(IonError::MalformedIon(_))
        ));

        // parse_decimal_exact checked arithmetic boundaries
        assert_eq!(
            parse_decimal_exact("-9223372036854775808"),
            Some((i64::MIN, 0))
        );
        assert_eq!(parse_decimal_exact("9223372036854775808"), None);
        assert_eq!(parse_decimal_exact("0.1e-2147483648"), None);
        assert_eq!(parse_decimal_exact("1.0e128"), None);
    }
}
