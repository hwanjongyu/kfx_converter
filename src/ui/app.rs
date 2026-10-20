//! Main Application View for Kindle Scribe Studio in GPUI
//! Features a unified Dual-Pane Split View connecting local EPUB conversion
//! directly with the real-time connected Kindle device library.

use gpui::*;
use super::theme::Theme;
use super::state::{
    detect_kindle_device, load_device_files, ConversionSettings, ConversionTask, ConverterMode,
    DeviceStatus, KindleFile, LocalEpubItem, LocalEpubStatus, LocalFolderState, StudioTab,
    TaskStatus,
};

pub struct KindleStudio {
    tab: StudioTab,
    converter_mode: ConverterMode,
    folder_state: LocalFolderState,
    device: DeviceStatus,
    settings: ConversionSettings,
    tasks: Vec<ConversionTask>,
    files: Vec<KindleFile>,
    filter_format: Option<String>,
    search_query: String,
    status_message: Option<(String, bool)>, // (text, is_error)
    folder_scroll_handle: ScrollHandle,
    explorer_scroll_handle: ScrollHandle,
}

impl KindleStudio {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        let device = detect_kindle_device();
        let files = load_device_files(&device);

        Self {
            tab: StudioTab::DualPane,
            converter_mode: ConverterMode::DropQueue,
            folder_state: LocalFolderState::default(),
            device,
            settings: ConversionSettings::default(),
            tasks: Vec::new(),
            files,
            filter_format: None,
            search_query: String::new(),
            status_message: None,
            folder_scroll_handle: ScrollHandle::new(),
            explorer_scroll_handle: ScrollHandle::new(),
        }
    }

    pub fn refresh_device(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let (device, files) = cx
                .background_executor()
                .spawn(async move {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    let dev = detect_kindle_device();
                    let f = load_device_files(&dev);
                    (dev, f)
                })
                .await;

            this.update(cx, |this, cx| {
                this.device = device;
                this.files = files;
                this.status_message = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn eject_device(&mut self, cx: &mut Context<Self>) {
        match super::state::eject_kindle_device(&self.device) {
            Ok(_) => {
                self.status_message = Some(("Kindle device safely ejected.".into(), false));
                self.device = DeviceStatus::default();
                self.files.clear();
            }
            Err(err) => {
                self.status_message = Some((format!("Eject failed: {}", err), true));
            }
        }
        cx.notify();
    }

    pub fn delete_file(&mut self, file_name: &str, cx: &mut Context<Self>) {
        if let Some(pos) = self.files.iter().position(|f| f.name == file_name) {
            let file = &self.files[pos];
            match super::state::delete_device_file(file) {
                Ok(_) => {
                    let msg = format!("'{}' book and cache deleted.", file_name);
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

    pub fn select_folder(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select folder containing EPUB books".into()),
        });

        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(folder) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| {
                    this.folder_state.current_folder = Some(folder);
                    this.rescan_folder(cx);
                }).ok();
            }
        }).detach();
    }

    pub fn rescan_folder(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = self.folder_state.current_folder.clone() else {
            return;
        };

        self.folder_state.is_scanning = true;
        cx.notify();

        let recursive = self.folder_state.recursive_scan;
        let device = self.device.clone();

        cx.spawn(async move |this, cx| {
            let items = cx.background_executor().spawn(async move {
                super::state::scan_local_epub_directory(&folder, recursive, &device)
            }).await;

            this.update(cx, |this, cx| {
                this.folder_state.items = items;
                this.folder_state.is_scanning = false;
                cx.notify();
            }).ok();
        }).detach();
    }

    pub fn toggle_select_all(&mut self, select: bool, cx: &mut Context<Self>) {
        for item in &mut self.folder_state.items {
            item.is_selected = select;
        }
        cx.notify();
    }

    pub fn select_unconverted_only(&mut self, cx: &mut Context<Self>) {
        for item in &mut self.folder_state.items {
            item.is_selected = item.status == super::state::LocalEpubStatus::Unconverted;
        }
        cx.notify();
    }

    pub fn toggle_item_selection(&mut self, item_id: u64, cx: &mut Context<Self>) {
        if let Some(item) = self.folder_state.items.iter_mut().find(|i| i.id == item_id) {
            item.is_selected = !item.is_selected;
        }
        cx.notify();
    }

    pub fn convert_selected_epubs(&mut self, sync_to_kindle: bool, cx: &mut Context<Self>) {
        let selected_paths: Vec<std::path::PathBuf> = self.folder_state.items
            .iter()
            .filter(|i| i.is_selected)
            .map(|i| i.path.clone())
            .collect();

        if selected_paths.is_empty() {
            self.status_message = Some(("Please select books to convert first.".into(), true));
            cx.notify();
            return;
        }

        let count = selected_paths.len();
        self.status_message = Some((format!("Starting bounded conversion of {} books (2 concurrent workers)...", count), false));

        if sync_to_kindle {
            self.settings.auto_deploy = true;
        }

        for item in self.folder_state.items.iter_mut() {
            if item.is_selected {
                item.status = LocalEpubStatus::InQueue;
            }
        }

        let mut queued_tasks = std::collections::VecDeque::new();
        for path in &selected_paths {
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
            let task_id = (self.tasks.len() as u64) + (queued_tasks.len() as u64) + 1;
            queued_tasks.push_back((task_id, path.clone()));
            self.tasks.insert(0, ConversionTask {
                id: task_id,
                filename,
                file_size_str,
                status: TaskStatus::Queued,
                progress: 0.0,
                word_wise: self.settings.word_wise,
                xray: false,
            });
        }
        cx.notify();

        let settings = self.settings.clone();
        let device = self.device.clone();

        // Bounded worker pool: max 2 concurrent conversion workers to safeguard memory
        let queue = std::sync::Arc::new(std::sync::Mutex::new(queued_tasks));
        let num_workers = 2.min(selected_paths.len());

        for _ in 0..num_workers {
            let q = queue.clone();
            let s_clone = settings.clone();
            let d_clone = device.clone();

            cx.spawn(async move |this, cx| {
                loop {
                    let next = {
                        let mut guard = match q.lock() {
                            Ok(g) => g,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        guard.pop_front()
                    };

                    let Some((task_id, path_buf)) = next else {
                        break;
                    };

                    let _ = this.update(cx, |this, cx| {
                        if let Some(t) = this.tasks.iter_mut().find(|t| t.id == task_id) {
                            t.status = TaskStatus::Converting;
                            t.progress = 0.5;
                        }
                        cx.notify();
                    });

                    let target_path = path_buf.clone();
                    let s = s_clone.clone();
                    let d = d_clone.clone();
                    let res = cx.background_executor().spawn(async move {
                        super::state::convert_epub_file(&target_path, &s, &d)
                    }).await;

                    let _ = this.update(cx, |this, cx| {
                        if let Some(t) = this.tasks.iter_mut().find(|t| t.id == task_id) {
                            t.progress = 1.0;
                            match &res {
                                Ok(r) => {
                                    if r.synced_to_device {
                                        t.status = TaskStatus::Completed;
                                    } else {
                                        t.status = TaskStatus::ConvertedLocalOnly;
                                    }
                                }
                                Err(err) => {
                                    t.status = TaskStatus::Failed(err.clone());
                                }
                            }
                        }
                        if let Some(item) = this.folder_state.items.iter_mut().find(|i| i.path == path_buf) {
                            match &res {
                                Ok(r) => {
                                    if r.synced_to_device {
                                        item.status = LocalEpubStatus::OnDevice;
                                    } else {
                                        item.status = LocalEpubStatus::ConvertedLocal;
                                    }
                                }
                                Err(err) => {
                                    item.status = LocalEpubStatus::Error(err.clone());
                                }
                            }
                        }
                        this.refresh_device(cx);
                        cx.notify();
                    });
                }
            }).detach();
        }

        cx.notify();
    }

    pub fn convert_single_epub(&mut self, path: std::path::PathBuf, sync_to_kindle: bool, cx: &mut Context<Self>) {
        if let Some(item) = self.folder_state.items.iter_mut().find(|i| i.path == path) {
            item.status = LocalEpubStatus::InQueue;
        }
        if sync_to_kindle {
            self.settings.auto_deploy = true;
        }
        self.process_epub_file(&path, cx);
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
        } else if path.is_dir()
            && let Ok(entries) = std::fs::read_dir(path)
        {
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
        let target_path = path_buf.clone();
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
                                this.refresh_device(cx);
                            }

                            if let Some(item) = this.folder_state.items.iter_mut().find(|i| i.path == target_path) {
                                item.status = if res.synced_to_device {
                                    LocalEpubStatus::OnDevice
                                } else {
                                    LocalEpubStatus::ConvertedLocal
                                };
                                item.is_selected = false;
                            }
                        }
                        Err(err) => {
                            t.status = TaskStatus::Failed(err.clone());
                            if let Some(item) = this.folder_state.items.iter_mut().find(|i| i.path == target_path) {
                                item.status = LocalEpubStatus::Error(err);
                            }
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
                                .child("Device Required")
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
                                .child("⏏ Eject Device")
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
                            .child("🔄 Refresh")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_device(cx);
                            })),
                    ),
            )
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .bg(if is_dual { Theme::bg_card() } else { hsla(0.0, 0.0, 0.0, 0.0) })
                    .text_color(if is_dual { Theme::accent_amber() } else { Theme::text_muted() })
                    .child("⚡ Dual-Pane")
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
                    .child("📥 Converter Queue")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab = StudioTab::Convert;
                        this.refresh_device(cx);
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
                    .bg(if is_folder { Theme::bg_card() } else { hsla(0.0, 0.0, 0.0, 0.0) })
                    .text_color(if is_folder { Theme::accent_amber() } else { Theme::text_muted() })
                    .child("📂 Local Folder")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab = StudioTab::LocalFolder;
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
                    .child("📚 Kindle Explorer")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab = StudioTab::Explorer;
                        this.refresh_device(cx);
                    })),
            )
    }

    /// Renders the Local Conversion Pane (Dropzone, Settings, and Task Queue or Folder Browser).
    fn render_converter_pane(&self, cx: &mut Context<Self>, in_split: bool) -> impl IntoElement {
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
            .drag_over::<ExternalPaths>(|style, _paths, _window, _cx| {
                style
            })
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
                                    .child("Format: EBOK"),
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
                            .child(if self.settings.auto_deploy { "✓ Auto-send to Kindle" } else { "✗ Auto-send Off" })
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
                                .child("🗑 Clear")
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
            .into_any_element()
    }

    fn render_converter_header_and_mode_switcher(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                            .bg(if is_drop { Theme::bg_card_hover() } else { hsla(0.0, 0.0, 0.0, 0.0) })
                            .text_color(if is_drop { Theme::accent_amber() } else { Theme::text_muted() })
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
                            .bg(if is_folder { Theme::bg_card_hover() } else { hsla(0.0, 0.0, 0.0, 0.0) })
                            .text_color(if is_folder { Theme::accent_amber() } else { Theme::text_muted() })
                            .child("📂 Folder Browser")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.converter_mode = ConverterMode::FolderBrowser;
                                cx.notify();
                            })),
                    ),
            )
    }

    /// Renders the Local Folder EPUB Browser View (Path selector, Filter/Selection bar, Table, Batch actions).
    fn render_folder_browser_view(&self, cx: &mut Context<Self>, in_split: bool) -> impl IntoElement {
        let _ = in_split;
        let selected_count = self.folder_state.items.iter().filter(|i| i.is_selected).count();
        let total_count = self.folder_state.items.len();

        let filtered_items: Vec<&LocalEpubItem> = self.folder_state.items.iter().filter(|item| {
            if !self.folder_state.search_query.is_empty() {
                let q = self.folder_state.search_query.to_lowercase();
                if !item.title.to_lowercase().contains(&q)
                    && !item.author.to_lowercase().contains(&q)
                    && !item.filename.to_lowercase().contains(&q)
                {
                    return false;
                }
            }
            if let Some(ref filter) = self.folder_state.filter_status {
                match filter.as_str() {
                    "unconverted" => item.status == LocalEpubStatus::Unconverted,
                    "converted" => item.status == LocalEpubStatus::ConvertedLocal,
                    "on_device" => item.status == LocalEpubStatus::OnDevice,
                    _ => true,
                }
            } else {
                true
            }
        }).collect();

        div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .gap_3()
            // 1. Folder Selection Header Card
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .p_3()
                    .rounded_xl()
                    .bg(Theme::bg_card())
                    .border_1()
                    .border_color(Theme::border())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::text_muted())
                                    .child("📂 Local EPUB Library Folder"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_primary())
                                    .child(
                                        self.folder_state.current_folder
                                            .as_ref()
                                            .map(|p| p.to_string_lossy().to_string())
                                            .unwrap_or_else(|| "No folder selected (Click Choose Folder)".into())
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("btn_select_folder")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(Theme::accent_amber())
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::bg_root())
                                    .cursor_pointer()
                                    .child("📁 Choose Folder")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.select_folder(cx);
                                    })),
                            )
                            .child(
                                div()
                                    .id("btn_rescan_folder")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(Theme::bg_card_hover())
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_primary())
                                    .cursor_pointer()
                                    .child(if self.folder_state.is_scanning { "⏳ Scanning..." } else { "🔄 Refresh" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.rescan_folder(cx);
                                    })),
                            ),
                    )
            )
            // 2. Selection & Filter Toolbar
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .p_2p5()
                    .rounded_lg()
                    .bg(Theme::bg_card())
                    .border_1()
                    .border_color(Theme::border())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("btn_select_all")
                                    .px_2p5()
                                    .py_1()
                                    .rounded_md()
                                    .bg(Theme::bg_card_hover())
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_muted())
                                    .cursor_pointer()
                                    .child("✓ Select All")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_select_all(true, cx);
                                    })),
                            )
                            .child(
                                div()
                                    .id("btn_deselect_all")
                                    .px_2p5()
                                    .py_1()
                                    .rounded_md()
                                    .bg(Theme::bg_card_hover())
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_muted())
                                    .cursor_pointer()
                                    .child("Deselect All")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_select_all(false, cx);
                                    })),
                            )
                            .child(
                                div()
                                    .id("btn_select_unconverted")
                                    .px_2p5()
                                    .py_1()
                                    .rounded_md()
                                    .bg(Theme::bg_card_hover())
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::accent_amber())
                                    .cursor_pointer()
                                    .child("⭐ Unconverted Only")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.select_unconverted_only(cx);
                                    })),
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
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_muted())
                                    .child(format!("{}/{} books selected", selected_count, total_count)),
                            )
                            .child(
                                div()
                                    .id("btn_top_batch_convert")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(if selected_count > 0 { Theme::accent_amber() } else { Theme::bg_card_hover() })
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if selected_count > 0 { Theme::bg_root() } else { Theme::text_muted() })
                                    .cursor_pointer()
                                    .child(format!("⚡ Convert ({})", selected_count))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.convert_selected_epubs(false, cx);
                                    })),
                            )
                            .child(
                                div()
                                    .id("btn_top_batch_convert_sync")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(if selected_count > 0 && self.device.connected { Theme::accent_green() } else { Theme::bg_card_hover() })
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if selected_count > 0 && self.device.connected { Theme::bg_root() } else { Theme::text_muted() })
                                    .cursor_pointer()
                                    .child(format!("📲 Sync to Kindle ({})", selected_count))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.convert_selected_epubs(true, cx);
                                    })),
                            ),
                    ),
            )
            // 3. Scrollable EPUB List Table with Visible Scrollbar
            .child(
                div()
                    .flex()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .overflow_hidden()
                    .rounded_xl()
                    .bg(Theme::bg_card())
                    .border_1()
                    .border_color(Theme::border())
                    .p_2()
                    .gap_1p5()
                    .child(
                        div()
                            .id("folder_epub_list_scroll")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .overflow_x_hidden()
                            .track_scroll(&self.folder_scroll_handle)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .w_full()
                                    .gap_1p5()
                                    .pr_1p5()
                                    .children(if filtered_items.is_empty() {
                                        vec![
                                            div()
                                                .flex()
                                                .flex_col()
                                                .items_center()
                                                .justify_center()
                                                .p_8()
                                                .rounded_xl()
                                                .child(
                                                    div()
                                                        .text_base()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(Theme::text_muted())
                                                        .child(
                                                            if self.folder_state.current_folder.is_none() {
                                                                "Click 'Choose Folder' above to select your EPUB directory."
                                                            } else {
                                                                "No EPUB books found in this folder."
                                                            }
                                                        ),
                                                )
                                                .into_any_element(),
                                        ]
                                    } else {
                                        filtered_items.iter().map(|item| {
                                            let item_id = item.id;
                                            let item_path = item.path.clone();
                                            let is_sel = item.is_selected;
                                            let status_badge = match &item.status {
                                                LocalEpubStatus::Unconverted => {
                                                    div().px_2().py_0p5().rounded_md().bg(Theme::bg_card_hover()).text_xs().text_color(Theme::text_muted()).child("⚪ Unconverted")
                                                }
                                                LocalEpubStatus::InQueue => {
                                                    div().px_2().py_0p5().rounded_md().bg(Theme::accent_amber()).text_xs().font_weight(FontWeight::BOLD).text_color(Theme::bg_root()).child("🟡 Converting")
                                                }
                                                LocalEpubStatus::ConvertedLocal => {
                                                    div().px_2().py_0p5().rounded_md().bg(Theme::accent_blue()).text_xs().font_weight(FontWeight::BOLD).text_color(Theme::text_primary()).child("🔵 Local KFX")
                                                }
                                                LocalEpubStatus::OnDevice => {
                                                    div().px_2().py_0p5().rounded_md().bg(Theme::accent_green()).text_xs().font_weight(FontWeight::BOLD).text_color(Theme::bg_root()).child("🟢 On Kindle")
                                                }
                                                LocalEpubStatus::Error(err) => {
                                                    div().px_2().py_0p5().rounded_md().bg(Theme::accent_red()).text_xs().text_color(Theme::text_primary()).child(format!("Error: {}", err))
                                                }
                                            };

                                            let quick_btn = if item.status == LocalEpubStatus::Unconverted {
                                                let p = item_path.clone();
                                                Some(
                                                    div()
                                                        .id(("btn_quick_convert", item_id as usize))
                                                        .px_2()
                                                        .py_0p5()
                                                        .rounded_md()
                                                        .bg(Theme::accent_amber())
                                                        .text_xs()
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(Theme::bg_root())
                                                        .cursor_pointer()
                                                        .child("⚡ Convert")
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            this.convert_single_epub(p.clone(), false, cx);
                                                        }))
                                                )
                                            } else {
                                                None
                                            };

                                            div()
                                                .id(("epub_item", item_id as usize))
                                                .flex()
                                                .justify_between()
                                                .items_center()
                                                .w_full()
                                                .overflow_hidden()
                                                .p_3()
                                                .rounded_xl()
                                                .bg(if is_sel { Theme::bg_card_hover() } else { Theme::bg_card() })
                                                .border_1()
                                                .border_color(if is_sel { Theme::accent_amber() } else { Theme::border() })
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.toggle_item_selection(item_id, cx);
                                                }))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_3()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .child(
                                                            div()
                                                                .flex_shrink_0()
                                                                .w_5()
                                                                .h_5()
                                                                .rounded_md()
                                                                .border_1()
                                                                .border_color(if is_sel { Theme::accent_amber() } else { Theme::border() })
                                                                .bg(if is_sel { Theme::accent_amber() } else { hsla(0.0, 0.0, 0.0, 0.0) })
                                                                .flex()
                                                                .items_center()
                                                                .justify_center()
                                                                .text_xs()
                                                                .font_weight(FontWeight::BOLD)
                                                                .text_color(Theme::bg_root())
                                                                .child(if is_sel { "✓" } else { "" }),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex()
                                                                .flex_col()
                                                                .gap_0p5()
                                                                .flex_1()
                                                                .min_w_0()
                                                                .overflow_hidden()
                                                                .child(
                                                                    div()
                                                                        .text_sm()
                                                                        .font_weight(FontWeight::BOLD)
                                                                        .text_color(Theme::text_primary())
                                                                        .truncate()
                                                                        .child(item.title.clone()),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .flex()
                                                                        .gap_2()
                                                                        .text_xs()
                                                                        .text_color(Theme::text_muted())
                                                                        .truncate()
                                                                        .child(format!("Author: {}", item.author))
                                                                        .child("•")
                                                                        .child(item.file_size_str.clone())
                                                                        .child("•")
                                                                        .child(item.modified_str.clone()),
                                                                ),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .flex_shrink_0()
                                                        .flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .child(status_badge)
                                                        .children(quick_btn)
                                                )
                                                .into_any_element()
                                        }).collect()
                                    })
                            )
                    )
                    .child(
                        self.render_custom_scrollbar(&self.folder_scroll_handle, filtered_items.len(), "folder", cx)
                    )
            )
            // 4. Bottom Batch Actions Bar
            .child(
                div()
                    .flex()
                    .gap_3()
                    .pt_2()
                    .child(
                        div()
                            .id("btn_batch_convert")
                            .flex_1()
                            .py_2p5()
                            .rounded_xl()
                            .bg(if selected_count > 0 { Theme::accent_amber() } else { Theme::bg_card_hover() })
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(if selected_count > 0 { Theme::bg_root() } else { Theme::text_muted() })
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(format!("⚡ Convert Selected ({})", selected_count))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.convert_selected_epubs(false, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("btn_batch_convert_sync")
                            .flex_1()
                            .py_2p5()
                            .rounded_xl()
                            .bg(if selected_count > 0 && self.device.connected { Theme::accent_green() } else { Theme::bg_card_hover() })
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(if selected_count > 0 && self.device.connected { Theme::bg_root() } else { Theme::text_muted() })
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(format!("📲 Convert & Sync to Kindle ({})", selected_count))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.convert_selected_epubs(true, cx);
                            })),
                    )
            )
    }

    /// Renders a visible custom scrollbar track & thumb bound to a GPUI ScrollHandle.
    fn render_custom_scrollbar(
        &self,
        scroll_handle: &ScrollHandle,
        item_count: usize,
        id_prefix: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let handle_for_down = scroll_handle.clone();
        let handle_for_move = scroll_handle.clone();
        let handle_for_paint = scroll_handle.clone();

        let track_id = ElementId::Name(format!("{}_scrollbar_track", id_prefix).into());

        div()
            .id(track_id)
            .flex_shrink_0()
            .w(px(14.0))
            .h_full()
            .relative()
            .rounded_md()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(move |_this, ev: &MouseDownEvent, _, cx| {
                let track_bounds = handle_for_down.bounds();
                let measured_max = handle_for_down.max_offset().height;
                let effective_max = if measured_max > px(1.0) {
                    measured_max
                } else if item_count > 0 {
                    let est = px((item_count as f32) * 68.0);
                    (est - track_bounds.size.height).max(px(0.0))
                } else {
                    px(0.0)
                };

                if track_bounds.size.height > px(0.0) && effective_max > px(1.0) {
                    let click_y = ev.position.y;
                    let rel_y = (click_y - track_bounds.origin.y).clamp(px(0.0), track_bounds.size.height);
                    let target_ratio = (rel_y / track_bounds.size.height).clamp(0.0, 1.0);
                    let target_offset = -(effective_max * target_ratio);
                    handle_for_down.set_offset(point(px(0.0), target_offset));
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(move |_this, ev: &MouseMoveEvent, _, cx| {
                if ev.dragging() {
                    let track_bounds = handle_for_move.bounds();
                    let measured_max = handle_for_move.max_offset().height;
                    let effective_max = if measured_max > px(1.0) {
                        measured_max
                    } else if item_count > 0 {
                        let est = px((item_count as f32) * 68.0);
                        (est - track_bounds.size.height).max(px(0.0))
                    } else {
                        px(0.0)
                    };

                    if track_bounds.size.height > px(0.0) && effective_max > px(1.0) {
                        let click_y = ev.position.y;
                        let rel_y = (click_y - track_bounds.origin.y).clamp(px(0.0), track_bounds.size.height);
                        let target_ratio = (rel_y / track_bounds.size.height).clamp(0.0, 1.0);
                        let target_offset = -(effective_max * target_ratio);
                        handle_for_move.set_offset(point(px(0.0), target_offset));
                        cx.notify();
                    }
                }
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |track_bounds, _, window, _| {
                        // 1. Paint scrollbar track background
                        let mut track_quad = fill(track_bounds, Theme::scrollbar_track());
                        track_quad.corner_radii = (4.0).into();
                        track_quad.border_widths = (1.0).into();
                        track_quad.border_color = Theme::border();
                        window.paint_quad(track_quad);

                        // 2. Real-time thumb geometry calculation at paint time
                        let viewport_h = track_bounds.size.height;
                        let measured_max = handle_for_paint.max_offset().height;
                        let effective_max = if measured_max > px(1.0) {
                            measured_max
                        } else if item_count > 0 {
                            let est_content = px((item_count as f32) * 68.0);
                            if est_content > viewport_h {
                                est_content - viewport_h
                            } else {
                                px(0.0)
                            }
                        } else {
                            px(0.0)
                        };

                        // Only render thumb if there is scrollable content
                        if effective_max > px(1.0) && viewport_h > px(20.0) {
                            let total_content_h = viewport_h + effective_max;
                            let scroll_offset_y = (-handle_for_paint.offset().y).clamp(px(0.0), effective_max);
                            let scroll_ratio = (scroll_offset_y / effective_max).clamp(0.0, 1.0);

                            // Proportional thumb size, clamped:
                            // Min: 36px so it's always easily draggable
                            // Max: 70% of viewport so it NEVER fills the entire height and always leaves dragging room
                            let raw_thumb_h = (viewport_h / total_content_h) * viewport_h;
                            let max_allowed_thumb = (viewport_h * 0.70).max(px(36.0));
                            let thumb_h = raw_thumb_h.clamp(px(36.0), max_allowed_thumb.min(viewport_h - px(10.0)));

                            let available_travel = (viewport_h - thumb_h - px(6.0)).max(px(0.0));
                            let thumb_top = track_bounds.origin.y + px(3.0) + (available_travel * scroll_ratio);
                            let thumb_left = track_bounds.origin.x + px(2.0);
                            let thumb_w = (track_bounds.size.width - px(4.0)).max(px(4.0));

                            let thumb_bounds = Bounds {
                                origin: point(thumb_left, thumb_top),
                                size: size(thumb_w, thumb_h),
                            };

                            // 3. Paint vibrant golden scrollbar thumb
                            let mut thumb_quad = fill(thumb_bounds, Theme::scrollbar_thumb());
                            thumb_quad.corner_radii = (4.0).into();
                            thumb_quad.border_widths = (1.0).into();
                            thumb_quad.border_color = Theme::border();
                            window.paint_quad(thumb_quad);
                        }
                    },
                )
                .size_full(),
            )
    }

    /// Renders the full-screen Local Folder Tab view.
    fn render_local_folder_full_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .p_6()
            .child(self.render_folder_browser_view(cx, false))
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
        let file_count = filtered_files.len();

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
            .size_full()
            .overflow_hidden()
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
                            .child("📚 Kindle Device Library"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child(if is_offline {
                                "Disconnected".to_string()
                            } else {
                                format!("Total {} books (Filtered: {})", count_all, filtered_files.len())
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
                                        .child(format!("Used: {}%", used_pct)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(Theme::text_muted())
                                        .child(format!("Free {:.1} GB / Total {:.1} GB", free_gb, self.device.total_gb)),
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
                                .child(format!("All ({})", count_all))
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
                                .child("Waiting for Kindle Device Connection"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(Theme::text_muted())
                                .child("Connect your Kindle via USB cable to view and manage books in real time."),
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
                                .child("🔄 Recheck Connection")
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
                                .child("No matching books found"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(Theme::text_muted())
                                .child("Convert and sync EPUB books from the left pane."),
                        )
                        .into_any_element()
                } else {
                    div()
                        .flex()
                        .flex_1()
                        .w_full()
                        .min_h_0()
                        .overflow_hidden()
                        .rounded_xl()
                        .bg(Theme::bg_card())
                        .border_1()
                        .border_color(Theme::border())
                        .p_2()
                        .gap_1p5()
                        .child(
                            div()
                                .id("kindle_files_list_scroll")
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .overflow_y_scroll()
                                .overflow_x_hidden()
                                .track_scroll(&self.explorer_scroll_handle)
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .w_full()
                                        .gap_2()
                                        .pr_1p5()
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
                                                .w_full()
                                                .overflow_hidden()
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
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .child(
                                                            div()
                                                                .flex_shrink_0()
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
                                                                    .flex_shrink_0()
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
                                                                .flex_1()
                                                                .min_w_0()
                                                                .text_xs()
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .text_color(Theme::text_primary())
                                                                .truncate()
                                                                .child(file.name.clone()),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .flex_shrink_0()
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
                                )
                        )
                        .child(
                            self.render_custom_scrollbar(&self.explorer_scroll_handle, file_count, "kindle", cx)
                        )
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
            .overflow_hidden()
            .child(
                div()
                    .id("left_pane_container")
                    .w_1_2()
                    .h_full()
                    .overflow_hidden()
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
                    .id("right_pane_container")
                    .w_1_2()
                    .h_full()
                    .overflow_hidden()
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
                        StudioTab::LocalFolder => div().id("folder_pane_container").size_full().child(self.render_local_folder_full_tab(cx)).into_any_element(),
                        StudioTab::Explorer => div().id("explorer_pane_container").size_full().child(self.render_kindle_pane(cx, false)).into_any_element(),
                    }),
            )
    }
}
