# Contributing to `kfx-converter`

Thank you for your interest in contributing to `kfx-converter`! We welcome bug reports, feature requests, documentation improvements, and code contributions.

---

## 1. Development Setup

### Requirements

- **Rust**: Version 1.85+ (2024 Edition)
- **Cargo**: Standard package manager included with Rust

```bash
# Clone the repository
git clone https://github.com/drew-developer/converter_epub2kfx.git
cd converter_epub2kfx

# Verify build
cargo check
cargo test
```

---

## 2. Code Organization

- **`src/symbols.rs`**: Amazon Yellowjacket standard symbol table and enum definitions.
- **`src/ion.rs`**: Pure-Rust Amazon Ion 1.0 binary serialization.
- **`src/container.rs`**: KFX container binary format (`CONT` / `ENTY`).
- **`src/epub.rs`**: EPUB parsing and DOM event extraction via `quick-xml`.
- **`src/ir.rs`**: Intermediate representation (`BookIR`) and chapter fragmentation algorithms.
- **`src/packager.rs`**: Yellowjacket entity assembly, location maps, and metadata generation.
- **`src/enhancements/`**: Word Wise sidecar generator and cover image optimizer.
- **`src/sync.rs`**: Kindle device detection and synchronization.
- **`src/ui/`**: GPUI-based desktop user interface (`kfx-studio`).

---

## 3. Engineering Guidelines

- **Zero Unsound Unsafe**: Avoid `unsafe` blocks unless strictly necessary for OS interop.
- **Deterministic Output**: Container generation and symbol interning must produce byte-reproducible outputs for identical inputs.
- **Defensive Resource Limits**: All binary and archive parsers must enforce strict size bounds and stream-limited readers to protect against zip bombs or memory exhaustion.
- **Kindle Engine Compliance**: Respect Kindle OS limits (such as the 240MB `pmond` memory ceiling) by maintaining chapter fragmentation rules.

---

## 4. Running Tests

```bash
# Run standard unit tests
cargo test

# Run tests with compiler warnings treated as errors
RUSTFLAGS="-D warnings" cargo test
```

---

## 5. Submitting Changes

1. Fork the repository and create a feature branch (`git checkout -b feat/my-feature`).
2. Ensure all unit tests pass (`cargo test`).
3. Follow [Conventional Commits](https://www.conventionalcommits.org/) for commit messages (e.g., `feat:`, `fix:`, `docs:`, `refactor:`).
4. Open a Pull Request detailing the purpose, technical rationale, and test results.
