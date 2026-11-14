use super::KindleStudio;
use crate::state::{ConverterMode, TaskStatus};
use crate::theme::Theme;
use gpui::*;

impl KindleStudio {
    /// Renders the Local Conversion Pane (Dropzone, Settings, and Task Queue or Folder Browser).
    pub(super) fn render_converter_pane(
        &self,
        cx: &mut Context<Self>,
        in_split: bool,
    ) -> impl IntoElement {
        let is_folder = self.converter_mode == ConverterMode::FolderBrowser;

        if is_folder {
            return div()
                .flex()
                .flex_col()
                .size_full()
                .overflow_hidden()
                .p_4()
                .gap_3()
                .child(self.render_converter_header_and_mode_switcher(cx))
                .child(self.render_folder_browser_view(cx, in_split))
                .into_any_element();
        }

        let p_class = if in_split { "p_4" } else { "p_6" };
        let _ = p_class;

        div()
            .id("single_converter_scroll")
            .flex()
            .flex_col()
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .gap_4()
            .drag_over::<ExternalPaths>(|style, _paths, _window, _cx| style)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                for path in paths.paths() {
                    this.handle_incoming_path(path, cx);
                }
            }))
            .child(self.render_converter_header_and_mode_switcher(cx))
            // 1. Dropzone Card
            .child(
                div()
                    .id("dropzone_card")
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .p_6()
                    .rounded_xl()
                    .border_2()
                    .border_color(Theme::border())
                    .bg(Theme::bg_card())
                    .cursor_pointer()
                    .drag_over::<ExternalPaths>(|mut style, _paths, _window, _cx| {
                        style.border_color = Some(Theme::accent_amber());
                        style.background = Some(Theme::bg_card_hover().into());
                        style
                    })
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                        for path in paths.paths() {
                            this.handle_incoming_path(path, cx);
                        }
                    }))
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::accent_amber())
                            .child("📖 Drop EPUB books here"),
                    )
                    .child(
                        div()
                            .mt_1p5()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child("Click to browse files or drag and drop here"),
                    )
                    .on_click(cx.listener(|_this, _, _, cx| {
                        let receiver = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: true,
                            prompt: Some("Select EPUB books to convert".into()),
                        });
                        cx.spawn(async move |this, cx| match receiver.await {
                            Ok(Ok(Some(paths))) => {
                                this.update(cx, |this, cx| {
                                    for path in paths {
                                        this.handle_incoming_path(&path, cx);
                                    }
                                })
                                .ok();
                            }
                            Ok(Ok(None)) => {}
                            _ => {
                                let sample_path = std::path::PathBuf::from(
                                    "../sample/Apocolocyntosis - Lucius Annaeus Seneca.epub",
                                );
                                if sample_path.exists() {
                                    this.update(cx, |this, cx| {
                                        this.handle_incoming_path(&sample_path, cx);
                                    })
                                    .ok();
                                }
                            }
                        })
                        .detach();
                    })),
            )
            // 2. Settings Bar
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .p_3()
                    .rounded_lg()
                    .bg(Theme::bg_card())
                    .border_1()
                    .border_color(Theme::border())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .id("toggle_content_type")
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .bg(Theme::accent_amber())
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::text_primary())
                                    .child(format!("Format: {}", self.settings.content_type))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.content_type =
                                            if this.settings.content_type == "PDOC" {
                                                "EBOK".into()
                                            } else {
                                                "PDOC".into()
                                            };
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("toggle_word_wise")
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .bg(if self.settings.word_wise {
                                        Theme::accent_purple()
                                    } else {
                                        Theme::border()
                                    })
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if self.settings.word_wise {
                                        Theme::text_primary()
                                    } else {
                                        Theme::text_muted()
                                    })
                                    .child(if self.settings.word_wise {
                                        "Word Wise ON"
                                    } else {
                                        "Word Wise OFF"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.word_wise = !this.settings.word_wise;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("toggle_auto_deploy")
                            .text_xs()
                            .cursor_pointer()
                            .text_color(if self.settings.auto_deploy {
                                Theme::accent_green()
                            } else {
                                Theme::text_muted()
                            })
                            .child(if self.settings.auto_deploy {
                                "✓ Auto-send to Kindle"
                            } else {
                                "✗ Auto-send Off"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings.auto_deploy = !this.settings.auto_deploy;
                                cx.notify();
                            })),
                    ),
            )
            // 3. Queue Header & Clear Button
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::text_primary())
                            .child(if self.tasks.is_empty() {
                                "Conversion Queue (Empty)".to_string()
                            } else {
                                format!("Conversion Queue ({} tasks)", self.tasks.len())
                            }),
                    )
                    .child(if !self.tasks.is_empty() {
                        div()
                            .id("btn_clear_completed")
                            .cursor_pointer()
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .bg(Theme::bg_card_hover())
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child("🗑 Clear")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tasks.retain(|t| {
                                    t.status == TaskStatus::Converting
                                        || t.status == TaskStatus::Syncing
                                        || t.status == TaskStatus::Queued
                                });
                                cx.notify();
                            }))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            )
            // 4. Task Queue Cards
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2p5()
                    .children(self.tasks.iter().map(|task| {
                        let (badge_bg, badge_text, sub_desc) = match &task.status {
                            TaskStatus::Completed => (
                                Theme::accent_green(),
                                "Completed".to_string(),
                                format!("{} | Synced to Kindle", task.file_size_str),
                            ),
                            TaskStatus::ConvertedLocalOnly => (
                                Theme::accent_blue(),
                                "Local KFX".to_string(),
                                format!("{} | Local KFX created", task.file_size_str),
                            ),
                            TaskStatus::SyncFailed(err) => (
                                Theme::accent_red(),
                                "Sync Failed".to_string(),
                                format!("{} | {}", task.file_size_str, err),
                            ),
                            TaskStatus::Converting => (
                                Theme::accent_amber(),
                                "Converting...".to_string(),
                                format!("{} | Packaging in progress...", task.file_size_str),
                            ),
                            TaskStatus::Syncing => (
                                Theme::accent_purple(),
                                "Syncing...".to_string(),
                                format!("{} | Copying to Kindle...", task.file_size_str),
                            ),
                            TaskStatus::Queued => (
                                Theme::border(),
                                "Queued".to_string(),
                                task.file_size_str.to_string(),
                            ),
                            TaskStatus::Failed(err) => (
                                Theme::accent_red(),
                                "Failed".to_string(),
                                format!("Error: {}", err),
                            ),
                        };

                        let is_active = matches!(
                            task.status,
                            TaskStatus::Converting | TaskStatus::Syncing | TaskStatus::Queued
                        );

                        div()
                            .flex()
                            .flex_col()
                            .p_3()
                            .gap_2()
                            .rounded_lg()
                            .bg(Theme::bg_card())
                            .border_1()
                            .border_color(Theme::border())
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .items_start()
                                    .child(
                                        div()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .gap_0p5()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(Theme::text_primary())
                                                    .child(task.filename.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(Theme::text_muted())
                                                    .child(sub_desc),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1p5()
                                            .child(
                                                div()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded_sm()
                                                    .bg(Theme::accent_amber())
                                                    .text_xs()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(hsla(0.0, 0.0, 0.0, 1.0))
                                                    .child("KFX"),
                                            )
                                            .child(
                                                div()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded_sm()
                                                    .bg(badge_bg)
                                                    .text_xs()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(Theme::text_primary())
                                                    .child(badge_text),
                                            ),
                                    ),
                            )
                            .child(if is_active {
                                div()
                                    .w_full()
                                    .h_1p5()
                                    .rounded_full()
                                    .bg(Theme::border())
                                    .child(div().h_full().rounded_full().bg(badge_bg).w(
                                        DefiniteLength::Fraction(task.progress.clamp(0.08, 1.0)),
                                    ))
                                    .into_any_element()
                            } else {
                                div().into_any_element()
                            })
                    })),
            )
            .into_any_element()
    }

    pub(super) fn render_converter_header_and_mode_switcher(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_drop = self.converter_mode == ConverterMode::DropQueue;
        let is_folder = self.converter_mode == ConverterMode::FolderBrowser;

        div()
            .flex()
            .flex_col()
            .gap_2p5()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::text_primary())
                            .child("📥 Local Converter & Sync"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child("Supports EPUB drag & drop"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .p_1()
                    .rounded_lg()
                    .bg(Theme::bg_card())
                    .border_1()
                    .border_color(Theme::border())
                    .child(
                        div()
                            .id("btn_mode_drop")
                            .flex_1()
                            .py_1()
                            .rounded_md()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_center()
                            .cursor_pointer()
                            .bg(if is_drop {
                                Theme::bg_card_hover()
                            } else {
                                hsla(0.0, 0.0, 0.0, 0.0)
                            })
                            .text_color(if is_drop {
                                Theme::accent_amber()
                            } else {
                                Theme::text_muted()
                            })
                            .child("📥 Dropzone & Queue")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.converter_mode = ConverterMode::DropQueue;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("btn_mode_folder")
                            .flex_1()
                            .py_1()
                            .rounded_md()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_center()
                            .cursor_pointer()
                            .bg(if is_folder {
                                Theme::bg_card_hover()
                            } else {
                                hsla(0.0, 0.0, 0.0, 0.0)
                            })
                            .text_color(if is_folder {
                                Theme::accent_amber()
                            } else {
                                Theme::text_muted()
                            })
                            .child("📂 Folder Browser")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.converter_mode = ConverterMode::FolderBrowser;
                                cx.notify();
                            })),
                    ),
            )
    }
}
