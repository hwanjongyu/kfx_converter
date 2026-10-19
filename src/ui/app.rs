//! Main Application View for Kindle Scribe Studio in GPUI

use gpui::*;
use super::theme::Theme;
use super::state::{
    detect_kindle_device, load_device_files, ConversionSettings, ConversionTask, DeviceStatus,
    KindleFile, StudioTab, TaskStatus,
};

pub struct KindleStudio {
    tab: StudioTab,
    device: DeviceStatus,
    settings: ConversionSettings,
    tasks: Vec<ConversionTask>,
    files: Vec<KindleFile>,
    filter_format: Option<String>,
    _search_query: String,
}

impl KindleStudio {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        let device = detect_kindle_device();
        let files = load_device_files(&device);

        Self {
            tab: StudioTab::Convert,
            device,
            settings: ConversionSettings::default(),
            tasks: Vec::new(),
            files,
            filter_format: None,
            _search_query: String::new(),
        }
    }

    pub fn refresh_device(&mut self, cx: &mut Context<Self>) {
        self.device = detect_kindle_device();
        self.files = load_device_files(&self.device);
        cx.notify();
    }

    pub fn handle_incoming_path(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if path.is_file() {
            if path
                .extension()
                .and_then(|s| s.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("epub"))
                .unwrap_or(false)
            {
                self.process_epub_file(path, cx);
            }
        } else if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let child_path = entry.path();
                    if child_path.is_file()
                        && child_path
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|ext| ext.eq_ignore_ascii_case("epub"))
                            .unwrap_or(false)
                    {
                        self.process_epub_file(&child_path, cx);
                    }
                }
            }
        }
    }

    pub fn process_epub_file(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let file_size_str = match std::fs::metadata(path) {
            Ok(meta) => {
                let bytes = meta.len();
                if bytes >= 1024 * 1024 {
                    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
                } else {
                    format!("{} KB", bytes / 1024)
                }
            }
            Err(_) => "Unknown".into(),
        };

        let task_id = (self.tasks.len() as u64) + 1;
        self.tasks.insert(0, ConversionTask {
            id: task_id,
            filename: filename.clone(),
            file_size_str,
            status: TaskStatus::Converting,
            progress: 0.5,
            word_wise: self.settings.word_wise,
            xray: false,
        });
        cx.notify();

        self.device = detect_kindle_device();

        let path_buf = path.to_path_buf();
        let settings = self.settings.clone();
        let device = self.device.clone();

        cx.spawn(async move |this, cx| {
            let task = cx.background_executor().spawn(async move {
                super::state::convert_epub_file(&path_buf, &settings, &device)
            });
            let res = task.await;

            this.update(cx, |this, cx| {
                if let Some(t) = this.tasks.iter_mut().find(|t| t.id == task_id) {
                    t.progress = 1.0;
                    match res {
                        Ok(res) => {
                            if res.synced_to_device {
                                t.status = TaskStatus::Completed;
                            } else if let Some(err) = res.sync_error {
                                t.status = TaskStatus::SyncFailed(err);
                            } else {
                                t.status = TaskStatus::ConvertedLocalOnly;
                            }
                            if res.synced_to_device {
                                this.files = super::state::load_device_files(&this.device);
                            }
                        }
                        Err(err) => {
                            t.status = TaskStatus::Failed(err);
                        }
                    }
                }
                cx.notify();
            }).ok();
        }).detach();
    }


    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                            .child(if self.device.connected {
                                format!("{:.1} GB / {:.1} GB", self.device.used_gb, self.device.total_gb)
                            } else {
                                "연결 확인 필요".to_string()
                            }),
                    )
                    .child(
                        div()
                            .id("btn_refresh_device")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(Theme::bg_card_hover())
                            .cursor_pointer()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child("🔄 새로고침")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_device(cx);
                            })),
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
                        this.refresh_device(cx);
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
                        this.refresh_device(cx);
                    })),
            )
    }

    fn render_converter_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .p_6()
            .gap_6()
            .drag_over::<ExternalPaths>(|style, _paths, _window, _cx| {
                style
            })
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                for path in paths.paths() {
                    this.handle_incoming_path(path, cx);
                }
            }))
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
                    .drag_over::<ExternalPaths>(|mut style, _paths, _window, _cx| {
                        style.border_color = Some(Theme::accent_amber().into());
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
                            .text_2xl()
                            .text_color(Theme::accent_amber())
                            .child("📖 EPUB 파일을 드래그하여 놓으세요"),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_sm()
                            .text_color(Theme::text_muted())
                            .child("클릭하여 컴퓨터의 도서 파일을 선택하거나, EPUB 파일을 여기로 드래그 앤 드롭하세요"),
                    )
                    .on_click(cx.listener(|_this, _, _, cx| {
                        let receiver = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: true,
                            prompt: Some("변환할 EPUB 도서를 선택하세요".into()),
                        });
                        cx.spawn(async move |this, cx| {
                            match receiver.await {
                                Ok(Ok(Some(paths))) => {
                                    this.update(cx, |this, cx| {
                                        for path in paths {
                                            this.handle_incoming_path(&path, cx);
                                        }
                                    }).ok();
                                }
                                Ok(Ok(None)) => {
                                    // User cancelled the file dialog
                                }
                                _ => {
                                    // File picker unsupported or failed, fallback to sample book if available
                                    let sample_path = std::path::PathBuf::from("../sample/Apocolocyntosis - Lucius Annaeus Seneca.epub");
                                    if sample_path.exists() {
                                        this.update(cx, |this, cx| {
                                            this.handle_incoming_path(&sample_path, cx);
                                        }).ok();
                                    }
                                }
                            }
                        }).detach();
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
            // 3. Task Queue Header & Clear Button
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::text_primary())
                            .child(if self.tasks.is_empty() {
                                "변환 대기열 (비어 있음)".to_string()
                            } else {
                                format!("변환 대기열 ({}개 작업)", self.tasks.len())
                            }),
                    )
                    .child(
                        if !self.tasks.is_empty() {
                            div()
                                .id("btn_clear_completed")
                                .cursor_pointer()
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .bg(Theme::bg_card_hover())
                                .text_xs()
                                .text_color(Theme::text_muted())
                                .child("🗑 완료된 작업 지우기")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.tasks.retain(|t| {
                                        t.status == TaskStatus::Converting || t.status == TaskStatus::Syncing || t.status == TaskStatus::Queued
                                    });
                                    cx.notify();
                                }))
                                .into_any_element()
                        } else {
                            div().into_any_element()
                        }
                    ),
            )
            // 4. Task Cards
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(self.tasks.iter().map(|task| {
                        let (badge_bg, badge_text, sub_desc) = match &task.status {
                            TaskStatus::Completed => (
                                Theme::accent_green(),
                                "변환 및 전송 완료".to_string(),
                                format!("크기: {} | Kindle 저장소 동기화 완료", task.file_size_str),
                            ),
                            TaskStatus::ConvertedLocalOnly => (
                                Theme::accent_blue(),
                                "로컬 변환 완료 (기기 미연결)".to_string(),
                                format!("크기: {} | 로컬 KFX 생성 완료 (Kindle 미연결)", task.file_size_str),
                            ),
                            TaskStatus::SyncFailed(err) => (
                                Theme::accent_red(),
                                "전송 실패".to_string(),
                                format!("크기: {} | 전송 오류: {}", task.file_size_str, err),
                            ),
                            TaskStatus::Converting => (
                                Theme::accent_amber(),
                                "변환 중...".to_string(),
                                format!("크기: {} | KFX 패키징 진행 중...", task.file_size_str),
                            ),
                            TaskStatus::Syncing => (
                                Theme::accent_purple(),
                                "기기 전송 중...".to_string(),
                                format!("크기: {} | Kindle 동기화 중...", task.file_size_str),
                            ),
                            TaskStatus::Queued => (
                                Theme::border(),
                                "대기 중".to_string(),
                                format!("크기: {}", task.file_size_str),
                            ),
                            TaskStatus::Failed(err) => (
                                Theme::accent_red(),
                                "변환 실패".to_string(),
                                format!("오류: {}", err),
                            ),
                        };

                        let is_active = matches!(task.status, TaskStatus::Converting | TaskStatus::Syncing | TaskStatus::Queued);

                        div()
                            .flex()
                            .flex_col()
                            .p_4()
                            .gap_3()
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
                                            .child(sub_desc),
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
                                            .bg(badge_bg)
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(Theme::text_primary())
                                            .child(badge_text),
                                    ),
                            )
                            .child(
                                if is_active {
                                    div()
                                        .w_full()
                                        .h_1p5()
                                        .rounded_full()
                                        .bg(Theme::border())
                                        .child(
                                            div()
                                                .h_full()
                                                .rounded_full()
                                                .bg(badge_bg)
                                                .w(DefiniteLength::Fraction(task.progress.clamp(0.08, 1.0))),
                                        )
                                        .into_any_element()
                                } else {
                                    div().into_any_element()
                                }
                            )
                    })),
            )
    }

    fn render_explorer_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_offline = !self.device.connected;
        let active_filter = self.filter_format.as_deref().unwrap_or("ALL");

        // Format counts
        let count_kfx = self.files.iter().filter(|f| f.format == "KFX").count();
        let count_azw3 = self.files.iter().filter(|f| f.format == "AZW3").count();
        let count_mobi = self.files.iter().filter(|f| f.format == "MOBI").count();
        let count_all = self.files.len();

        let filtered_files: Vec<&KindleFile> = if let Some(ref fmt) = self.filter_format {
            self.files.iter().filter(|f| &f.format == fmt).collect()
        } else {
            self.files.iter().collect()
        };

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
                            .child(if is_offline {
                                "기기 미연결".to_string()
                            } else {
                                format!("총 {}권 (필터 결과: {}권)", self.files.len(), filtered_files.len())
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("filter_all")
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .cursor_pointer()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .bg(if active_filter == "ALL" { Theme::accent_amber() } else { Theme::bg_card() })
                            .text_color(if active_filter == "ALL" { hsla(0.0, 0.0, 0.0, 1.0) } else { Theme::text_muted() })
                            .child(format!("전체 ({})", count_all))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.filter_format = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("filter_kfx")
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .cursor_pointer()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .bg(if active_filter == "KFX" { Theme::accent_amber() } else { Theme::bg_card() })
                            .text_color(if active_filter == "KFX" { hsla(0.0, 0.0, 0.0, 1.0) } else { Theme::text_muted() })
                            .child(format!("KFX ({})", count_kfx))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.filter_format = Some("KFX".into());
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("filter_azw3")
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .cursor_pointer()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .bg(if active_filter == "AZW3" { Theme::accent_blue() } else { Theme::bg_card() })
                            .text_color(if active_filter == "AZW3" { hsla(0.0, 0.0, 0.0, 1.0) } else { Theme::text_muted() })
                            .child(format!("AZW3 ({})", count_azw3))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.filter_format = Some("AZW3".into());
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("filter_mobi")
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .cursor_pointer()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .bg(if active_filter == "MOBI" { Theme::accent_purple() } else { Theme::bg_card() })
                            .text_color(if active_filter == "MOBI" { hsla(0.0, 0.0, 0.0, 1.0) } else { Theme::text_muted() })
                            .child(format!("MOBI ({})", count_mobi))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.filter_format = Some("MOBI".into());
                                cx.notify();
                            })),
                    ),
            )
            .child(
                if is_offline {
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .p_12()
                        .rounded_xl()
                        .border_1()
                        .border_color(Theme::border())
                        .bg(Theme::bg_card())
                        .gap_3()
                        .child(div().text_3xl().child("🔌"))
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::BOLD)
                                .text_color(Theme::text_primary())
                                .child("킨들 기기가 연결되어 있지 않습니다"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(Theme::text_muted())
                                .child("USB 케이블로 킨들을 PC에 연결한 후 아래 새로고침 버튼을 눌러주세요."),
                        )
                        .child(
                            div()
                                .id("btn_reconnect")
                                .cursor_pointer()
                                .mt_2()
                                .px_4()
                                .py_1p5()
                                .rounded_md()
                                .bg(Theme::accent_amber())
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(hsla(0.0, 0.0, 0.0, 1.0))
                                .child("🔄 기기 연결 다시 확인")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_device(cx);
                                })),
                        )
                        .into_any_element()
                } else if filtered_files.is_empty() {
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .p_12()
                        .rounded_xl()
                        .border_1()
                        .border_color(Theme::border())
                        .bg(Theme::bg_card())
                        .gap_2()
                        .child(div().text_3xl().child("📚"))
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::BOLD)
                                .text_color(Theme::text_primary())
                                .child("해당 조건의 도서가 없습니다"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(Theme::text_muted())
                                .child("변환 탭에서 EPUB 도서를 드래그하여 킨들로 전송해 보세요."),
                        )
                        .into_any_element()
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(filtered_files.into_iter().map(|file| {
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
                    }))
                    .into_any_element()
                }
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
            .child(self.render_header(cx))
            .child(self.render_tabs(cx))
            .child(
                div()
                    .flex_1()
                    .child(match self.tab {
                        StudioTab::Convert => self.render_converter_view(cx).into_any_element(),
                        StudioTab::Explorer => self.render_explorer_view(cx).into_any_element(),
                    })
            )
    }
}
