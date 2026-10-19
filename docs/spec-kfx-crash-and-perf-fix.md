# Specification & Remediation Plan: KFX Stability & Performance Overhaul

**Task Document:** `docs/spec-kfx-crash-and-perf-fix.md`  
**Status:** `Pending Approval (Phase 1 Gate)`  
**Target Codebase:** [`/home/drew/github/drew-developer/converter_epub2kfx`](file:///home/drew/github/drew-developer/converter_epub2kfx)  
**Primary Objective:** Resolve KFX container loading failure (`error code = 1`), CPU thread freezes (97% CPU, 1.6s freeze), Linux kernel OOM crashes, and memory duplication on low-resource Kindle hardware (512MB RAM).

---

## 1. Problem Statement & Root Cause Mapping

From device logs (`all_system_logs_as_of_Sun_Oct_18_23.49.42_ADT_2026.txt`) and code audit:

| Issue | Severity | Root Cause File & Line | Symptom on Kindle Hardware |
| :--- | :---: | :--- | :--- |
| **Yellowjacket Symbol Collision** | **CRITICAL (P0)** | [`src/packager.rs:195`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/packager.rs#L195)<br/>`next_eid: 1` | `DocumentContextYJ: Failed to attach containers (error code = 1)`. Non-singleton entity IDs must be $\ge 852$. Collision with standard Yellowjacket symbols ($0..851) breaks container loading. |
| **Excessive Chunk Size & OOM** | **HIGH (P1)** | [`src/ir.rs:135`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/ir.rs#L135)<br/>`7,500 chars / 25 blocks` | 512MB RAM Kindle devices experience high RSS spikes in `KPPMainApp`, invoking Linux kernel `oom-killer` and thread freeze warnings (`JSQueue: 1666ms`). |
| **Dictionary Smart Quote Panic** | **CRITICAL (P0)** | [`src/enhancements/dictionary.rs:240`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/enhancements/dictionary.rs#L240)<br/>`clean.truncate(len - 2)` on `’s` | `String::truncate` panic on UTF-8 char boundary (`’` is 3 bytes in UTF-8), crashing the process on any English book with smart quotes. |
| **Word Wise Offset Drift** | **HIGH (P1)** | [`src/enhancements/word_wise.rs:182`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/enhancements/word_wise.rs#L182)<br/>Mixed byte offset & char count | Word Wise glosses drift and detach from their target words on non-ASCII text. |
| **Broken Image Resource Mapping** | **HIGH (P1)** | [`src/epub.rs:121`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/epub.rs#L121) vs [`src/packager.rs:954`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/packager.rs#L954) | Manifest items keyed by OPF ID while XHTML references raw filenames, breaking inline images in KFX storylines. |
| **Kindle Sync SDR Directory & Fsync** | **HIGH (P1)** | [`src/sync.rs:147, 213`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/sync.rs#L147) | `.sdr` directory not created prior to sidecar writing (I/O error), and FAT32 writes uncommitted without `sync_all()`. |
| **100% Memory Duplication** | **MEDIUM (P2)** | [`src/container.rs:199`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/container.rs#L199)<br/>`wrapped_payloads: Vec<Vec<u8>>` | Doubles heap memory consumption during packaging. |

---

## 2. Multi-Step Implementation Plan

### Step 1: Fix Yellowjacket Entity ID Allocation (`src/packager.rs`)
- Change initial `next_eid` from `1` to `852`:
  ```rust
  Self {
      ...
      entities: Vec::new(),
      next_eid: 852, // Standard Yellowjacket symbols end at 851; document entities strictly start at 852
  }
  ```
- This directly resolves `DocumentContextYJ: Failed to attach containers (error code = 1)` and eliminates entity ID collisions in `bcIndexTab`.

### Step 2: Optimize Chapter Fragmentation Thresholds (`src/ir.rs`)
- Adjust fragmentation thresholds for low-resource Kindle hardware (512MB RAM):
  ```rust
  pub const DEFAULT_MAX_SECTION_CHARS: usize = 3_500; // Reduced from 7,500
  pub const DEFAULT_MAX_SECTION_BLOCKS: usize = 15;   // Reduced from 25
  ```
- This cuts peak memory allocations per section chunk in half, eliminating `pmond` 240MB alerts and `JSQueue` thread freezes on e-ink devices.

### Step 3: Fix Smart Apostrophe Unicode Slicing Panic (`src/enhancements/dictionary.rs`)
- Replace the byte-saturating truncate with safe slice suffix stripping:
  ```rust
  if let Some(stripped) = clean.strip_suffix("'s").or_else(|| clean.strip_suffix("’s")) {
      clean = stripped.to_string();
  }
  ```

### Step 4: Fix Word Wise Character Offset Tracking (`src/enhancements/word_wise.rs`)
- Track character positions monotonically (`char_idx`) during text iteration:
  ```rust
  let mut char_idx = 0usize;
  let mut word_char_start = None;
  for ch in text.chars() {
      if ch.is_alphabetic() {
          if word_char_start.is_none() { word_char_start = Some(char_idx); }
      } else if let Some(start_c) = word_char_start.take() {
          // Add gloss at abs_pos + start_c
      }
      char_idx += 1;
  }
  ```

### Step 5: Fix Resource ID Resolution Between Manifest & XHTML (`src/epub.rs`)
- Build an `href -> id` map during OPF manifest parsing.
- When parsing `<img src="...">` in `parse_xhtml_blocks`, resolve the normalized `src` path against the manifest map to store the exact `item.id`.

### Step 6: Create SDR Directory & Add `fsync` on FAT32 Volumes (`src/sync.rs`)
- Ensure companion `.sdr` directory exists before syncing `.kll` sidecars:
  ```rust
  if kll_bytes.is_some() || sdr_dir.exists() {
      std::fs::create_dir_all(&sdr_dir)?;
  }
  ```
- Flush USB storage writes using `f.sync_all()`:
  ```rust
  let f = std::fs::OpenOptions::new().write(true).open(dst)?;
  f.sync_all()?;
  ```

### Step 7: Stream Entity Payloads Directly in Container Writer (`src/container.rs`)
- Eliminate `wrapped_payloads: Vec<Vec<u8>>`.
- Compute payload offsets mathematically and stream `MAGIC_ENTY`, header, and raw data directly into `writer`.

---

## 3. Verification & Deterministic Test Strategy

1. **Unit Test Verification (`cargo test`)**:
   - Verify `alloc_eid()` never produces an ID $< 852$ (except 348).
   - Test `dictionary::lookup` against smart quotes: `"it’s"`, `"Grant’s"`, `"writer's"`.
   - Test `BookIR::fragment_large_sections` with new thresholds (3,500 chars / 15 blocks).
   - Test `WordWiseBuilder` offset calculation with accented and Unicode characters.
2. **Container Roundtrip Test**:
   - Verify generated container header, `bcIndexTab` entries, and `format_capabilities` placement.
3. **Compilation Verification**:
   - `cargo check --all-targets`
   - `cargo build --release --bin kfx-conv`

---

## 4. Phase 1 Pre-Approval Gate

> [!IMPORTANT]
> In accordance with the **NEXUS Multi-Agent Handoff Protocol**, no code modifications or builds will commence until explicit approval is granted by the user.
