# Specification: KFX 목차(TOC) 복원 및 타이포그래피/스타일 포맷팅 엔진 구현

## 1. 개요 및 문제 진단

### 1.1 현상
- 킨들에서 변환된 KFX 도서를 열었을 때:
  1. **목차(Table of Contents) 미작동**: 킨들 메뉴의 목차 버튼이 비활성화되거나 챕터 목록이 표시되지 않음.
  2. **포맷/스타일 미적용**: 제목 크기 구분, 문단 간격, 줄바꿈, 굵게/기울임(Bold/Italic) 서식, 이미지 등이 모두 사라지고 밋밋한 단일 플레인 텍스트로만 렌더링됨.

### 1.2 근본 원인 분석
1. **EPUB 파서의 목차 누락 (`src/epub.rs`)**:
   - `EpubReader::read_from_file`에서 OPF 매니페스트 내의 `toc.ncx`(EPUB 2) 및 `nav.xhtml`(EPUB 3)을 탐색하거나 파싱하지 않고, `BookIR.toc`를 `Vec::new()`로 하드코딩하여 반환함.
2. **KFX 패키저의 네비게이션 엔티티 누락 (`src/packager.rs`)**:
   - KFX 포맷에서 목차를 킨들 UI에 표시하기 위해 필수적인 `book_navigation` ($389), `nav_container` ($391), `nav_unit` ($393), `toc` ($212) 엔티티 생성이 전무함.
   - `document_data` ($546)에 `book_navigation` 참조가 연결되지 않아 킨들 OS가 목차가 없는 책으로 인식함.
3. **블록 및 인라인 스타일 소실 (`src/packager.rs`)**:
   - `KfxPackager`가 블록을 패키징할 때 `b.plain_text()`로 모든 구조(Heading, Image, List, Span 스타일)를 플레인 텍스트로 뭉개버리고, 모든 요소를 `sym_type_text` ($269) 단일 텍스트로만 취급함.
   - `h1`~`h6`에 대한 폰트 크기/두께/마진 스타일 엔티티(`$159 = style`)와, 볼드/이탤릭/밑줄 인라인 서식을 KFX 엔진에 전달하는 `style_events` ($142: offset, length, style)가 전혀 구현되어 있지 않음.
   - 본문 내 `Block::Image`가 KFX 이미지 객체(`$270 = image`, `$162 = resource`)로 생성되지 않음.

---

## 2. 해결 방안 (Specification)

### 2.1 EPUB 목차(TOC) 파서 구현 (`src/epub.rs`)
- **EPUB 2 (NCX) 파서**:
  - `application/x-dtbncx+xml` 항목 검색 (`toc.ncx`).
  - `<navMap>` 계층 구조를 파싱하여 `navPoint`의 `navLabel/text`와 `content/@src` (섹션 파일명 및 `#anchor`)를 `TocItem` 트리로 변환.
- **EPUB 3 (Navigation Document) 파서**:
  - `properties="nav"` 또는 `nav.xhtml` 항목 검색.
  - `<nav epub:type="toc">`의 `<ol>/<li>/<a>` 태그를 파싱하여 `TocItem` 생성.
- **폴백(Fallback)**:
  - TOC 파일이 없는 경우, 책 내에 존재하는 `Block::Heading` 또는 `Section` 목록을 기반으로 자동 기본 목차 생성.

### 2.2 KFX 네비게이션 엔진 구축 (`src/packager.rs`)
- **KFX 심볼 등록**:
  - `book_navigation` (389), `nav_container` (391), `nav_unit` (393), `toc` (212), `nav_type` (235), `nav_unit_name` (240), `nav_container_name` (239).
- **네비게이션 엔티티 생성**:
  - `build_navigation()`: `BookIR.toc`를 순회하며 챕터별 `nav_unit`을 생성하고, 이를 담는 `nav_container` (타입: `toc`)를 구성.
  - 각 `nav_unit`에 제목 문자열, 타깃 섹션 심볼(`sec_sym_id`), 시작 위치(PID / block index) 바인딩.
- **Document Data 연동**:
  - `document_data` ($546) 구조체에 `book_navigation: nav_container_sym_id`를 추가하여 킨들이 책을 열었을 때 상단 목차 아이콘을 활성화.

### 2.3 서식/스타일(Typography & Styling) 엔진 구현 (`src/packager.rs`)
- **스타일 엔티티 정의**:
  - `h1`: `font_size: 1.6em`, `font_weight: bold`, `margin_top: 1.2em`, `margin_bottom: 0.6em`
  - `h2`: `font_size: 1.35em`, `font_weight: bold`, `margin_top: 1.0em`, `margin_bottom: 0.5em`
  - `h3`~`h6`: 적절한 단계별 em 크기 및 볼드 적용
  - `p`: 기본 문단 마진 및 텍스트 들여쓰기(`text_indent: 1.0em` 또는 `margin_bottom: 0.5em`)
  - `blockquote`: 좌우 여백(`margin_left: 1.5em`, `margin_right: 1.5em`, `font_style: italic`)
- **스토리라인 블록 매핑**:
  - `Block::Heading`: 제목 전용 스타일 ID 부여.
  - `Block::Paragraph`: 문단 스타일 부여 및 인라인 `TextSpan`의 볼드/이탤릭 정보를 KFX `style_events` ($142)로 직렬화하여 특정 글자 범위에 볼드/이탤릭 적용.
  - `Block::Image`: KFX `image` ($270) 스토리라인 객체로 생성하고 리소스 ID 바인딩.

---

## 3. 검증 계획 (Verification Plan)
1. **단위 테스트**:
   - NCX 및 NAV XHTML 파싱 테스트 (`EpubReader`).
   - KFX 네비게이션 엔티티(`book_navigation`, `nav_container`, `nav_unit`) 생성 및 Ion 직렬화 검증 (`packager::tests`).
   - 스타일 및 `style_events` 엔티티 구조 검증.
2. **실제 EPUB 도서 엔드투엔드 변환 검증**:
   - 샘플 EPUB 파일을 변환하여 생성된 KFX 컨테이너 내에 `book_navigation` 및 `style` 엔티티가 정확히 기록되었는지 확인.
3. `cargo test --all-targets` 회귀 테스트 통과 확인.
