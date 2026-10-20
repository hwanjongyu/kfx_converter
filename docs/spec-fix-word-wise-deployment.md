# Spec: Fix Word Wise Automatic Generation and Sidecar Deployment

## 1. 개요 (Overview)
- **목적:** 킨들에서 Word Wise가 표시되지 않던 원인(CLI 기본 플래그 비활성화, 출력 디렉토리 내 `.sdr` 미생성, 기기 동기화 시 파일명 불일치)을 영구적으로 해결하여, 도서 변환 시 Word Wise(`.kll`)가 100% 자동 생성되고 올바른 `.sdr` 폴더에 배포되도록 보장합니다.
- **적용 대상:** [`src/main.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/main.rs), [`src/pipeline.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/pipeline.rs), [`src/sync.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/sync.rs)

---

## 2. 현상 분석 및 원인 (Root Cause)
1. **CLI 인자 기본값 누락 ([`src/main.rs:29`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/main.rs#L29)):**
   - 파이프라인 기본값은 `word_wise: true`였으나, CLI의 clap 옵션 `#[arg(short, long)] word_wise: bool`에 `default_value_t = true`가 지정되지 않아 `-w`를 명시하지 않으면 Word Wise 생성이 건너뛰어졌습니다.
2. **로컬/직접 지정 출력 시 `.sdr` 미생성 ([`src/pipeline.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/pipeline.rs)):**
   - `-o`로 직접 출력 경로를 지정했을 때, `pipeline.rs`는 `output_path`와 나란히 `.sdr` 폴더를 생성하고 `.kll`을 저장하는 로직이 없었습니다.
3. **기기 동기화 시 파일명 불일치 ([`src/sync.rs:291-295`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/sync.rs#L291-L295)):**
   - 도서 파일명(`Originals - Adam Grant.kfx`) 대신 메타데이터의 `book_title`("Originals")을 사용하여 `Originals.kfx`와 `Originals.sdr`을 생성했습니다.
   - 이로 인해 사용자가 열람 중인 `Originals - Adam Grant.sdr`에는 `.kll`이 유입되지 않았습니다.

---

## 3. 상세 설계 및 수정 계획

### 3.1. CLI Word Wise 기본 활성화 (`src/main.rs`)
- `#[arg(long, default_value_t = true, action = clap::ArgAction::Set)]` 적용 (원할 경우 `--word-wise false`로 비활성화 가능).

### 3.2. 파이프라인의 자동 `.sdr` 사이드카 배포 (`src/pipeline.rs`)
- `kll_bytes`가 생성되면, `output_path`가 가리키는 위치 옆에 자동으로 `output_path.with_extension("sdr")` 폴더를 생성하고 `LanguageLayer.en.<asin>.kll`을 동기화.

### 3.3. 동기화 파일명 정합성 (`src/sync.rs`)
- `kfx_source.file_stem()`을 우선 사용하여 사용자가 지정한 파일명과 `.sdr` 폴더명이 1:1로 일치하도록 수정.

---

## 4. 검증 계획 (Verification Plan)
1. `cargo test --features gui` 전체 통과 (74개 이상).
2. `-o` 옵션만으로 변환 시 `.kfx`와 함께 `.sdr/LanguageLayer.en.<asin>.kll`이 자동 생성되는지 검증.
3. 킨들 단말기의 `Originals - Adam Grant.sdr`에 20KB 크기의 정상 `.kll` 배치 및 검증.
