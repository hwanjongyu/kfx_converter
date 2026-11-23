# Contributing to `kfx-converter`

Thank you for your interest in contributing to `kfx-converter`! We welcome bug reports, feature requests, documentation improvements, and code contributions.

---

## 1. Development Setup

### Requirements

- **Rust**: Version 1.85+ (2024 Edition)
- **Cargo**: Standard package manager included with Rust
- **Linux GUI System Packages** (required when building with `--features gui` or `--all-features`):
  ```bash
  sudo apt-get update
  sudo apt-get install -y --no-install-recommends \
    pkg-config \
    libasound2-dev \
    libxkbcommon-dev \
    libxkbcommon-x11-dev \
    libvulkan-dev \
    libwayland-dev \
    libfontconfig1-dev
  ```

```bash
# Clone the repository
git clone https://github.com/hwanjongyu/kfx_converter.git
cd kfx_converter

# Verify build
cargo check
cargo test --all-features
```

---

## 2. Code Organization

- **`src/symbols.rs`**: Amazon Yellowjacket standard symbol table (`$0..851`) and `KfxSymbol` enum definitions.
- **`src/ion.rs`**: Pure-Rust Amazon Ion 1.0 binary serialization and `ENTY` envelope wrapping.
- **`src/container.rs`**: KFX container binary format (`CONT` / `bcIndexTab`).
- **`src/css.rs`**: Stylesheet parser, font-size ratios, and element style cache.
- **`src/epub/`**: Modular EPUB parsing (`mod.rs`, `archive.rs`, `opf.rs`, `nav.rs`, `xhtml.rs`, `css_style.rs`, `dropcap.rs`, `ruby.rs`).
- **`src/ir.rs`**: Intermediate representation (`BookIR`) and section fragmentation algorithms (`DEFAULT_MAX_SECTION_CHARS = 1_000_000`, `DEFAULT_MAX_SECTION_BLOCKS = 10_000`).
- **`src/packager/`**: Modular Yellowjacket entity assembly (`mod.rs`, `core.rs`, `accumulator.rs`, `error.rs`, `types.rs`, `locators.rs`, `metadata.rs`, `navigation.rs`, `resources.rs`, `storylines.rs`, `styles.rs`, `symbols.rs`, `tests.rs`).
- **`src/pipeline.rs`**: Unified `ConversionPipeline` orchestrator and conversion logging.
- **`src/validator.rs`**: Pre-flight Yellowjacket entity, link, anchor, and reference validator (`KfxPreflightValidator`).
- **`src/enhancements/`**: Word Wise `.kll` sidecar generator (`word_wise.rs`, `dictionary.rs`), cover image optimizer (`cover.rs`), and `.sdr` deployment (`sdr.rs`).
- **`src/sync/`**: Modular Kindle device detection and synchronization (`mod.rs`, `catalog.rs`, `device.rs`, `sanitize.rs`, `transfer.rs`, `types.rs`).
- **`src/ui/`**: GPUI-based desktop user interface (`main.rs`, `state.rs`, `theme.rs`, and `app/{mod,actions,components,converter,dual_pane,folder_browser,header,kindle_explorer}.rs`).

---

## 3. Engineering Guidelines

- **Zero Unsound Unsafe**: Avoid `unsafe` blocks unless strictly necessary for OS interop, and document every `unsafe` block with a `// SAFETY:` invariant comment.
- **Deterministic Output**: Container generation and symbol interning must produce byte-reproducible outputs for identical inputs.
- **Defensive Resource Limits**: All binary and archive parsers must enforce strict size bounds and stream-limited readers to protect against zip bombs or memory exhaustion.
- **Kindle Engine Compliance**: Respect Kindle OS limits (such as the 240MB `pmond` memory ceiling) by maintaining section fragmentation and CSS page-break rules.

---

## 4. Running Tests

```bash
# Run core library tests without optional CLI/GUI features
cargo test --lib --no-default-features

# Run full unit + integration test suite across all features (CLI + GUI)
cargo test --all-features

# Check formatting, lints, and rustdoc with zero warnings
cargo fmt -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

---

## 5. Submitting Changes

1. Fork the repository and create a feature branch (`git checkout -b feat/my-feature`).
2. Ensure formatting, clippy, docs, and all tests pass cleanly.
3. Follow [Conventional Commits](https://www.conventionalcommits.org/) for commit messages (e.g., `feat:`, `fix:`, `docs:`, `refactor:`).
4. Open a Pull Request detailing the purpose, technical rationale, and test results.
