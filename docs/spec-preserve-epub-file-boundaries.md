# Spec: Preserve EPUB File Boundaries & Eliminate Artificial Section Fragmentation

## 1. 개요 (Overview)
- **목적:** EPUB 원본 파일 단위(XHTML 단위)를 KFX 섹션 단위로 1:1 보존하여, 챕터 본문 도중(예: 챕터 6의 문단 21 "crew." 뒤) 임의 글자수(3,500자/15블록) 초과로 인한 강제 줄바꿈(페이지 넘김) 결함을 제거합니다.
- **적용 대상:** [`src/ir.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/ir.rs), [`src/packager.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/packager.rs)

---

## 2. 현상 분석 및 원인 (Root Cause)
- `src/ir.rs`의 `DEFAULT_MAX_SECTION_CHARS`(3,500자) 및 `DEFAULT_MAX_SECTION_BLOCKS`(15개) 임계값으로 인해, 보통 50,000자 내외인 일반 단행본 챕터가 10~15개의 조각(`_p1`, `_p2`...)으로 난도질되었습니다.
- 킨들 렌더러는 섹션 변경 시 강제 페이지 넘김을 수행하므로, 챕터 6의 문단 21(3,486자 시점)에서 다음 페이지로 뜬금없이 넘어가는 현상이 발생했습니다.

---

## 3. 상세 설계 및 변경 사항 (Design & Architecture)

### 3.1. 섹션 임계값 상향
- `DEFAULT_MAX_SECTION_CHARS`를 `1,000,000자`(백만 자), `DEFAULT_MAX_SECTION_BLOCKS`를 `10,000개`로 상향.
- 일반적인 도서의 모든 챕터(40KB~100KB)가 EPUB 원본 파일 단위 그대로 온전하게 1개의 섹션으로 유지됩니다.
- 단, 원본 전자책 제작자가 명시한 CSS 강제 페이지 넘김(`page-break-before`, `page-break-after`)은 여전히 존중되어 필요한 곳에서만 정확히 페이지가 나뉩니다.

### 3.2. 테스트 스위트 보강
- `test_packager_navigation_resolves_fragmented_sections`: 강제 페이지 넘김 스타일(`page_break_before`)을 포함하여 섹션 분할 시의 네비게이션 재매핑 로직을 확정적으로 검증.
- 챕터 6 원본 도서 검증 테스트 추가: `Originals`의 챕터 6이 15조각이 아닌 단 1개의 온전한 섹션으로 변환되는지 검증.

---

## 4. 검증 계획 (Verification Plan)
1. `cargo test --features gui` 전체 통과 (74개 이상).
2. `Originals - Adam Grant.epub` 변환 후 챕터 6 섹션 개수가 1개인지 확인.
3. 킨들 단말기(`/media/drew/Kindle/documents/Originals - Adam Grant.kfx`)에 최신 변환본 동기화.
