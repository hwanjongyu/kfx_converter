//! Local symbols and ID generators for Amazon KFX packaging.

use std::collections::HashMap;
use sha1::{Digest, Sha1};
use crate::ion::{IonValue, IonWriter};

pub const SINGLETON_ID: u32 = 348; // $null in KFX symbol table
pub const AMAZON_YJ_SYMBOLS_MAX_ID: u32 = 851;
pub const FIRST_LOCAL_SYMBOL_ID: u32 = AMAZON_YJ_SYMBOLS_MAX_ID + 1;

/// Dynamic local symbol manager for KFX documents.
/// Per Yellowjacket standard, shared YJ_symbols end at 851,
/// and local document symbols begin strictly at SID 852.
pub struct LocalSymbols {
    pub names: Vec<String>,
    map: HashMap<String, u32>,
}

impl LocalSymbols {
    pub fn new() -> Self {
        Self {
            names: Vec::new(),
            map: HashMap::new(),
        }
    }

    /// Retrieves an existing symbol ID in O(1) time without allocating.
    pub fn get_id(&self, name: &str) -> Option<u32> {
        self.map.get(name).copied()
    }

    /// Interns a symbol string in O(1) time, returning its local symbol ID (SID >= 852).
    pub fn intern(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.map.get(name) {
            id
        } else {
            let id = FIRST_LOCAL_SYMBOL_ID + self.names.len() as u32;
            let s = name.to_string();
            self.map.insert(s.clone(), id);
            self.names.push(s);
            id
        }
    }

    pub fn to_ion_symbol_table(&self) -> Vec<u8> {
        let import_entry = IonValue::Struct(vec![
            (4, IonValue::String("YJ_symbols".to_string())), // $4 = name
            (5, IonValue::Int(10)),                         // $5 = version
            (8, IonValue::Int(AMAZON_YJ_SYMBOLS_MAX_ID as i64)), // $8 = max_id in YJ_symbols shared table
        ]);
        let sym_values: Vec<IonValue> = self
            .names
            .iter()
            .cloned()
            .map(IonValue::String)
            .collect();
        let max_sid = AMAZON_YJ_SYMBOLS_MAX_ID as i64 + self.names.len() as i64;
        let symtab_struct = IonValue::Struct(vec![
            (8, IonValue::Int(max_sid)),             // $8 = max_id of this document symbol table
            (6, IonValue::List(vec![import_entry])), // $6 = imports
            (7, IonValue::List(sym_values)),         // $7 = symbols
        ]);

        let mut sym_writer = IonWriter::new();
        sym_writer.write_bvm();
        sym_writer.write_annotated_value(&[3], &symtab_struct); // Annotated with symbol 3 ($ion_symbol_table)
        sym_writer.into_bytes()
    }
}

impl Default for LocalSymbols {
    fn default() -> Self {
        Self::new()
    }
}

/// Generates deterministic Amazon Content Repository container ID.
/// Format: 'CR!' + 28 uppercase alphanumeric characters (31 chars total).
pub fn generate_container_id(seed: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(seed.as_bytes());
    let digest = hasher.finalize();

    let mut state = u128::from_be_bytes(digest[..16].try_into().unwrap());
    let chars = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut id_chars = Vec::with_capacity(28);

    for _ in 0..28 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let idx = ((state >> 56) as usize) % chars.len();
        id_chars.push(chars[idx]);
    }

    format!("CR!{}", String::from_utf8(id_chars).unwrap())
}

/// Generates deterministic 32-character uppercase hex personal-document ID.
/// Keys sideloaded cover thumbnails on Kindle devices.
pub fn generate_content_id(seed: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(seed.as_bytes());
    let digest = hasher.finalize();

    let mut hex = String::with_capacity(32);
    for b in &digest[..16] {
        hex.push_str(&format!("{:02X}", b));
    }
    hex
}

/// Resolves or deterministically generates an Amazon ASIN conforming to regex `^B[0-9A-Z]{9}$`.
pub fn resolve_asin(identifier: &str, content_id: &str) -> String {
    let cleaned = identifier
        .trim()
        .trim_start_matches("urn:asin:")
        .trim_start_matches("mobi-asin:")
        .trim_start_matches("asin:")
        .trim();

    if cleaned.len() == 10
        && (cleaned.starts_with('B') || cleaned.starts_with('b'))
        && cleaned.chars().all(|c| c.is_ascii_alphanumeric())
    {
        cleaned.to_uppercase()
    } else {
        // Deterministic 10-character Amazon ASIN: 'B' followed by first 9 hex digits of content_id
        format!("B{}", &content_id[..9])
    }
}

/// Generates 23-character URL-safe Base64 publication ID (version byte 0x05 + 16 derived bytes).
pub fn generate_book_id(seed: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(seed.as_bytes());
    let digest = hasher.finalize();

    let mut raw = Vec::with_capacity(17);
    raw.push(0x05);
    raw.extend_from_slice(&digest[..16]);

    const URL_SAFE_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity(23);
    let mut i = 0;
    while i + 3 <= raw.len() {
        let b0 = raw[i] as usize;
        let b1 = raw[i + 1] as usize;
        let b2 = raw[i + 2] as usize;
        result.push(URL_SAFE_CHARS[(b0 >> 2) & 0x3F] as char);
        result.push(URL_SAFE_CHARS[(((b0 & 0x03) << 4) | (b1 >> 4)) & 0x3F] as char);
        result.push(URL_SAFE_CHARS[(((b1 & 0x0F) << 2) | (b2 >> 6)) & 0x3F] as char);
        result.push(URL_SAFE_CHARS[b2 & 0x3F] as char);
        i += 3;
    }
    if raw.len() - i == 2 {
        let b0 = raw[i] as usize;
        let b1 = raw[i + 1] as usize;
        result.push(URL_SAFE_CHARS[(b0 >> 2) & 0x3F] as char);
        result.push(URL_SAFE_CHARS[(((b0 & 0x03) << 4) | (b1 >> 4)) & 0x3F] as char);
        result.push(URL_SAFE_CHARS[((b1 & 0x0F) << 2) & 0x3F] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_symbols_intern_hashmap_parity() {
        let mut symbols = LocalSymbols::new();
        let id1 = symbols.intern("sym_first");
        let id2 = symbols.intern("sym_second");
        let id3 = symbols.intern("sym_third");

        assert_eq!(id1, 852);
        assert_eq!(id2, 853);
        assert_eq!(id3, 854);

        // Re-interning returns exact same IDs in O(1)
        assert_eq!(symbols.intern("sym_first"), 852);
        assert_eq!(symbols.intern("sym_second"), 853);
        assert_eq!(symbols.intern("sym_third"), 854);

        // get_id returns same without mutability
        assert_eq!(symbols.get_id("sym_first"), Some(852));
        assert_eq!(symbols.get_id("sym_second"), Some(853));
        assert_eq!(symbols.get_id("sym_nonexistent"), None);

        // Total count matches
        assert_eq!(symbols.names.len(), 3);
    }
}
