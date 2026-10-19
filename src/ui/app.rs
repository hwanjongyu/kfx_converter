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

        match super::state::convert_epub_file(&path.to_path_buf(), &self.settings, &self.device) {
            Ok(res) => {
                if let Some(t) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                    t.progress = 1.0;
                    if res.synced_to_device {
                        t.status = TaskStatus::Completed;
                    } else if let Some(err) = res.sync_error {
                        t.status = TaskStatus::SyncFailed(err);
                    } else {
                        t.status = TaskStatus::ConvertedLocalOnly;
                    }
                }

                // If file was truly synced to the device, reload the real Kindle files list
                if res.synced_to_device {
                    self.files = super::state::load_device_files(&self.device);
                }
            }
            Err(err) => {
                if let Some(t) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                    t.status = TaskStatus::Failed(err);
                }
            }
        }
        cx.notify();
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
            // 3. Task Cards
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
            .child(self.render_header(cx))
            .child(self.render_tabs(cx))
            .child(match self.tab {
                StudioTab::Convert => self.render_converter_view(cx).into_any_element(),
                StudioTab::Explorer => self.render_explorer_view().into_any_element(),
            })
    }
}
