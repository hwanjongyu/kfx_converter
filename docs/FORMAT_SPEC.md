# Amazon KFX / Yellowjacket Format Specification Reference

This document provides a technical reference for the Amazon KFX (Yellowjacket / Enhanced Typesetting) format as implemented in `kfx-converter`.

---

## 1. Container Binary Format (`CONT`)

A KFX file is a binary container structured into a header, an index table, optional header payloads, container metadata (`ContainerInfo`), and raw entity payloads.

### 1.1 Header Structure (18 Bytes)

| Offset (Bytes) | Size (Bytes) | Type | Field Name | Description |
| :--- | :--- | :--- | :--- | :--- |
| `0x00..0x03` | 4 | `[u8; 4]` | `magic` | Container magic string: `b"CONT"` (`0x43 0x4F 0x4E 0x54`) |
| `0x04..0x05` | 2 | `u16` (LE) | `version` | Container format version (typically `1` or `2`) |
| `0x06..0x09` | 4 | `u32` (LE) | `header_length` | Total length of container header data before entity payloads |
| `0x0A..0x0D` | 4 | `u32` (LE) | `info_offset` | Offset from file start to `ContainerInfo` payload |
| `0x0E..0x11` | 4 | `u32` (LE) | `info_length` | Length of `ContainerInfo` payload in bytes |

### 1.2 Chunk Index Table (`bcIndexTab`)

Immediately following the 18-byte header is the index table. It contains an array of 24-byte chunk descriptor entries:

| Offset in Entry | Size (Bytes) | Type | Field Name | Description |
| :--- | :--- | :--- | :--- | :--- |
| `0x00..0x03` | 4 | `u32` (LE) | `entity_id` | Document-unique entity ID ($852+ or 348 for singleton) |
| `0x04..0x07` | 4 | `u32` (LE) | `chunk_type` | Yellowjacket Symbol ID representing entity type |
| `0x08..0x0F` | 8 | `u64` (LE) | `offset` | Absolute byte offset to entity payload from file start |
| `0x10..0x17` | 8 | `u64` (LE) | `length` | Byte length of the entity payload (including ENTY header) |

---

## 2. Entity Envelope Format (`ENTY`)

Entity payloads in `bcIndexTab` are enveloped with a 21-byte header:

```
[0..3]   Magic bytes: 'ENTY' (0x45 0x4E 0x54 0x59)
[4..5]   Envelope version: uint16 LE (1)
[6..9]   Header length: uint32 LE (21)
[10..20] Pre-serialized Ion header:
         0xE0 0x01 0x00 0xEA (Ion BVM)
         0xD6 0x03 0x9A 0x20 0x03 0x9B 0x20 (Struct {$410: 0, $411: 0})
[21..]   Raw Ion payload
```
- **Symbol 410 (`bcComprType`)**: Compression type (`0` = uncompressed).
- **Symbol 411 (`bcDRMScheme`)**: DRM scheme (`0` = unprotected / DRM-free).

---

## 3. Yellowjacket Symbol Allocation

Amazon KFX relies on indexed symbol tables to minimize string overhead:

- **Standard Yellowjacket Symbols (`$0` to `$851`)**: Fixed, built-in symbol mappings defined across all Kindle e-readers.
- **Document-Local Symbols (`$852+`)**: Dynamic symbols declared within the document's `$ion_symbol_table` (Ion Symbol 3).

### Key Standard Symbol IDs

| Symbol ID | Symbol Name | Usage |
| :--- | :--- | :--- |
| **`$1`** | `$ion` | Ion system symbol |
| **`$3`** | `$ion_symbol_table` | Embedded symbol table annotation |
| **`$145`** | `content` | Text block and inline styling payloads |
| **`$157`** | `style` | Formatting styles |
| **`$164`** | `external_resource` | External image and media descriptors |
| **`$258`** | `metadata` | Publication and reading metadata |
| **`$259`** | `storyline` | Structural storyline chunk |
| **`$260`** | `section` | Chapter/section template and block container |
| **`$264`** | `position_map` | Mapping between positions and content elements |
| **`$265`** | `position_id_map` | Mapping between IDs and positions |
| **`$409`** | `bcContId` | Container ID descriptor |
| **`$419`** | `container_entity_map` | Manifest of all entities contained in the file |
| **`$490`** | `book_metadata` | High-level book title, author, and language |
| **`$538`** | `document_data` | Document layout and baseline typographical rules |
| **`$550`** | `location_map` | Kindle reading location calibration (350 chars/loc) |
| **`$585`** | `content_features` | Declared typesetting engine feature flags |
| **`$593`** | `format_capabilities` | Container capabilities header |
| **`$597`** | `auxiliary_data` | Supplemental per-section reading data |
| **`$609`** | `section_position_id_map` | Per-section position identifier index |
| **`$621`** | `yj.location_pid_map` | Yellowjacket location to PID mapping |

---

## 4. Deterministic Identifier Schemes

`kfx-converter` uses cryptographic hashing (SHA-1) to generate Kindle-compliant deterministic identifiers:

1. **Container ID (`generate_container_id`)**:
   - Format: `CR!` followed by 28 alphanumeric uppercase characters (e.g., `CR!A1B2C3D4E5F6G7H8I9J0K1L2M3N4`).
   - Total length: 31 characters.
2. **Content ID (`generate_content_id`)**:
   - Format: 32-character uppercase hexadecimal string.
   - Purpose: Keys sideloaded cover thumbnails and Word Wise sidecars on Kindle devices.
3. **Book ID (`generate_book_id`)**:
   - Format: 23-character URL-safe Base64 string prefixed with version byte `0x05`.

---

## 5. Word Wise LanguageLayer Database (`.kll`)

Kindle Word Wise sidecars are SQLite 3 databases stored in the companion `<Title>.sdr/` folder under the name:
```
LanguageLayer.en.<ContentID>.kll
```

### Database Schema

```sql
CREATE TABLE metadata (
    key TEXT PRIMARY KEY,
    value TEXT
);

CREATE TABLE glosses (
    id INTEGER PRIMARY KEY,
    start_pos INTEGER NOT NULL,
    end_pos INTEGER NOT NULL,
    word TEXT NOT NULL,
    stem TEXT NOT NULL,
    difficulty INTEGER NOT NULL,
    sense_id INTEGER NOT NULL
);
```

- **`start_pos` / `end_pos`**: Character offsets within the reading text.
- **`difficulty`**: Integer rating (1 = Basic, 3 = Intermediate B2, 4 = Advanced C1, 5 = Rare C2).
- **`sense_id`**: Reference index into Kindle's internal Word Wise definitions database.
