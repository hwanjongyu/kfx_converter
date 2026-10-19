//! Main Application View for Kindle Scribe Studio in GPUI
//! Features a unified Dual-Pane Split View connecting local EPUB conversion
//! directly with the real-time connected Kindle device library.

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
    search_query: String,
    status_message: Option<(String, bool)>, // (text, is_error)
}

impl KindleStudio {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        let device = detect_kindle_device();
        let files = load_device_files(&device);

        Self {
            tab: StudioTab::DualPane,
            device,
            settings: ConversionSettings::default(),
            tasks: Vec::new(),
            files,
            filter_format: None,
            search_query: String::new(),
            status_message: None,
        }
    }

    pub fn refresh_device(&mut self, cx: &mut Context<Self>) {
        self.device = detect_kindle_device();
        self.files = load_device_files(&self.device);
        self.status_message = None;
        cx.notify();
    }

    pub fn eject_device(&mut self, cx: &mut Context<Self>) {
        match super::state::eject_kindle_device(&self.device) {
            Ok(_) => {
                self.status_message = Some(("킨들 기기가 안전하게 제거되었습니다.".into(), false));
                self.device = DeviceStatus::default();
                self.files.clear();
            }
            Err(err) => {
                self.status_message = Some((format!("연결 해제 실패: {}", err), true));
            }
        }
        cx.notify();
    }

    pub fn delete_file(&mut self, file_name: &str, cx: &mut Context<Self>) {
        if let Some(pos) = self.files.iter().position(|f| f.name == file_name) {
            let file = &self.files[pos];
            match super::state::delete_device_file(file) {
                Ok(_) => {
                    let msg = format!("'{}' 도서 및 캐시가 삭제되었습니다.", file_name);
                    self.files.remove(pos);
                    self.status_message = Some((msg, false));
                    self.device = detect_kindle_device();
                }
                Err(err) => {
                    self.status_message = Some((err, true));
                }
            }
            cx.notify();
        }
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
                    )
                    .children(self.status_message.as_ref().map(|(msg, is_err)| {
                        div()
                            .px_3()
                            .py_0p5()
                            .rounded_md()
                            .bg(if *is_err { Theme::accent_red() } else { Theme::accent_green() })
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
                    .child(
                        if self.device.connected {
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
                                                .bg(if used_fraction > 0.85 { Theme::accent_red() } else { Theme::accent_green() })
                                                .w(DefiniteLength::Fraction(used_fraction)),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(Theme::text_muted())
                                        .child(format!("{:.1}G/{:.1}G", self.device.used_gb, self.device.total_gb)),
                                )
                                .into_any_element()
                        } else {
                            div()
                                .text_xs()
                                .text_color(Theme::text_muted())
                                .child("기기 연결 필요")
                                .into_any_element()
                        }
                    )
                    .children(if self.device.connected {
                        Some(
                            div()
                                .id("btn_eject_device")
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .bg(Theme::bg_card_hover())
                                .cursor_pointer()
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(Theme::accent_amber())
                                .child("⏏ 안전 제거")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.eject_device(cx);
                                })),
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
                            .child("🔄 새로고침")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_device(cx);
                            })),
                    ),
            )
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_dual = self.tab == StudioTab::DualPane;
        let is_convert = self.tab == StudioTab::Convert;
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
                    .bg(if is_dual { Theme::bg_card() } else { hsla(0.0, 0.0, 0.0, 0.0) })
                    .text_color(if is_dual { Theme::accent_amber() } else { Theme::text_muted() })
                    .child("⚡ 듀얼 분할 뷰 (Dual-Pane)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab = StudioTab::DualPane;
                        this.refresh_device(cx);
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
                    .bg(if is_convert { Theme::bg_card() } else { hsla(0.0, 0.0, 0.0, 0.0) })
                    .text_color(if is_convert { Theme::accent_amber() } else { Theme::text_muted() })
                    .child("📥 변환 전용 (Converter)")
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
                    .child("📚 킨들 탐색기 (Explorer)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab = StudioTab::Explorer;
                        this.refresh_device(cx);
                    })),
            )
    }

    /// Renders the Local Conversion Pane (Dropzone, Settings, and Task Queue).
    fn render_converter_pane(&self, cx: &mut Context<Self>, in_split: bool) -> impl IntoElement {
        let p_class = if in_split { "p_4" } else { "p_6" };
        let _ = p_class;

        div()
            .flex()
            .flex_col()
            .p_4()
            .gap_4()
            .drag_over::<ExternalPaths>(|style, _paths, _window, _cx| {
                style
            })
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                for path in paths.paths() {
                    this.handle_incoming_path(path, cx);
                }
            }))
            // Pane Header
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
                            .child("📥 로컬 도서 변환 & 전송 (Converter)"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child("EPUB 드래그 앤 드롭 지원"),
                    ),
            )
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
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::accent_amber())
                            .child("📖 EPUB 도서를 여기에 놓으세요"),
                    )
                    .child(
                        div()
                            .mt_1p5()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child("클릭하여 탐색기에서 파일을 선택하거나 드래그하세요"),
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
                                Ok(Ok(None)) => {}
                                _ => {
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
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_primary())
                                    .child("포맷: EBOK"),
                            )
                            .child(
                                div()
                                    .id("toggle_word_wise")
                                    .px_2()
                                    .py_0p5()
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
                            ),
                    )
                    .child(
                        div()
                            .id("toggle_auto_deploy")
                            .text_xs()
                            .cursor_pointer()
                            .text_color(if self.settings.auto_deploy { Theme::accent_green() } else { Theme::text_muted() })
                            .child(if self.settings.auto_deploy { "✓ 킨들 자동전송" } else { "✗ 자동전송 끔" })
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
                                .px_2()
                                .py_0p5()
                                .rounded_md()
                                .bg(Theme::bg_card_hover())
                                .text_xs()
                                .text_color(Theme::text_muted())
                                .child("🗑 지우기")
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
                                "전송 완료".to_string(),
                                format!("{} | Kindle 동기화 완료", task.file_size_str),
                            ),
                            TaskStatus::ConvertedLocalOnly => (
                                Theme::accent_blue(),
                                "로컬 완료".to_string(),
                                format!("{} | 로컬 KFX 생성됨", task.file_size_str),
                            ),
                            TaskStatus::SyncFailed(err) => (
                                Theme::accent_red(),
                                "전송 실패".to_string(),
                                format!("{} | {}", task.file_size_str, err),
                            ),
                            TaskStatus::Converting => (
                                Theme::accent_amber(),
                                "변환 중...".to_string(),
                                format!("{} | 패키징 진행 중...", task.file_size_str),
                            ),
                            TaskStatus::Syncing => (
                                Theme::accent_purple(),
                                "전송 중...".to_string(),
                                format!("{} | Kindle 복사 중...", task.file_size_str),
                            ),
                            TaskStatus::Queued => (
                                Theme::border(),
                                "대기 중".to_string(),
                                format!("{}", task.file_size_str),
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

    /// Renders the Connected Kindle Library Pane (Storage Meter, Format Filters, Book List, Delete).
    fn render_kindle_pane(&self, cx: &mut Context<Self>, _in_split: bool) -> impl IntoElement {
        let is_offline = !self.device.connected;
        let active_filter = self.filter_format.as_deref().unwrap_or("ALL");

        let count_kfx = self.files.iter().filter(|f| f.format == "KFX").count();
        let count_wordwise = self.files.iter().filter(|f| f.has_word_wise).count();
        let count_azw3 = self.files.iter().filter(|f| f.format == "AZW3").count();
        let count_mobi = self.files.iter().filter(|f| f.format == "MOBI").count();
        let count_all = self.files.len();

        let filtered_files: Vec<&KindleFile> = self.files
            .iter()
            .filter(|f| {
                let fmt_match = match self.filter_format.as_deref() {
                    Some("KFX") => f.format == "KFX",
                    Some("WORDWISE") => f.has_word_wise,
                    Some("AZW3") => f.format == "AZW3",
                    Some("MOBI") => f.format == "MOBI",
                    _ => true,
                };
                let query_match = if self.search_query.is_empty() {
                    true
                } else {
                    f.name.to_lowercase().contains(&self.search_query.to_lowercase())
                };
                fmt_match && query_match
            })
            .collect();

        let free_gb = (self.device.total_gb - self.device.used_gb).max(0.0);
        let used_pct = if self.device.total_gb > 0.0 {
            (self.device.used_gb / self.device.total_gb * 100.0) as u32
        } else {
            0
        };
        let used_fraction = if self.device.total_gb > 0.0 {
            (self.device.used_gb / self.device.total_gb).clamp(0.0, 1.0)
        } else {
            0.0
        };

        div()
            .flex()
            .flex_col()
            .p_4()
            .gap_4()
            // Pane Header
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
                            .child("📚 킨들 기기 보관함 (Kindle Library)"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child(if is_offline {
                                "기기 미연결".to_string()
                            } else {
                                format!("총 {}권 (필터: {}권)", count_all, filtered_files.len())
                            }),
                    ),
            )
            // Storage Overview Card (when connected)
            .children(if !is_offline {
                Some(
                    div()
                        .flex()
                        .flex_col()
                        .p_3()
                        .gap_1p5()
                        .rounded_lg()
                        .bg(Theme::bg_card())
                        .border_1()
                        .border_color(Theme::border())
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(Theme::text_primary())
                                        .child(format!("점유율: {}%", used_pct)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(Theme::text_muted())
                                        .child(format!("여유 {:.1} GB / 총 {:.1} GB", free_gb, self.device.total_gb)),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .h_1p5()
                                .rounded_full()
                                .bg(Theme::border())
                                .child(
                                    div()
                                        .h_full()
                                        .rounded_full()
                                        .bg(if used_pct > 85 { Theme::accent_red() } else { Theme::accent_green() })
                                        .w(DefiniteLength::Fraction(used_fraction)),
                                ),
                        ),
                )
            } else {
                None
            })
            // Filter Chips Bar
            .children(if !is_offline {
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(
                            div()
                                .id("f_all")
                                .px_2p5()
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
                                .id("f_kfx")
                                .px_2p5()
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
                                .id("f_wordwise")
                                .px_2p5()
                                .py_1()
                                .rounded_full()
                                .cursor_pointer()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .bg(if active_filter == "WORDWISE" { Theme::accent_purple() } else { Theme::bg_card() })
                                .text_color(if active_filter == "WORDWISE" { Theme::text_primary() } else { Theme::text_muted() })
                                .child(format!("Word Wise ({})", count_wordwise))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.filter_format = Some("WORDWISE".into());
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("f_azw3")
                                .px_2p5()
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
                                .id("f_mobi")
                                .px_2p5()
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
            } else {
                None
            })
            // Book List or Empty States
            .child(
                if is_offline {
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .p_8()
                        .rounded_xl()
                        .border_1()
                        .border_color(Theme::border())
                        .bg(Theme::bg_card())
                        .gap_2p5()
                        .child(div().text_3xl().child("🔌"))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(Theme::text_primary())
                                .child("킨들 기기 연결 대기 중"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(Theme::text_muted())
                                .child("USB 케이블로 킨들을 PC에 연결하면 도서 목록이 실시간으로 표시됩니다."),
                        )
                        .child(
                            div()
                                .id("btn_reconnect_pane")
                                .cursor_pointer()
                                .mt_2()
                                .px_3p5()
                                .py_1()
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
                        .p_8()
                        .rounded_xl()
                        .border_1()
                        .border_color(Theme::border())
                        .bg(Theme::bg_card())
                        .gap_2()
                        .child(div().text_2xl().child("📚"))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(Theme::text_primary())
                                .child("해당 조건의 도서가 없습니다"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(Theme::text_muted())
                                .child("좌측 변환창에서 EPUB 도서를 변환·전송해 보세요."),
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

                            let file_name = file.name.clone();

                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .p_2p5()
                                .rounded_md()
                                .bg(Theme::bg_card())
                                .border_1()
                                .border_color(Theme::border())
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2p5()
                                        .child(
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(badge_bg)
                                                .text_xs()
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(hsla(0.0, 0.0, 0.0, 1.0))
                                                .child(file.format.clone()),
                                        )
                                        .children(if file.has_word_wise {
                                            Some(
                                                div()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded_sm()
                                                    .bg(Theme::accent_purple())
                                                    .text_xs()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(Theme::text_primary())
                                                    .child("Word Wise"),
                                            )
                                        } else {
                                            None
                                        })
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(Theme::text_primary())
                                                .child(file.name.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_3()
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
                                        )
                                        .child(
                                            div()
                                                .id(ElementId::Name(format!("del_{}", file_name).into()))
                                                .cursor_pointer()
                                                .px_2()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(Theme::bg_card_hover())
                                                .text_xs()
                                                .text_color(Theme::accent_red())
                                                .child("🗑")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.delete_file(&file_name, cx);
                                                })),
                                        ),
                                )
                        }))
                        .into_any_element()
                }
            )
    }

    /// Renders the Unified Dual-Pane Split View:
    /// Left Pane: Local Converter & Queue (50%)
    /// Center: Subtle Divider
    /// Right Pane: Connected Kindle Library & Storage (50%)
    fn render_dual_pane_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .child(
                div()
                    .id("left_pane_scroll")
                    .w_1_2()
                    .h_full()
                    .overflow_y_scroll()
                    .child(self.render_converter_pane(cx, true)),
            )
            .child(
                div()
                    .w_px()
                    .h_full()
                    .bg(Theme::border()),
            )
            .child(
                div()
                    .id("right_pane_scroll")
                    .w_1_2()
                    .h_full()
                    .overflow_y_scroll()
                    .child(self.render_kindle_pane(cx, true)),
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
                    .overflow_hidden()
                    .child(match self.tab {
                        StudioTab::DualPane => self.render_dual_pane_view(cx).into_any_element(),
                        StudioTab::Convert => div().id("convert_pane_scroll").size_full().overflow_y_scroll().child(self.render_converter_pane(cx, false)).into_any_element(),
                        StudioTab::Explorer => div().id("explorer_pane_scroll").size_full().overflow_y_scroll().child(self.render_kindle_pane(cx, false)).into_any_element(),
                    }),
            )
    }
}
