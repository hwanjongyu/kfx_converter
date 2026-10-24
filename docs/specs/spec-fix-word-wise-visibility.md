# Spec: Fix Word Wise Invisibility & Calibration on Kindle Devices

## 1. 개요 (Overview)
- **현상:** 킨들 단말기에서 Word Wise 기능이 활성화(설정)되었음에도 불구하고, 실제 본문 열람 시 단어 위에 힌트(Gloss)가 단 하나도 표시되지 않는 문제.
- **목적:** 킨들 Yellowjacket 렌더러의 Word Wise 힌트 매칭 실패 원인(프론트매터 스킵, 난이도 1~2단계 공백, 슬라이더 임계치 불일치, PID 오프셋 드리프트, 과도한 페이싱 제한)을 완벽하게 교정하여, 단말기 슬라이더 위치(1~5단계)에 상관없이 첫 페이지부터 안정적으로 단어 힌트가 표시되도록 보장합니다.
- **적용 대상:**
  - [`src/enhancements/word_wise.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/enhancements/word_wise.rs)
  - [`src/enhancements/dictionary.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/enhancements/dictionary.rs)
  - [`src/pipeline.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/pipeline.rs)
  - [`src/packager.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/packager.rs)

---

## 2. 근본 원인 정밀 분석 (Root Cause Analysis)

실제 킨들 단말기(`/media/drew/Kindle`)의 공식 아마존 KFX 도서(`The War of the Worlds`, `The Time Machine`) 및 시스템 사전(`kll.en.en.klld`)과 본 프로젝트에서 생성된 `Originals - Adam Grant.sdr/LanguageLayer.en.*.kll`의 SQLite 데이터를 교차 분석한 결과, **5가지 중대한 결함**이 확인되었습니다.

### 2.1. 프론트매터 8,000자 무조건 스킵 (`frontmatter_skip_chars: 8000`)
- **원인:** [`src/enhancements/word_wise.rs:44`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/enhancements/word_wise.rs#L44)의 `WordWisePacingConfig` 기본값에 `frontmatter_skip_chars: 8000`이 지정되어 있습니다.
- **영향:** 8,000자는 킨들 기준 **약 10~12페이지(1,200~1,500 단어)**에 달합니다. 실제 단말기에 생성된 도서의 첫 번째 단어 힌트 시작 위치는 **9,506자(min(start) = 9506)**였습니다. 사용자가 책을 열고 챕터 1의 초반 5~10페이지를 읽는 동안 단 하나의 힌트도 출력되지 않습니다.

### 2.2. 난이도(Difficulty) 1~2단계 100% 공백 및 슬라이더 필터링 불일치
- **아마존 킨들의 동작 방식:**
  - 킨들 하단 오른쪽 **Word Wise 슬라이더**는 1단계("Fewer Hints")부터 5단계("More Hints")까지 지원합니다.
  - 슬라이더가 1~2단계(기본값 또는 적은 힌트)일 때 킨들은 `WHERE difficulty <= ?` 쿼리를 실행하여 **최고 난도 단어(Difficulty 1~2)**만 우선 표시합니다.
  - 아마존 공식 도서(`The War of the Worlds`)의 실제 난이도 분포:
    - **Difficulty 1:** 1,617개 (가장 어렵고 드문 어휘: *sphinx, camphor, vestige, somber, clamber...*)
    - **Difficulty 2:** 1,617개 (고급 어휘: *countess, sinister, blunder, automaton, caress...*)
    - **Difficulty 3:** 1,617개 (중상급 어휘: *lever, scarcely, glare, shiver, splendid...*)
    - **Difficulty 4:** 1,617개 (중급 어휘)
    - **Difficulty 5:** 1,614개 (일반 학술 어휘: 슬라이더를 최우측 "More Hints"로 둘 때 추가 표시)
- **우리 코드의 치명적 결함:**
  - [`src/enhancements/dictionary.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/enhancements/dictionary.rs)의 `BUILTIN_VOCABULARY`는 총 182단어에 불과하며, **Difficulty 1과 2가 단 1개도 정의되어 있지 않았습니다** (Difficulty 3: 14개, 4: 70개, 5: 184개).
  - 따라서 킨들 슬라이더가 기본값(1 또는 2)으로 되어 있는 사용자는 **도서 전체에서 일치하는 단어가 0개**가 되어 아무것도 보이지 않습니다.

### 2.3. 과도하게 억제된 페이싱 및 제한적 어휘 사전
- **원인:**
  - `max_glosses_per_window: 1` (800자=1페이지당 최대 1단어)
  - `min_distance: 80`자
  - `max_total_glosses: 500`개
- **영향:** 전체 도서(63만 자, 400페이지)에서 추출된 단어가 겨우 268개에 불과하며, 페이지당 단어가 0~1개 수준으로 드물어 실제 체감 상 Word Wise가 전혀 동작하지 않는 것처럼 보입니다.

### 2.4. Yellowjacket PID(Position ID) 누적 오프셋 불일치 위험
- **원인:**
  - [`src/packager.rs`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/packager.rs)에서는 빈 블록이나 이미지 블록의 길이를 `.max(1)`로 계산하여 position_id_map($265)에 반영합니다.
  - 반면 [`src/enhancements/word_wise.rs:232`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/enhancements/word_wise.rs#L232)에서는 `pid += char_idx;`로만 더해 빈 문단이나 대체 텍스트 없는 이미지에서 오프셋 드리프트가 누적되어 단어 시작 위치와 Yellowjacket PID가 어긋날 위험이 있었습니다.

### 2.5. Seed 및 Container/Content ID 재계산 불일치 위험
- **원인:** [`src/pipeline.rs:118`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/pipeline.rs#L118)에서 `packager` 인스턴스 외부에서 독립적으로 `seed`를 재계산하여 `content_id`와 `container_id`를 생성하고 있었습니다. 식별자(identifier) 유무에 따라 패키저 내부와 값이 달라질 수 있습니다.

---

## 3. 상세 수정 설계 (Design & Implementation Plan)

### 3.1. 페이싱 파라미터 현실화 (`src/enhancements/word_wise.rs`)
```rust
impl Default for WordWisePacingConfig {
    fn default() -> Self {
        Self {
            min_difficulty: 1,           // 1~5 전체 난이도 계층 수용
            target_difficulty: 3,        // 균형 기본값
            min_distance: 20,            // 단어 간 최소 거리 20자 (한 문단 내 1~2개 허용)
            max_glosses_per_window: 4,   // 800자(한 페이지)당 최대 4단어 힌트
            window_size: 800,
            frontmatter_skip_chars: 0,   // 책 첫 장부터 정상 단어 매칭 허용
            max_total_glosses: 10_000,   // 도서당 최대 힌트 수 대폭 상향
        }
    }
}
```

### 3.2. 어휘 사전 확장 및 1~5단계 정격 난이도 재배치 (`src/enhancements/dictionary.rs`)
킨들 공식 영영사전(`kll.en.en.klld`)의 유효한 `sense_id`를 기반으로, 1~5단계 전 구간에 걸쳐 핵심 어휘를 체계적으로 재배치합니다:
- **Difficulty 1 (GRE / CEFR C2 초고난도):** 슬라이더 최저(Fewer Hints)에서도 필수 노출 (예: *abstruse: 8435, acrimony: 42592, ameliorate: 35386, caterwaul: 7989, chimerical: 100161, defalcation: 105478, hebdomadal: 105139, recondite: 71322, sesquipedalian: 99005, vestige: 64146...*)
- **Difficulty 2 (Advanced CEFR C1 고급):** (예: *aberration: 10914, acquiesce: 19129, alacrity: 3846, arcane: 20376, audacious: 16205, circumspect: 24001, ephemeral: 3367, esoteric: 22280, ubiquitous: 52437...*)
- **Difficulty 3 (Upper-Intermediate CEFR B2/C1 중상급):** (예: *ambiguous: 24295, benevolent: 30789, contemplate: 4756, delineate: 13735, elaborate: 18417, eloquent: 39084, feasible: 4278, mitigate: 16026, plausible: 70711...*)
- **Difficulty 4 (Intermediate CEFR B2 중급):** (예: *abundant: 6756, adequate: 6120, anticipate: 8729, crucial: 751, diminish: 18524, integrate: 32170, profound: 54502, prominent: 47249, robust: 63202, subtle: 48265...*)
- **Difficulty 5 (Academic / General 고빈도 학술어휘):** 슬라이더 최고(More Hints) 시 노출 (예: *genuine: 21375, hesitate: 5308, immense: 7997, intense: 22987, persist: 61561, pursue: 68585, rational: 54323, sincere: 52396, valid: 55287...*)

### 3.3. Yellowjacket PID 계산 로직 동기화 (`src/enhancements/word_wise.rs`)
`packager.rs`의 블록 길이 계산 규칙과 100% 동일하게 일치시킵니다:
- 각 블록 길이: `let block_len = text.chars().count().max(1);`
- 빈 섹션 폴백: `if section.blocks.is_empty() { pid += 1; }`
- 섹션 루트: `pid += 1;`

### 3.4. 파이프라인 ID 직접 참조 보장 (`src/packager.rs`, `src/pipeline.rs`)
`KfxPackager`에 getter 함수(`content_id()`, `container_id()`)를 추가하여, `pipeline.rs`가 패키저가 확정한 식별자를 그대로 전달받아 `.kll` 및 `.sdr`을 생성하도록 보장합니다.

---

## 4. 검증 계획 (Verification Plan)
1. `cargo test --features gui` 전체 통과 (74개 이상).
2. `Originals - Adam Grant.epub` 재변환 검증:
   - 생성된 `.kll`의 `min(start)`가 0~500자 이내인지 확인.
   - 1단계부터 5단계까지 각 난이도별로 최소 수십 개 이상의 Gloss가 균일하게 분포하는지 검증 (`SELECT difficulty, count(*) FROM glosses`).
   - 총 Gloss 수가 500개 이상으로 정상 생성되는지 검증.
3. 킨들 단말기(`/media/drew/Kindle/documents/Originals - Adam Grant.sdr/`)에 최신 `.kll` 배포 및 단말기 화면 확인.
