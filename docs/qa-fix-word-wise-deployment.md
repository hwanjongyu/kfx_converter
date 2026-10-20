# QA Report: Fix Word Wise Automatic Generation and Sidecar Deployment

## 1. 개요 (Overview)
- **작업 내용:** Word Wise 기능 누락 원인 해결 (CLI 기본값 활성화, 출력 경로 `.sdr` 사이드카 자동 배포, 기기 동기화 파일명 정합성 확보)
- **관련 파일:** [`src/main.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/main.rs), [`src/pipeline.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/pipeline.rs), [`src/sync.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/sync.rs)
- **일자:** 2026-10-19
- **판정:** **PASS (74/74 Tests Passed, Real Book Word Wise Verified & Deployed)**

---

## 2. 검증 항목 및 결과 (Verification Matrix)

| Test ID | 구분 | 검증 내용 | 검증 방식 | 결과 |
| :--- | :--- | :--- | :--- | :--- |
| **`WW-01`** | CLI 기본값 | `-w` 옵션을 지정하지 않아도 기본적으로 Word Wise 사이드카가 자동 생성됨 | CLI 변환 실행 및 로그 확인 | **PASS** (`Word Wise sidecar generated (268 glosses)`) |
| **`WW-02`** | 사이드카 배포 | `-o`로 출력 파일 지정 시 출력 경로 옆에 `<filename>.sdr/LanguageLayer.en.<asin>.kll`이 자동 생성 | 파일 시스템 디렉토리 및 파일 존재 확인 | **PASS** (`LanguageLayer.en.034BE348BEAA0CDDBF568FB246A84883.kll`, 20KB) |
| **`WW-03`** | 동기화 정합성 | 기기 동기화 시 소스 파일명을 보존하여 엉뚱한 파일명으로 복제되거나 SDR이 분리되지 않음 | `KindleSync::sync_to_device` 로직 검증 | **PASS** |
| **`WW-04`** | 회귀 테스트 | 전체 유닛/GUI 테스트 스위트 74개 통과 확인 | `cargo test --features gui` | **PASS** (71 lib + 3 UI) |
| **`WW-05`** | 단말 배포 | 킨들 Paperwhite(`/media/drew/Kindle/documents/Originals - Adam Grant.sdr/LanguageLayer.en.034BE348BEAA0CDDBF568FB246A84883.kll`) 배포 및 버퍼 플러시 | CLI 빌드 및 `sync` | **PASS** |

---

## 3. 세부 결과
- **ASIN:** `034BE348BEAA0CDDBF568FB246A84883`
- **ACR:** `CR!D6U4CNNQCY7EVD2Y3CK6URXOINP2`
- **Glosses 수:** 268개
- **KLL 포맷:** SQLite 3 (metadata, glosses 테이블 정상 생성)
- **킨들 동기화:** 플래시 메모리 캐시 플러시(`sync`) 완료
