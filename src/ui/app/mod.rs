//! Main Application View for Kindle Studio in GPUI
//! Features a unified Dual-Pane Split View connecting local EPUB conversion
//! directly with the real-time connected Kindle device library.

mod actions;
mod components;
mod converter;
mod dual_pane;
mod folder_browser;
mod header;
mod kindle_explorer;

use crate::state::{
    ConversionSettings, ConversionTask, ConverterMode, DeviceStatus, EpubSortField, KindleFile,
    LocalFolderState, SortDirection, StudioTab, detect_kindle_device, load_device_files,
};
use crate::theme::Theme;
use gpui::*;

pub struct KindleStudio {
    pub(super) tab: StudioTab,
    pub(super) converter_mode: ConverterMode,
    pub(super) folder_state: LocalFolderState,
    pub(super) device: DeviceStatus,
    pub(super) settings: ConversionSettings,
    pub(super) tasks: Vec<ConversionTask>,
    pub(super) files: Vec<KindleFile>,
    pub(super) filter_format: Option<String>,
    pub(super) search_query: String,
    pub(super) status_message: Option<(String, bool)>, // (text, is_error)
    pub(super) batch_cancel_token: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub(super) folder_scroll_handle: UniformListScrollHandle,
    pub(super) explorer_scroll_handle: UniformListScrollHandle,
    pub(super) explorer_sort_field: EpubSortField,
    pub(super) explorer_sort_direction: SortDirection,
    pub(super) confirm_delete_file: Option<String>,
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
            batch_cancel_token: None,
            folder_scroll_handle: UniformListScrollHandle::new(),
            explorer_scroll_handle: UniformListScrollHandle::new(),
            explorer_sort_field: EpubSortField::Title,
            explorer_sort_direction: SortDirection::Ascending,
            confirm_delete_file: None,
        }
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
                div().flex_1().overflow_hidden().child(match self.tab {
                    StudioTab::DualPane => self.render_dual_pane_view(cx).into_any_element(),
                    StudioTab::Convert => div()
                        .id("convert_pane_scroll")
                        .size_full()
                        .overflow_y_scroll()
                        .child(self.render_converter_pane(cx, false))
                        .into_any_element(),
                    StudioTab::LocalFolder => div()
                        .id("folder_pane_container")
                        .size_full()
                        .child(self.render_local_folder_full_tab(cx))
                        .into_any_element(),
                    StudioTab::Explorer => div()
                        .id("explorer_pane_container")
                        .size_full()
                        .child(self.render_kindle_pane(cx, false))
                        .into_any_element(),
                }),
            )
    }
}
