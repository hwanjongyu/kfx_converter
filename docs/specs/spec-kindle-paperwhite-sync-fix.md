# Specification: Kindle Paperwhite 연결 감지 및 전송 상태 정합성 수정

## 1. 개요 및 문제 진단

### 1.1 현상 분석
1. **Kindle Paperwhite(WhitePaper) 미인식**:
   - PC에는 `/media/drew/Kindle` (USB 대용량 저장장치, UMS, FAT32)로 정상 마운트되어 있으나, 앱 UI에서는 "Kindle 미연결"로 표시됨.
2. **미전송 상태이나 전송 완료로 오표시**:
   - 실제 킨들 기기(`documents/`)로 파일이 복사되지 않았음에도 UI 상에서는 "변환 및 전송 완료"로 표시되고 "킨들 내 저장된 도서 목록"에 추가됨.

### 1.2 근본 원인 (Root Cause)
1. **기기 감지 로직의 UMS 누락 (`src/ui/state.rs`)**:
   - [`detect_kindle_device()`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/ui/state.rs#L81-L112)가 GVFS MTP 경로(`/run/user/{uid}/gvfs/mtp:host=...`)만 확인하도록 구현되어 있음.
   - Kindle Paperwhite와 같은 전통적인 UMS(USB Mass Storage) 마운트 지점(`/media/$USER/Kindle`, `/run/media/$USER/Kindle`, `/mnt/Kindle` 또는 `/proc/mounts`)을 전혀 조회하지 않음.
   - 용량 정보(`used_gb`, `total_gb`) 또한 `12.4 GB / 27.2 GB`로 하드코딩되어 실제 스토리지 상태를 반영하지 못함.
2. **전송 결과 분리 및 상태 불일치 (`src/ui/state.rs`, `src/ui/app.rs`)**:
   - [`convert_epub_file()`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/ui/state.rs#L186-L198)에서 기기 미연결 시 `sync_to_device`를 건너뛰며, 실행되더라도 `let _ = ...`로 에러를 무시함.
   - [`process_epub_file()`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/ui/app.rs#L132-L163)에서 KFX 로컬 변환만 성공하면 기기 전송 여부와 무관하게 `TaskStatus::Completed` ("변환 및 전송 완료")로 렌더링하고 `self.files`에 항목을 임의 추가함.
   - 실제 킨들의 `documents/` 실제 파일 목록 대신 하드코딩된 mock 파일 목록이 표시됨.

---

## 2. 해결 방안 (Specification)

### 2.1 기기 감지 엔진 통일 및 UMS 지원 강화 (`src/ui/state.rs`, `src/sync.rs`)
- `detect_kindle_device()`에서 GVFS MTP 뿐만 아니라 UMS 마운트(`/media/*`, `/run/media/*`, `/proc/mounts`)를 포괄적으로 검색.
- 마운트된 디렉터리 내에 `documents` 폴더나 `system` 폴더, 또는 Calibre/Kindle 메타데이터가 존재하는지 확인하여 신뢰성 있는 Kindle 감지 수행.
- `statvfs` (또는 `nix`/`libc::statvfs`)를 사용하여 기기의 실제 여유 공간 및 총 용량(`used_gb`, `total_gb`)을 정확히 산출.
- 모델명 감지: 시리얼 넘버/레이블 또는 `system/version.txt` 등을 확인하여 "Kindle Paperwhite" 또는 "Kindle"로 정확히 표시.

### 2.2 변환과 전송 파이프라인 명확한 분리 및 에러 전파
- `ConversionResult` 구조체 도입:
  - `kfx_path: PathBuf`
  - `synced_to_device: bool`
  - `sync_error: Option<String>`
- `settings.auto_deploy`가 켜져 있고 기기가 연결되어 있을 때:
  - 실제 `KindleSync::sync_to_device()`를 수행하고 에러 발생 시 `TaskStatus::Failed(SyncError)` 또는 명확한 경고 상태(`TaskStatus::ConvertedLocallyOnly`) 제공.
- 기기가 미연결된 상태라면:
  - "로컬 변환 완료 (기기 미연결)" 상태로 표시하여 사용자에게 전송되지 않았음을 명확히 고지.

### 2.3 킨들 탐색기(Explorer) 실제 디바이스 연동
- 앱 초기화 및 기기 연결 시 하드코딩된 mock 파일 대신, 연결된 킨들의 `documents/` 폴더를 실제로 스캔하여 `.kfx`, `.azw3`, `.mobi`, `.pdf` 파일 목록을 로드.
- 실제 킨들에 성공적으로 복사된 경우에만 탐색기 목록을 갱신.

---

## 3. 검증 계획 (Verification Plan)
1. 현재 연결된 `/media/drew/Kindle` 감지 확인 (`detect_kindle_device` 단위 테스트 및 빌드 검증).
2. 실제 도서 변환 시:
   - 기기 미연결 시: "로컬 변환 완료 (기기 미연결)" 표시 확인.
   - 기기 연결 시: `/media/drew/Kindle/documents/`로 정상 복사 및 UI에 "변환 및 전송 완료" 표시 확인.
3. `cargo test` 및 `cargo check` 통과 확인.
