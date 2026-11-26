use super::KindleStudio;
use crate::state::StudioTab;
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;
use gpui::*;

impl KindleStudio {
    pub(super) fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let conn_color = if self.device.connected {
            Theme::accent_green()
        } else {
            Theme::text_muted()
        };

        let used_fraction = if self.device.total_gb > 0.0 {
            (self.device.used_gb / self.device.total_gb).clamp(0.0, 1.0)
        } else {
            0.0
        };

        div()
            .flex()
            .justify_between()
            .items_center()
            .px_6()
            .py_3p5()
            .bg(Theme::bg_card())
            .border_b_1()
            .border_color(Theme::border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::accent_amber())
                            .child("⚡ Kindle Studio"),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .bg(Theme::bg_card_hover())
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child("Native GPUI v0.1"),
                    )
                    .children(self.status_message.as_ref().map(|(msg, is_err)| {
                        div()
                            .px_3()
                            .py_0p5()
                            .rounded_md()
                            .bg(if *is_err {
                                Theme::accent_red()
                            } else {
                                Theme::accent_green()
                            })
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::text_primary())
                            .child(msg.clone())
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().w_2p5().h_2p5().rounded_full().bg(conn_color))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_primary())
                                    .child(self.device.model.clone()),
                            ),
                    )
                    .child(if self.device.connected {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w_16()
                                    .h_1p5()
                                    .rounded_full()
                                    .bg(Theme::border())
                                    .child(
                                        div()
                                            .h_full()
                                            .rounded_full()
                                            .bg(if used_fraction > 0.85 {
                                                Theme::accent_red()
                                            } else {
                                                Theme::accent_green()
                                            })
                                            .w(DefiniteLength::Fraction(used_fraction)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(Theme::text_muted())
                                    .child(format!(
                                        "{:.1}G/{:.1}G",
                                        self.device.used_gb, self.device.total_gb
                                    )),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child("Device Required")
                            .into_any_element()
                    })
                    .children(if self.device.connected {
                        let eject_disabled = self.eject_in_progress || self.has_active_tasks();
                        Some(
                            div()
                                .id("btn_eject_device")
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .bg(Theme::bg_card_hover())
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(if eject_disabled {
                                    Theme::text_muted()
                                } else {
                                    Theme::accent_amber()
                                })
                                .child(if self.eject_in_progress {
                                    "⏏ Ejecting..."
                                } else {
                                    "⏏ Eject Device"
                                })
                                .when(!eject_disabled, |el| {
                                    el.cursor_pointer().on_click(cx.listener(|this, _, _, cx| {
                                        this.eject_device(cx);
                                    }))
                                }),
                        )
                    } else {
                        None
                    })
                    .child(
                        div()
                            .id("btn_refresh_device")
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .bg(Theme::bg_card_hover())
                            .cursor_pointer()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child("🔄 Refresh")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_device(cx);
                            })),
                    ),
            )
    }

    pub(super) fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_dual = self.tab == StudioTab::DualPane;
        let is_convert = self.tab == StudioTab::Convert;
        let is_folder = self.tab == StudioTab::LocalFolder;
        let is_explorer = self.tab == StudioTab::Explorer;

        div()
            .flex()
            .gap_2()
            .px_6()
            .py_2p5()
            .bg(Theme::bg_root())
            .border_b_1()
            .border_color(Theme::border())
            .child(
                div()
                    .id("tab_dual")
                    .px_4()
                    .py_1p5()
                    .rounded_md()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .bg(if is_dual {
                        Theme::bg_card()
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .text_color(if is_dual {
                        Theme::accent_amber()
                    } else {
                        Theme::text_muted()
                    })
                    .child("⚡ Dual-Pane")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.switch_tab(StudioTab::DualPane, cx);
                    })),
            )
            .child(
                div()
                    .id("tab_convert")
                    .px_4()
                    .py_1p5()
                    .rounded_md()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .bg(if is_convert {
                        Theme::bg_card()
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .text_color(if is_convert {
                        Theme::accent_amber()
                    } else {
                        Theme::text_muted()
                    })
                    .child("📥 Converter Queue")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.switch_tab(StudioTab::Convert, cx);
                    })),
            )
            .child(
                div()
                    .id("tab_folder")
                    .px_4()
                    .py_1p5()
                    .rounded_md()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .bg(if is_folder {
                        Theme::bg_card()
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .text_color(if is_folder {
                        Theme::accent_amber()
                    } else {
                        Theme::text_muted()
                    })
                    .child("📂 Local Folder")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.switch_tab(StudioTab::LocalFolder, cx);
                    })),
            )
            .child(
                div()
                    .id("tab_explorer")
                    .px_4()
                    .py_1p5()
                    .rounded_md()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .bg(if is_explorer {
                        Theme::bg_card()
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .text_color(if is_explorer {
                        Theme::accent_amber()
                    } else {
                        Theme::text_muted()
                    })
                    .child("📚 Kindle Explorer")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.switch_tab(StudioTab::Explorer, cx);
                    })),
            )
    }
}
