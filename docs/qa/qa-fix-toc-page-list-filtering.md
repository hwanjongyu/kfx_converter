# QA Report: EPUB 3 Navigation & Page-List Filtering for Kindle Table of Contents

## 1. 개요 (Overview)
- **대상 작업:** EPUB 3 Navigation Document(`navDoc.xhtml`) 파싱 시 `<nav epub:type="toc">`와 `<nav epub:type="page-list">` 분리 격리
- **관련 파일:** [`src/epub.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/epub.rs)
- **일자:** 2026-10-19
- **최종 판정:** **PASS (74/74 Tests Passed, Real Book Verified & Sideloaded)**

---

## 2. 검증 항목 및 결과 (Verification Matrix)

| Test ID | 구분 | 검증 내용 | 검증 방식 | 결과 |
| :--- | :--- | :--- | :--- | :--- |
| **`TOC-01`** | 단위 테스트 | `parse_nav_xhtml`이 `toc`, `landmarks`, `page-list` 태그를 독립 컨텍스트로 파싱 | `test_parse_nav_xhtml_separates_toc_and_page_list` | **PASS** |
| **`TOC-02`** | 단위 테스트 | EPUB 2 NCX의 `navPoint`와 `pageTarget` 분리 검증 | `test_parse_ncx_and_nav_xhtml` | **PASS** |
| **`TOC-03`** | 실제 도서 검증 | `Originals - Adam Grant.epub`에서 335개의 인쇄 페이지를 제외하고 정확히 20개의 목차 챕터만 수집되는지 확인 | `test_epub_reader_real_originals_book_toc_and_page_list` | **PASS** |
| **`TOC-04`** | 회귀 테스트 | 전체 유닛/GUI 테스트 스위트 74개 통과 확인 | `cargo test --features gui` | **PASS** (71 lib + 3 UI) |
| **`TOC-05`** | 단말 배포 | 킨들 Paperwhite(`/media/drew/Kindle/documents/Originals - Adam Grant.kfx`) 배포 및 버퍼 플러시 | CLI 빌드 및 `sync` | **PASS** |

---

## 3. 세부 검증 로그

### 3.1. 도서 목차 항목 격리 검증 (`TOC-03`)
```
Originals - Adam Grant.epub 검증:
- TOC 수집 개수: 20개 (Cover, Praise, Also by Adam Grant, Title Page, Copyright, Dedication, Contents, Foreword, Chapters 1-8, Actions, Acknowledgments, References, Index)
- Page List 수집 개수: 335개 (i ~ xiii, 1 ~ 322)
- 목차 Drawer 항목에서 335개의 숫자 페이지 번호 유입 100% 차단 확인
```

### 3.2. 테스트 스위트 실행 결과 (`TOC-04`)
```
running 71 tests in src/lib.rs ... ok. 71 passed; 0 failed
running 3 tests in src/ui/main.rs ... ok. 3 passed; 0 failed
Total: 74 passed, 0 failed.
```

### 3.3. 단말 배포 검증 (`TOC-05`)
- 파일 위치: `/media/drew/Kindle/documents/Originals - Adam Grant.kfx`
- 생성 크기: 2,076.28 KB (2.08 MB)
- 변환 시간: 365.32 ms
- 파일 시스템 캐시 플러시(`sync`) 완료
