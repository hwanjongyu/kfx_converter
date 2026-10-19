# Specification: GUI UX/UI Polish & Interaction Enhancements

## 1. Overview & Objectives

Following the stabilization of core serialization and hardware sync, this specification governs the **GUI UX/UI Polish** for `kfx-studio` (powered by GPUI). It targets visual hierarchy, interaction responsiveness, feedback clarity, and usability in desktop environments:

1. **`UX-01` Animated / Visual Progress Bar on Conversion Tasks**:
   - Add visual progress bars (`h_1p5 rounded_full`) with dynamic width (`DefiniteLength::Fraction(progress)`) to tasks in `Converting` and `Syncing` states.
2. **`UX-02` Task Queue Management (Clear Completed / Cancel Tasks)**:
   - Provide a "모두 지우기 (Clear Completed)" button when tasks exist, allowing users to tidy up their workspace after bulk conversions.
3. **`UX-03` Explorer Empty & Offline States**:
   - Provide a visually distinct empty state card when no Kindle is connected ("킨들이 연결되어 있지 않습니다") with a retry button, and an empty state when connected but no books are found.
4. **`UX-04` Explorer Format Filtering & Count Badges**:
   - Add format filter chips (`전체`, `KFX`, `AZW3`, `MOBI`) with counts to quickly filter books in Kindle storage.
5. **`UX-05` Smooth Layout & Scroll Containers**:
   - Ensure `overflow_y_scroll()` is applied to both Converter task views and Explorer book listings so lengthy collections do not clip off screen.

---

## 2. Technical Design & Interfaces

### 2.1 Visual Progress Bar in Task Cards
In `render_converter_view`:
```rust
// Visual Progress Bar
if task.status == TaskStatus::Converting || task.status == TaskStatus::Syncing {
    div()
        .w_full()
        .h_1p5()
        .rounded_full()
        .bg(Theme::border())
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(if task.status == TaskStatus::Converting { Theme::accent_amber() } else { Theme::accent_purple() })
                .w(DefiniteLength::Fraction(task.progress.clamp(0.05, 1.0)))
        )
}
```

### 2.2 Task List Header & Clear Completed
Add a header above task cards:
```rust
div()
    .flex()
    .justify_between()
    .items_center()
    .child(div().text_sm().font_weight(FontWeight::BOLD).child(format!("변환 대기열 ({}개)", self.tasks.len())))
    .child(
        div()
            .id("btn_clear_tasks")
            .cursor_pointer()
            .text_xs()
            .text_color(Theme::text_muted())
            .hover(|s| s.text_color(Theme::accent_amber()))
            .child("완료된 작업 지우기")
            .on_click(...)
    )
```

### 2.3 Explorer Format Filter Chips
Add state `filter_format: Option<String>` in `KindleStudio`.
Render interactive filter buttons:
`전체 (All)`, `KFX`, `AZW3`, `MOBI`, `PDF` with item counts and active highlight.

### 2.4 Explorer Empty States
- When `!self.device.connected`: show illustration/icon, guidance to connect via USB UMS or MTP, and a large "기기 다시 검색" button.
- When `self.device.connected && self.files.is_empty()`: show "도서가 없습니다. EPUB을 변환하여 전송해 보세요."

---

## 3. Verification & Acceptance Criteria
- `cargo test --features gui` passes all 51+ unit and UI integration tests.
- UI elements compile cleanly with GPUI 0.2 element tree semantics.
- Task completion, clear queue, and format filtering behaviors tested.
