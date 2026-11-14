use super::KindleStudio;
use crate::theme::Theme;
use gpui::*;

impl KindleStudio {
    /// Renders the Unified Dual-Pane Split View:
    /// Left Pane: Local Converter & Queue (50%)
    /// Center: Subtle Divider
    /// Right Pane: Connected Kindle Library & Storage (50%)
    pub(super) fn render_dual_pane_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .overflow_hidden()
            .child(
                div()
                    .id("left_pane_container")
                    .w_1_2()
                    .h_full()
                    .overflow_hidden()
                    .child(self.render_converter_pane(cx, true)),
            )
            .child(div().w_px().h_full().bg(Theme::border()))
            .child(
                div()
                    .id("right_pane_container")
                    .w_1_2()
                    .h_full()
                    .overflow_hidden()
                    .child(self.render_kindle_pane(cx, true)),
            )
    }
}
