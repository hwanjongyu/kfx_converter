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

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::state::{
    ConversionSettings, ConversionTask, ConverterMode, DeviceStatus, EpubSortField, KindleFile,
    LocalFolderState, SortDirection, StudioTab,
};
use crate::theme::Theme;
use gpui::*;

#[derive(Debug, Clone)]
pub(super) struct DropJob {
    pub task_id: u64,
    pub path: PathBuf,
    pub settings: ConversionSettings,
}

pub(super) type BatchQueue = Arc<Mutex<VecDeque<(u64, PathBuf)>>>;

pub struct KindleStudio {
    pub(super) tab: StudioTab,
    pub(super) converter_mode: ConverterMode,
    pub(super) folder_state: LocalFolderState,
    pub(super) device: DeviceStatus,
    pub(super) settings: ConversionSettings,
    pub(super) tasks: Vec<ConversionTask>,
    pub(super) next_task_id: u64,
    pub(super) files: Vec<KindleFile>,
    pub(super) filter_format: Option<String>,
    pub(super) search_query: String,
    pub(super) status_message: Option<(String, bool)>, // (text, is_error)
    pub(super) batch_cancel_token: Option<Arc<AtomicBool>>,
    pub(super) batch_generation: u64,
    pub(super) batch_active_workers: usize,
    pub(super) batch_queue: Option<BatchQueue>,
    pub(super) drop_queue: Arc<Mutex<VecDeque<DropJob>>>,
    pub(super) drop_active_workers: usize,
    pub(super) scan_cancel_token: Option<Arc<AtomicBool>>,
    pub(super) device_refresh_generation: u64,
    pub(super) eject_in_progress: bool,
    pub(super) delete_in_progress: bool,
    pub(super) last_output_path: Option<PathBuf>,
    pub(super) folder_scroll_handle: UniformListScrollHandle,
    pub(super) explorer_scroll_handle: UniformListScrollHandle,
    pub(super) explorer_sort_field: EpubSortField,
    pub(super) explorer_sort_direction: SortDirection,
    pub(super) confirm_delete_file: Option<PathBuf>,
}

impl KindleStudio {
    fn new_unconnected() -> Self {
        Self {
            tab: StudioTab::DualPane,
            converter_mode: ConverterMode::DropQueue,
            folder_state: LocalFolderState::default(),
            device: DeviceStatus::default(),
            settings: ConversionSettings::default(),
            tasks: Vec::new(),
            next_task_id: 1,
            files: Vec::new(),
            filter_format: None,
            search_query: String::new(),
            status_message: None,
            batch_cancel_token: None,
            batch_generation: 0,
            batch_active_workers: 0,
            batch_queue: None,
            drop_queue: Arc::new(Mutex::new(VecDeque::new())),
            drop_active_workers: 0,
            scan_cancel_token: None,
            device_refresh_generation: 0,
            eject_in_progress: false,
            delete_in_progress: false,
            last_output_path: None,
            folder_scroll_handle: UniformListScrollHandle::new(),
            explorer_scroll_handle: UniformListScrollHandle::new(),
            explorer_sort_field: EpubSortField::Title,
            explorer_sort_direction: SortDirection::Ascending,
            confirm_delete_file: None,
        }
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut studio = Self::new_unconnected();
        studio.refresh_device_internal(false, cx);
        studio
    }

    #[cfg(test)]
    pub(super) fn new_for_test() -> Self {
        Self::new_unconnected()
    }

    pub(super) fn allocate_task_id(&mut self) -> u64 {
        let id = self.next_task_id;
        self.next_task_id = self.next_task_id.saturating_add(1);
        id
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
