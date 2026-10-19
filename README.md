# kfx-converter

[![Rust 2024](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Zero-Dependency](https://img.shields.io/badge/runtime%20deps-zero%20Python%2FC-brightgreen.svg)]()

High-performance native EPUB to Amazon KFX (Yellowjacket / Enhanced Typesetting) e-book converter and Kindle toolchain written in Rust.

---

## ⚡ Key Highlights

- **Zero Python / Calibre Runtime Dependencies**: Direct binary generation of Amazon KFX containers and Ion serialization without requiring external Python scripts, Calibre plugins, or Java/C runtimes.
- **Pure-Rust Amazon Ion Serializer**: Custom implementation of binary Amazon Ion 1.0 (Binary Version Marker `0xE0 0x01 0x00 0xEA`, 7-bit `VarUInt`/`VarInt`, and type descriptors).
- **Yellowjacket (YJ) Container Packaging**: Complete adherence to the 18-byte `CONT` header, `bcIndexTab` chunk indexing, and `ENTY` envelope wrapping.
- **Full Locator & Position Mapping**: Emits Amazon Kindle internal locator suites including `$609` (section position ID map), `$264` (position map), `$265` (position ID map), `$550` (350 chars/location map), and `$621` (`yj.location_pid_map`).
- **Kindle OS Crash Mitigation (Chapter Fragmentation)**: Automatically splits oversized sections exceeding thresholds (7,500 characters or 25 blocks) to prevent reader engines from hitting Kindle's 240MB `pmond` memory ceiling.
- **Reading Enhancements**:
  - **Word Wise Sidecars (`.kll`)**: Automatically builds Amazon LanguageLayer SQLite databases with a curated built-in CEFR B2–C2 academic lemma dictionary.
  - **Cover Thumbnails**: Generates baseline JPEG thumbnails matching Kindle display requirements.
- **Kindle Hardware Synchronization**: Automated device detection and sideloading for both USB Mass Storage (UMS) and Linux GVFS MTP (`/run/user/$UID/gvfs/mtp:*`) with automated `.sdr` directory maintenance.
- **Dual Frontends**: Fast CLI tool (`kfx-conv`) and modern GPU-accelerated desktop UI (`kfx-studio` powered by GPUI).

---

## 🏗️ Architecture Overview

```mermaid
flowchart TD
    subgraph Ingestion
        EPUB[EPUB Source File] -->|EpubReader| IR[Intermediate Representation: BookIR]
    end

    subgraph Transformation & Fragmentation
        IR -->|Chapter-First Fragmentation| FRAG[Sub-fragmented Sections <br/><i>Guards 240MB pmond ceiling</i>]
    end

    subgraph Packaging Engine
        FRAG -->|KfxPackager| PKG[Entity Assembly]
        SYM[Yellowjacket Symbol Table<br/><i>$0..851 standard, $852+ local</i>] --> PKG
        ION[IonWriter<br/><i>Binary Ion 1.0 Serializer</i>] --> PKG
        PKG -->|CONT / ENTY Writer| KFX[Output .kfx Container]
    end

    subgraph Enhancements & Sideloading
        IR -->|CoverOptimizer| THUMB[Cover Thumbnail JPEG]
        IR -->|WordWiseBuilder| KLL[LanguageLayer .kll SQLite DB]
        KFX --> SYNC[KindleSync Engine]
        THUMB --> SYNC
        KLL --> SYNC
        SYNC -->|USB UMS / GVFS MTP| KINDLE[Amazon Kindle Device]
    end
```

---

## 📦 Project Structure

```
converter_epub2kfx/
├── Cargo.toml                 # Package configuration and feature flags
├── build.rs                   # Build script
├── src/
│   ├── lib.rs                 # Core library definitions and re-exports
│   ├── main.rs                # CLI entry point (`kfx-conv`)
│   ├── symbols.rs             # 852-entry Yellowjacket symbol table and enum
│   ├── ion.rs                 # Binary Amazon Ion serializer and ENTY wrapper
│   ├── container.rs           # CONT header and bcIndexTab chunk reader/writer
│   ├── epub.rs                # Streaming EPUB/ZIP reader and quick-xml parser
│   ├── ir.rs                  # Book intermediate representation and section fragmenter
│   ├── packager.rs            # Yellowjacket entity packager and metadata builder
│   ├── sync.rs                # Kindle device sync (UMS & Linux GVFS MTP)
│   ├── enhancements/
│   │   ├── mod.rs
│   │   ├── cover.rs           # Baseline JPEG thumbnail optimizer
│   │   ├── dictionary.rs      # Built-in CEFR B2-C2 academic lemma database
│   │   └── word_wise.rs       # Kindle SQLite Word Wise sidecar generator
│   └── ui/                    # GPUI Desktop Application (`kfx-studio`)
│       ├── main.rs            # Desktop GUI binary entry point
│       ├── app.rs             # GPUI views and layouts
│       ├── state.rs           # GUI application state
│       └── theme.rs           # UI color palettes and styling
└── docs/
    ├── ARCHITECTURE.md               # Technical architecture and subsystems
    ├── FORMAT_SPEC.md                # KFX / Yellowjacket container format specification
    ├── CLI_GUIDE.md                  # Detailed CLI user guide and device sync
    ├── YELLOWJACKET.md               # In-depth Yellowjacket rendering engine monograph
    └── multi_model_review_report.md  # Multi-model consensus audit
```

---

## 🚀 Installation & Building

### Prerequisites

- [Rust](https://rustup.rs/) (2024 edition, 1.85+ recommended)
- `pkg-config` and `libssl-dev` (if compiling with optional GUI features on Linux)

### Build the CLI (`kfx-conv`)

```bash
# Build the default CLI binary
cargo build --release --bin kfx-conv

# Output binary will be at:
# target/release/kfx-conv
```

### Build the GPUI Desktop GUI (`kfx-studio`)

The desktop GUI uses the high-performance GPUI framework. Enable the `gui` feature flag:

```bash
cargo build --release --bin kfx-studio --features gui
```

---

## 💻 CLI Usage (`kfx-conv`)

```
High-performance native EPUB to Amazon KFX converter

Usage: kfx-conv [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Input EPUB or KFX file path

Options:
  -o, --output <OUTPUT>    Output destination path [default: <INPUT>.kfx]
  -i, --inspect            Inspect / dump KFX container metadata and chunks
  -w, --word-wise          Generate Word Wise sidecar (.kll)
  -s, --sync               Automatically synchronize converted book to connected Kindle device
  -h, --help               Print help
  -V, --version            Print version
```

### Examples

#### 1. Basic Conversion
Convert an EPUB file into a native `.kfx` book:
```bash
kfx-conv book.epub -o book.kfx
```

#### 2. Conversion with Word Wise Sidecar
Generate the KFX container and an accompanying Word Wise SQLite database (`.kll`):
```bash
kfx-conv book.epub -o book.kfx -w
```

#### 3. Convert & Auto-Sync to Kindle
Convert an EPUB, optimize its cover thumbnail, compile Word Wise glosses, and automatically sideload directly to a connected Kindle device:
```bash
kfx-conv book.epub -w -s
```

#### 4. Inspect an Existing KFX File
Examine the internal chunk hierarchy and symbol types of any KFX container:
```bash
kfx-conv book.kfx -i
```

Output:
```text
Valid KFX Container:
  Version: 2
  Header Length: 42 bytes
  ContainerInfo Offset: 88 bytes
  ContainerInfo Length: 124 bytes
  Total Chunks: 14
    [0] ID: 852, Type: bcContId ($), Offset: 212, Length: 64 bytes
    [1] ID: 853, Type: book_metadata ($), Offset: 276, Length: 512 bytes
    [2] ID: 854, Type: section ($), Offset: 788, Length: 4096 bytes
    ...
```

---

## 🧪 Testing

Run standard unit tests across the container, Ion encoder, and section fragmentation modules:

```bash
cargo test
```

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
