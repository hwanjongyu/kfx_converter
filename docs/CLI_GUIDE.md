# Command-Line User Guide: `kfx-conv`

The `kfx-conv` command-line interface provides high-performance conversion, sidecar generation, container inspection, and Kindle device synchronization.

---

## 1. Synopsis

```bash
kfx-conv [OPTIONS] <INPUT>
```

### Options

| Flag | Short | Description |
| :--- | :---: | :--- |
| `--output <OUTPUT>` | `-o` | Specifies output destination path. Defaults to `<INPUT>.kfx`. |
| `--inspect` | `-i` | Inspects and displays internal KFX container metadata and chunks. |
| `--sync` | `-s` | Automatically synchronizes the converted book and sidecars to a connected Kindle device. |
| `--word-wise[=<WORD_WISE>]` | `-w` | Controls Word Wise LanguageLayer sidecar (`.kll`) generation (`true` or `false`, **default: `true`**). |
| `--content-type <CONTENT_TYPE>` | `-c` | Kindle content definition environment type: `EBOK` (books/lockscreen) or `PDOC` (personal documents), case-insensitive (**default: `EBOK`**). |
| `--help` | `-h` | Prints help information. |
| `--version` | `-V` | Prints version information. |

---

## 2. Common Workflows

### 2.1 Basic Conversion (Word Wise On by Default)
Converts an EPUB into a native `.kfx` book placed alongside the input file (with Word Wise enabled by default):
```bash
kfx-conv my_book.epub
# Produces my_book.kfx and companion .sdr/ sidecar when applicable
```

To specify an explicit destination path:
```bash
kfx-conv my_book.epub -o /path/to/output.kfx
```

---

### 2.2 Configuring or Disabling Word Wise (`-w` / `--word-wise`)
Word Wise displays inline hints above challenging words on Kindle screens using a built-in 52,000+ Kindle master lemma dictionary. It is **enabled by default (`true`)**:

```bash
# Default behavior (Word Wise enabled)
kfx-conv my_book.epub
# Or explicitly pass -w / --word-wise
kfx-conv my_book.epub -w
```

To disable Word Wise `.kll` sidecar generation, pass `false` using either `--word-wise=false`, `--word-wise false`, or `-w false`:

```bash
kfx-conv my_book.epub --word-wise false
kfx-conv my_book.epub --word-wise=false
kfx-conv my_book.epub -w false
```

When enabled:
1. English prose text is analyzed for vocabulary matching Word Wise difficulty pacing thresholds (skipping non-rendered image `alt` metadata).
2. Inflected forms are normalized and lemmatized.
3. When non-zero glosses exist, an accompanying SQLite `.kll` sidecar is compiled and deployed.

---

### 2.3 Setting Kindle Content Type (`-c` / `--content-type`)
Controls the Kindle CDE content type (`EBOK` by default for full book/lockscreen support, or `PDOC` for personal documents; case-insensitive):

```bash
# Default EBOK mode
kfx-conv my_book.epub -c EBOK

# Personal Document (PDOC) mode
kfx-conv my_book.epub -c pdoc
```

---

### 2.4 Sideloading to Kindle Hardware (`-s` / `--sync`)
Automatically detects a mounted Kindle e-reader and transfers the book, cover thumbnail, and Word Wise database to their proper directories:

```bash
kfx-conv my_book.epub -s
# Or disable Word Wise while syncing:
kfx-conv my_book.epub --word-wise=false -s
```

#### What Happens During Sync:
1. **Device Detection**: Scans USB Mass Storage mount points (`/media/$USER/*`, `/run/media/$USER/*`), Linux GVFS MTP locations (`/run/user/$UID/gvfs/mtp:*`), macOS `/Volumes/*`, and Windows drive volumes.
2. **KFX Deployment**: Copies `<Title>.kfx` into the Kindle's `documents/` folder.
3. **Cover Thumbnail**: Writes a normalized baseline JPEG to `system/thumbnails/thumbnail_<ContentID>_<EBOK|PDOC>_portrait.jpg` (matching `-c`, default `EBOK`).
4. **Word Wise Sidecar**: Writes `LanguageLayer.en.<ContentID>.kll` into `documents/<Title>.sdr/` (when glosses > 0).
5. **Housekeeping**: Removes stale `.bad_file` markers and orphaned sidecars in `<Title>.sdr/`.

---

### 2.5 Inspecting Container Internals (`-i` / `--inspect`)
Examines the chunk index table and entity structure of any KFX file:

```bash
kfx-conv my_book.kfx -i
```

Example Output:
```text
=== Rust KFX Converter v0.1.0 ===
Target: "my_book.kfx"
Valid KFX Container:
  Version: 2
  Header Length: 42 bytes
  ContainerInfo Offset: 88 bytes
  ContainerInfo Length: 124 bytes
  Total Chunks: 18
    [0] ID: 852, Type: bcContId ($), Offset: 212, Length: 64 bytes
    [1] ID: 853, Type: book_metadata ($), Offset: 276, Length: 512 bytes
    [2] ID: 854, Type: section ($), Offset: 788, Length: 4096 bytes
    [3] ID: 855, Type: storyline ($), Offset: 4884, Length: 1228 bytes
    ...
```

---

## 3. Device Sync Troubleshooting

### Linux USB Mass Storage (UMS)
Kindle Paperwhite (up to 11th Gen), Oasis, Voyage, and Basic models typically mount as USB Mass Storage.
Ensure the Kindle partition is mounted with write permissions:
```bash
ls -ld /media/$USER/Kindle/documents
```
If not mounted automatically, mount manually or via your desktop environment's file manager.

### Linux Desktop MTP (Kindle Scribe, Colorsoft & Modern Devices)
Kindle Scribe and Colorsoft use the Media Transfer Protocol (MTP) rather than UMS.
- The converter automatically discovers GNOME GVFS mounts at `/run/user/$UID/gvfs/mtp:host=*`.
- Ensure GVFS MTP tools are installed (`gvfs-backends` on Debian/Ubuntu).
- The converter executes `gio copy` with a 60-second watchdog timer to avoid MTP deadlocks.

### Headless Linux, WSL & Docker Environments
In headless servers or containers lacking GNOME desktop daemons, GVFS mounts are unavailable. `kfx-converter` handles this transparently:
1. **USB Hardware Probe**: The CLI inspects `/sys/bus/usb/devices/` for connected Amazon Kindle hardware (Vendor ID `1949`) and emits diagnostic guidance if the device is plugged in but not mounted.
2. **User-Space FUSE Mount (`jmtpfs`)**:
   ```bash
   mkdir -p ~/kindle
   jmtpfs ~/kindle
   kfx-conv book.epub -s
   ```
3. **Environment Variable Override**: You can explicitly point to any mounted Kindle directory:
   ```bash
   KINDLE_ROOT=/mnt/kindle kfx-conv book.epub -s
   ```

---

## 4. Desktop GUI Application (`kfx-studio`)

For a graphical desktop experience with drag-and-drop book conversion, settings toggles, and live Kindle device explorer:

```bash
# Run the pre-built release binary directly
./target/release/kfx-studio

# Or build and launch directly via Cargo (requires Linux GPUI dev packages on Linux)
cargo run --release --bin kfx-studio --features gui
```
