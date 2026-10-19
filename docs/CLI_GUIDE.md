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
| `--output <PATH>` | `-o` | Specifies output path. Defaults to `<INPUT>.kfx`. |
| `--inspect` | `-i` | Inspects and displays internal KFX container metadata and chunks. |
| `--word-wise` | `-w` | Compiles and emits a Word Wise LanguageLayer sidecar (`.kll`). |
| `--sync` | `-s` | Sideloads the book and sidecars to a connected Kindle device. |
| `--help` | `-h` | Prints help information. |
| `--version` | `-V` | Prints version information. |

---

## 2. Common Workflows

### 2.1 Basic Conversion
Converts an EPUB into a native `.kfx` book placed alongside the input file:
```bash
kfx-conv my_book.epub
# Produces my_book.kfx
```

To specify an explicit destination path:
```bash
kfx-conv my_book.epub -o /path/to/output.kfx
```

---

### 2.2 Enabling Word Wise Support (`-w`)
Word Wise displays inline hints above challenging words on Kindle screens. `kfx-converter` uses a built-in CEFR B2–C2 academic lemma database:

```bash
kfx-conv my_book.epub -w
```

When run with `-w`:
1. The text is analyzed for vocabulary matching CEFR B2–C2 thresholds.
2. Inflected forms are normalized and stemmed.
3. An accompanying SQLite sidecar is compiled.

---

### 2.3 Sideloading to Kindle Hardware (`-s`)
Automatically detects a mounted Kindle e-reader and transfers the book, cover thumbnail, and Word Wise database to their proper directories:

```bash
kfx-conv my_book.epub -w -s
```

#### What Happens During Sync:
1. **Device Detection**: Scans USB Mass Storage mount points (`/media/$USER/*`, `/run/media/$USER/*`) and Linux GVFS MTP locations (`/run/user/$UID/gvfs/mtp:*`).
2. **KFX Deployment**: Copies `<Title>.kfx` into the Kindle's `documents/` folder.
3. **Cover Thumbnail**: Writes a normalized baseline JPEG to `system/thumbnails/thumbnail_<ContentID>_EBOK_portrait.jpg`.
4. **Word Wise Sidecar**: Writes `LanguageLayer.en.<ContentID>.kll` into `documents/<Title>.sdr/`.
5. **Housekeeping**: Removes stale `.bad_file` markers and orphaned sidecars in `<Title>.sdr/`.

---

### 2.4 Inspecting Container Internals (`-i`)
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
Ensure the Kindle partition is mounted with write permissions:
```bash
ls -ld /media/$USER/Kindle/documents
```
If not mounted, mount manually or via your desktop environment's file manager.

### Linux MTP (Kindle Scribe & Modern FireOS Devices)
Kindle Scribe uses the Media Transfer Protocol (MTP) rather than UMS.
- The converter detects GVFS mounts at `/run/user/$UID/gvfs/mtp:host=*`.
- Ensure GVFS MTP tools are installed (`gvfs-backends` on Debian/Ubuntu).
- The converter executes `gio copy` with a 60-second watchdog timer to avoid MTP deadlocks.
