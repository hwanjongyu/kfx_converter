# Specification: Kindle Sleep Screen Cover & KFX Compliance Fix

## 1. Executive Summary & Root Cause Analysis

### 왜 AZW3는 슬립화면 커버가 나오고 KFX는 나오지 않았는가?
- **AZW3 (MobiBooklet):** 레거시 MOBI C++ 렌더러를 사용하며, 파일 내부의 구조적 유효성 검증(Validation)이 느슨하여 메타데이터만 있으면 킨들의 `com.lab126.powerd`(절전모드 관리 데몬)에 정상 전달됩니다.
- **KFX (YellowjacketBooklet):** 킨들의 최신 KPR/Yellowjacket 네이티브 렌더러를 사용합니다. KFX 내부의 구조적 결함이나 청크 규격 위반이 발생하면 렌더러가 내부 예외(Internal Exception) 상태에 빠져, 전원 버튼을 눌러 슬립 상태로 진입할 때 `powerd`에 현재 책의 세션 및 커버 정보를 전달하지 못하고 기본 화면보호기(연필/활자 이미지)로 폴백됩니다.
- **EBOK vs PDOC의 진실:**
  - 규격에 맞는 정상적인 KFX 파일이라면 **EBOK와 PDOC 둘 다 슬립모드 커버가 정상 표시**됩니다.
  - 다만 `EBOK`는 킨들 Wi-Fi가 켜졌을 때 아마존 구매 내역과 대조하여 일치하지 않으면 킨들 OS가 썸네일을 주기적으로 삭제하는 버그(Amazon Cover Bug)가 있습니다 (Calibre 재연결이나 `amazon-cover-bug/` 캐시로 보존 가능).
  - `PDOC`는 아마존 클라우드 동기화 삭제를 타지 않으므로 사이드로딩에 가장 안전합니다.
  - 그러나 이전 변환본은 **KFX 규격 결함으로 인해 Yellowjacket 렌더러 자체가 에러 상태였기 때문에 EBOK이든 PDOC이든 커버가 뜨지 않았던 것**입니다.

---

## 2. Calibre `kfxlib` 실측 결과 발견된 7대 규격 결함

킨들 라이브러리 파일(`Originals - Adam Grant.kfx`)을 디코딩 분석한 결과 다음 에러들이 확인되었습니다:

1. **`Content chunk exceeds maximum (8192 bytes)`**:
   - 킨들 e-ink Yellowjacket 리더의 청크 버퍼 한계는 **8,192바이트**입니다.
   - 현재 구현은 챕터 전체(최대 112,315바이트)를 단일 `$145` 청크에 담아 킨들 리더의 페이징 및 버퍼 오버플로우를 유발했습니다.
2. **`Malformed auxiliary_data ($597)`**:
   - 보조 데이터 `$597` 엔티티는 오직 `$258`(`metadata`)와 `$598`(`kfx_id`) 키만 허용됩니다.
   - 현재 구현에서 `$617`(`yj_note`) 및 각주 팝업 요소들을 `$597`에 잘못 임베딩하여 킨들 파서가 auxiliary_data 전체를 기각했습니다.
3. **`ASIN != content_id` 불일치**:
   - 킨들과 Calibre는 사이드로딩 썸네일을 매칭할 때 `thumbnail_{ID}_{cde_content_type}_portrait.jpg`를 사용합니다.
   - 현재 `ASIN`(`B034BE348B`)과 `content_id`(`034BE348BEAA0CDDBF568FB246A84883`)가 서로 달라, Calibre가 생성한 썸네일 파일명과 킨들이 찾는 키가 엇갈렸습니다. (표준 KFX 및 reference_boko처럼 `ASIN`을 `content_id`와 동일한 32-hex 값으로 통일 필요).
4. **`reflow-section-size` 누락 ($585)**:
   - 섹션의 최대 PID 수가 65,536을 초과하는 대용량 책(`Originals`는 109,308 PIDs)은 `content_features`($585)에 `reflow-section-size` 메이저 버전 `(((max_pids - 65536) / 16384) + 2)` 선언이 필수입니다. 누락 시 킨들 하드웨어 렌더러가 크래시합니다.
5. **`yj_jpg_rst_marker_present` 누락 ($585)**:
   - JPEG 이미지에 재시작 마커(`\xff[\xd0-\xd7]`)가 포함되어 있을 때 `content_features`에 해당 플래그가 없으면 킨들 하드웨어 JPEG 디코더가 실패합니다.
6. **고아 프래그먼트 (`Unreferenced fragments`)**:
   - 표지 섹션(텍스트 없이 이미지만 존재)에 대해 아무런 텍스트가 참조하지 않는 빈 `c_section_1-content`($145)를 생성하여 KFX 컨테이너 인덱스 테이블에 고아 프래그먼트 오류를 발생시켰습니다.
   - 사용되지 않는 미참조 스타일 프래그먼트들도 컨테이너에 방치되었습니다.
7. **`cover_image` 메타데이터 리소스 심볼 일치**:
   - `kindle_title_metadata`의 `cover_image` 값은 파일명(`cover.jpg`)이 아니라 `$164` `external_resource`의 프래그먼트 심볼 ID와 정확히 일치해야 합니다.

---

## 3. 구현 계획 (Phase 2 상세)

### 1) [`src/packager.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager.rs) 수정
- **TextAccumulator 도입 (8,192바이트 청킹):**
  - 책 전체 텍스트를 누적하되, UTF-8 바이트 기준 8,192바이트 단위로 분할하여 `content_1`, `content_2`... 엔티티를 생성하고, 각 스토리라인 요소가 올바른 `(chunk_sym_id, index_in_chunk)`를 가리키도록 연결합니다.
  - 텍스트가 없는 섹션(표지 섹션)은 `$145` 엔티티를 생성하지 않습니다.
- **ASIN & content_id 단일화:**
  - `self.asin = self.content_id.clone()`으로 단일화하여 Calibre 썸네일 생성 및 킨들 디바이스 조회가 100% 일치하도록 보장합니다.
- **Auxiliary Data ($597) 규격화:**
  - `$597` 내부에서 `$617`(`yj_note`) 및 미등록 심볼을 제거하고, 순수하게 `kfx_id`와 `IS_TARGET_SECTION: true`만 남깁니다.
- **Dynamic Content Features ($585) 보강:**
  - `reflow-section-size`: `max_section_pids > 65536`일 때 공식에 따라 동적 추가.
  - `yj_jpg_rst_marker_present`: 이미지 바이트 스캔 후 마커 존재 시 추가.
  - `yj_table` / `yj_table_viewer`: 표가 존재할 때 자동 추가.
- **미사용 스타일 필터링:**
  - 스토리라인에서 실제로 참조된 스타일만 `$157` 프래그먼트로 방출하여 고아 프래그먼트 제거.

### 2) 테스트 및 검증
- `cargo test`: 기존 101개 테스트 및 신규 규격 검증 테스트 통과.
- `calibre-debug` + `kfxlib`: `Originals - Adam Grant.kfx`를 디코딩하여 0개 Error, 0개 Mismatch 확인.
- Calibre 라이브러리 및 킨들 사이드로딩 테스트.

---

## 4. 명시적 승인 게이트 (Pre-Approval Gate)
위 스펙 및 원인 분석을 검토하신 후 승인해주시면 Phase 2 (구현 및 검증 루프)를 진행하겠습니다.
