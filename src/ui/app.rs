//! Main Application View for Kindle Scribe Studio in GPUI

use gpui::*;
use super::theme::Theme;
use super::state::{detect_kindle_device, ConversionSettings, ConversionTask, DeviceStatus, KindleFile, StudioTab, TaskStatus};

pub struct KindleStudio {
    tab: StudioTab,
    device: DeviceStatus,
    settings: ConversionSettings,
    tasks: Vec<ConversionTask>,
    files: Vec<KindleFile>,
    _search_query: String,
}

impl KindleStudio {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        let device = detect_kindle_device();
        
        let sample_tasks = vec![
            ConversionTask {
                id: 1,
                filename: "Contagious - Jonah Berger.epub".into(),
                file_size_str: "1.6 MB".into(),
                status: TaskStatus::Completed,
                progress: 1.0,
                word_wise: true,
                xray: true,
            },
            ConversionTask {
                id: 2,
                filename: "Andrew Huberman Decoded - Success Decoded.epub".into(),
                file_size_str: "249.5 KB".into(),
                status: TaskStatus::Completed,
                progress: 1.0,
                word_wise: true,
                xray: false,
            },
        ];

        let sample_files = vec![
            KindleFile {
                name: "Contagious - Jonah Berger.kfx".into(),
                format: "KFX".into(),
                size_formatted: "1.6 MB".into(),
                modified: "2026-10-13 22:15".into(),
            },
            KindleFile {
                name: "Zorba the Greek - Nikos Kazantzakis.kfx".into(),
                format: "KFX".into(),
                size_formatted: "840 KB".into(),
                modified: "2026-10-13 21:50".into(),
            },
            KindleFile {
                name: "Fourth Wing - Rebecca Yarros.kfx".into(),
                format: "KFX".into(),
                size_formatted: "2.1 MB".into(),
                modified: "2026-10-13 21:30".into(),
            },
            KindleFile {
                name: "Classic Reader Sample.mobi".into(),
                format: "MOBI".into(),
                size_formatted: "950 KB".into(),
                modified: "2026-10-11 14:20".into(),
            },
        ];

        Self {
            tab: StudioTab::Convert,
            device,
            settings: ConversionSettings::default(),
            tasks: sample_tasks,
            files: sample_files,
            _search_query: String::new(),
        }
    }

    fn render_header(&self) -> impl IntoElement {
        let conn_color = if self.device.connected {
            Theme::accent_green()
        } else {
            Theme::text_muted()
        };

        div()
            .flex()
            .justify_between()
            .items_center()
            .px_6()
            .py_4()
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
                            .child("⚡ Kindle Scribe Studio"),
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
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
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
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child(format!("{:.1} GB / {:.1} GB", self.device.used_gb, self.device.total_gb)),
                    ),
            )
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_convert = self.tab == StudioTab::Convert;
        let is_explorer = self.tab == StudioTab::Explorer;

        div()
            .flex()
            .gap_2()
            .px_6()
            .py_3()
            .bg(Theme::bg_root())
            .border_b_1()
            .border_color(Theme::border())
            .child(
                div()
                    .id("tab_convert")
                    .px_4()
                    .py_1p5()
                    .rounded_md()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .bg(if is_convert { Theme::bg_card() } else { hsla(0.0, 0.0, 0.0, 0.0) })
                    .text_color(if is_convert { Theme::accent_amber() } else { Theme::text_muted() })
                    .child("📥 도서 변환 & 전송 (Converter)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab = StudioTab::Convert;
                        cx.notify();
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
                    .bg(if is_explorer { Theme::bg_card() } else { hsla(0.0, 0.0, 0.0, 0.0) })
                    .text_color(if is_explorer { Theme::accent_amber() } else { Theme::text_muted() })
                    .child("📚 킨들 도서 탐색기 (Explorer)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab = StudioTab::Explorer;
                        cx.notify();
                    })),
            )
    }

    fn render_converter_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .p_6()
            .gap_6()
            // 1. Dropzone Card
            .child(
                div()
                    .id("dropzone_card")
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .p_8()
                    .rounded_xl()
                    .border_2()
                    .border_color(Theme::border())
                    .bg(Theme::bg_card())
                    .cursor_pointer()
                    .child(
                        div()
                            .text_2xl()
                            .text_color(Theme::accent_amber())
                            .child("📖 EPUB 파일을 드래그하여 놓으세요"),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_sm()
                            .text_color(Theme::text_muted())
                            .child("클릭하여 컴퓨터의 도서 파일을 변환할 수 있습니다 (샘플 도서 자동 감지)"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        let sample_path = std::path::PathBuf::from("../sample/Apocolocyntosis - Lucius Annaeus Seneca.epub");
                        if sample_path.exists() {
                            let task_id = this.tasks.len() as u64 + 1;
                            this.tasks.insert(0, ConversionTask {
                                id: task_id,
                                filename: "Apocolocyntosis - Lucius Annaeus Seneca.epub".into(),
                                file_size_str: "128 KB".into(),
                                status: TaskStatus::Converting,
                                progress: 0.5,
                                word_wise: this.settings.word_wise,
                                xray: false,
                            });
                            cx.notify();

                            match super::state::convert_epub_file(&sample_path, &this.settings, &this.device) {
                                Ok(out) => {
                                    if let Some(t) = this.tasks.iter_mut().find(|t| t.id == task_id) {
                                        t.status = TaskStatus::Completed;
                                        t.progress = 1.0;
                                    }
                                    let filename = out.file_name().unwrap_or_default().to_string_lossy().to_string();
                                    this.files.insert(0, KindleFile {
                                        name: filename,
                                        format: "KFX".into(),
                                        size_formatted: "120 KB".into(),
                                        modified: "방금 전".into(),
                                    });
                                }
                                Err(err) => {
                                    if let Some(t) = this.tasks.iter_mut().find(|t| t.id == task_id) {
                                        t.status = TaskStatus::Failed(err);
                                    }
                                }
                            }
                            cx.notify();
                        }
                    })),
            )
            // 2. Settings Bar
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .p_4()
                    .rounded_lg()
                    .bg(Theme::bg_card())
                    .border_1()
                    .border_color(Theme::border())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_primary())
                                    .child("콘텐츠 유형: EBOK (정식 전자책)"),
                            )
                            .child(
                                div()
                                    .id("toggle_word_wise")
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .bg(if self.settings.word_wise { Theme::accent_purple() } else { Theme::border() })
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if self.settings.word_wise { Theme::text_primary() } else { Theme::text_muted() })
                                    .child(if self.settings.word_wise { "Word Wise ON" } else { "Word Wise OFF" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.word_wise = !this.settings.word_wise;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(Theme::border())
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::text_muted())
                                    .child("X-Ray OFF"),
                            ),
                    )
                    .child(
                        div()
                            .id("toggle_auto_deploy")
                            .text_xs()
                            .cursor_pointer()
                            .text_color(if self.settings.auto_deploy { Theme::accent_green() } else { Theme::text_muted() })
                            .child(if self.settings.auto_deploy { "✓ 킨들 연결 시 자동 전송 활성화됨" } else { "✗ 킨들 자동 전송 비활성화됨" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings.auto_deploy = !this.settings.auto_deploy;
                                cx.notify();
                            })),
                    ),
            )
            // 3. Task Cards
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(self.tasks.iter().map(|task| {
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .p_4()
                            .rounded_lg()
                            .bg(Theme::bg_card())
                            .border_1()
                            .border_color(Theme::border())
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
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
                                            .child(format!("크기: {} | 소요시간: 0.28초", task.file_size_str)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .px_2()
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
                                            .px_2()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(Theme::accent_green())
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(Theme::text_primary())
                                            .child("변환 및 전송 완료"),
                                    ),
                            )
                    })),
            )
    }

    fn render_explorer_view(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .p_6()
            .gap_4()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::text_primary())
                            .child("킨들 내 저장된 도서 목록"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child(format!("총 {}권의 도서 확인됨", self.files.len())),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(self.files.iter().map(|file| {
                        let badge_bg = match file.format.as_str() {
                            "KFX" => Theme::accent_amber(),
                            "AZW3" => Theme::accent_blue(),
                            "MOBI" => Theme::accent_purple(),
                            _ => Theme::text_muted(),
                        };

                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .p_3()
                            .rounded_md()
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
                                            .px_2()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(badge_bg)
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(hsla(0.0, 0.0, 0.0, 1.0))
                                            .child(file.format.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(Theme::text_primary())
                                            .child(file.name.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_4()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(Theme::text_muted())
                                            .child(file.size_formatted.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(Theme::text_muted())
                                            .child(file.modified.clone()),
                                    ),
                            )
                    })),
            )
    }
}

impl Render for KindleStudio {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(Theme::bg_root())
            .child(self.render_header())
            .child(self.render_tabs(cx))
            .child(match self.tab {
                StudioTab::Convert => self.render_converter_view(cx).into_any_element(),
                StudioTab::Explorer => self.render_explorer_view().into_any_element(),
            })
    }
}
