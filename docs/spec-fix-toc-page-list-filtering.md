# Spec: EPUB 3 Navigation & Page-List Filtering for Kindle Table of Contents

## 1. 개요 (Overview)
- **목적:** EPUB 3 네비게이션 문서(`navDoc.xhtml`) 파싱 시 `<nav epub:type="toc">`(실제 목차)와 `<nav epub:type="page-list">`(인쇄본 페이지 번호), `<nav epub:type="landmarks">`(랜드마크)를 명확히 구분하여, 킨들의 목차 서랍(Table of Contents Drawer)에 인쇄 페이지 번호(`i`~`xiii`, `1`~`322`)가 유입되지 않도록 격리합니다.
- **적용 대상:** [`src/epub.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/epub.rs), [`src/ir.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/ir.rs)

---

## 2. 현상 분석 및 원인 (Root Cause)
- EPUB 3 사양의 Navigation Document(`navDoc.xhtml`)는 책의 구조 정보를 여러 `<nav>` 블록으로 나누어 정의합니다:
  1. `<nav id="toc" epub:type="toc">`: 실제 챕터 목차 (20개 항목)
  2. `<nav epub:type="landmarks">`: 읽기 가이드 랜드마크 (3개 항목)
  3. `<nav epub:type="page-list" hidden="">`: 인쇄본 페이지 매핑 (335개 항목: `i`~`xiii`, `1`~`322`)
- 현재 [`parse_nav_xhtml`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/epub.rs#L517-L576)은 부모 `<nav>` 태그의 속성을 확인하지 않고 모든 `<a>` 태그를 `Vec<TocItem>`에 무조건 적재하여 총 358개 항목이 킨들의 `$391 toc_nav`로 전송되었습니다.

---

## 3. 상세 설계 및 변경 사항 (Design & Architecture)

### 3.1. 네비게이션 태그 컨텍스트 추적
- `enum NavContext` 도입:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  enum NavContext {
      Toc,
      PageList,
      Landmarks,
      Other,
  }
  ```
- XML 파서가 `<nav>` 시작 이벤트를 감지할 때 속성을 분석:
  - `epub:type.contains("toc") || role == "doc-toc" || id == "toc"` $\rightarrow$ `NavContext::Toc`
  - `epub:type.contains("page-list") || role == "doc-pagelist" || id == "page-list"` $\rightarrow$ `NavContext::PageList`
  - `epub:type.contains("landmarks")` $\rightarrow$ `NavContext::Landmarks`
  - 속성이 없는 단일 `<nav>`인 경우 (단순 EPUB 호환): 이전에 `Toc`가 수집되지 않았으면 `NavContext::Toc`, 그 외 `NavContext::Other`
- `<nav>` 깊이를 관리하기 위한 `nav_stack: Vec<NavContext>` 유지.

### 3.2. 링크(`<a>`) 라우팅
- 현재 활성화된 최상단 `NavContext`에 따라 분기:
  - **`NavContext::Toc`**: `(target_sec, target_anchor, title)`를 추출하여 `toc_items: Vec<TocItem>`에 추가.
  - **`NavContext::PageList`**: `(target_sec, target_anchor, page_number)`를 추출하여 `page_items: Vec<PageEntry>`에 추가.
  - **`NavContext::Landmarks` / `NavContext::Other`**: 목차 drawer에 노출되지 않도록 제외.

### 3.3. 함수 시그니처 및 BookIR 연동
- `parse_nav_xhtml`의 반환 타입을 `(Vec<TocItem>, Vec<PageEntry>)`로 변경.
- `extract_toc`를 `extract_navigation`으로 확장하여 `(toc, page_list)`를 반환하도록 수정하고, [`src/epub.rs:217-218`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/epub.rs#L217-L218)에서 `BookIR.toc`와 `BookIR.page_list`에 각각 바인딩.

---

## 4. 검증 계획 (Verification Plan)
1. **단위 테스트 추가:**
   - `test_parse_nav_xhtml_separates_toc_and_page_list`: `<nav epub:type="toc">`와 `<nav epub:type="page-list">`가 공존하는 EPUB 3 XHTML 샘플을 파싱하여, TOC에는 챕터만 수집되고 page-list에는 페이지 번호만 분리 수집되는지 검증.
2. **회귀 테스트:**
   - 전체 테스트 스위트 통과 확인 (`cargo test --features gui`).
3. **실제 도서 변환 및 킨들 검증:**
   - `Originals - Adam Grant.epub`를 변환하여 TOC 항목 수가 358개에서 **정확히 20개**로 축소되었는지 검증.
   - 단말기(`/media/drew/Kindle/documents/Originals - Adam Grant.kfx`)에 재동기화 후 확인.
