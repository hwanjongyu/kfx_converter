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

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::state::{
    ConversionSettings, ConversionTask, ConverterMode, DeviceStatus, EpubSortField, KindleFile,
    LocalFolderState, SortDirection, StudioTab, case_insensitive_cmp, compare_authors_unknown_last,
    contains_ascii_or_unicode_icase,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct ExplorerFilterCounts {
    pub all: usize,
    pub kfx: usize,
    pub word_wise: usize,
    pub azw3: usize,
    pub mobi: usize,
}

#[derive(Debug, Clone)]
pub(super) struct CachedIndexView {
    pub epoch: u64,
    pub search_query: String,
    pub filter: Option<String>,
    pub sort_field: EpubSortField,
    pub sort_direction: SortDirection,
    pub counts: ExplorerFilterCounts,
    pub indices: Arc<Vec<usize>>,
}

impl CachedIndexView {
    pub(super) fn matches(
        &self,
        epoch: u64,
        search_query: &str,
        filter: Option<&str>,
        sort_field: EpubSortField,
        sort_direction: SortDirection,
    ) -> bool {
        self.epoch == epoch
            && self.search_query == search_query
            && self.filter.as_deref() == filter
            && self.sort_field == sort_field
            && self.sort_direction == sort_direction
    }
}

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
    pub(super) device_refresh_in_flight: bool,
    pub(super) last_device_refresh_at: Option<Instant>,
    pub(super) eject_in_progress: bool,
    pub(super) delete_in_progress: bool,
    pub(super) last_output_path: Option<PathBuf>,
    pub(super) folder_scroll_handle: UniformListScrollHandle,
    pub(super) explorer_scroll_handle: UniformListScrollHandle,
    pub(super) explorer_sort_field: EpubSortField,
    pub(super) explorer_sort_direction: SortDirection,
    pub(super) confirm_delete_file: Option<PathBuf>,
    pub(super) folder_data_epoch: u64,
    pub(super) files_data_epoch: u64,
    pub(super) folder_view_cache: RefCell<Option<CachedIndexView>>,
    pub(super) explorer_view_cache: RefCell<Option<CachedIndexView>>,
    pub(super) hovered_scrollbar: Option<&'static str>,
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
            device_refresh_in_flight: false,
            last_device_refresh_at: None,
            eject_in_progress: false,
            delete_in_progress: false,
            last_output_path: None,
            folder_scroll_handle: UniformListScrollHandle::new(),
            explorer_scroll_handle: UniformListScrollHandle::new(),
            explorer_sort_field: EpubSortField::Title,
            explorer_sort_direction: SortDirection::Ascending,
            confirm_delete_file: None,
            folder_data_epoch: 0,
            files_data_epoch: 0,
            folder_view_cache: RefCell::new(None),
            explorer_view_cache: RefCell::new(None),
            hovered_scrollbar: None,
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

    pub(super) fn mark_folder_items_dirty(&mut self) {
        self.folder_data_epoch = self.folder_data_epoch.wrapping_add(1);
    }

    pub(super) fn mark_files_dirty(&mut self) {
        self.files_data_epoch = self.files_data_epoch.wrapping_add(1);
    }

    pub(super) fn cached_folder_indices(&self) -> Arc<Vec<usize>> {
        {
            let cache = self.folder_view_cache.borrow();
            if let Some(cached) = cache.as_ref()
                && cached.matches(
                    self.folder_data_epoch,
                    &self.folder_state.search_query,
                    self.folder_state.filter_status.as_deref(),
                    self.folder_state.sort_field,
                    self.folder_state.sort_direction,
                )
            {
                return Arc::clone(&cached.indices);
            }
        }

        let indices = Arc::new(self.folder_state.sorted_and_filtered_indices());
        *self.folder_view_cache.borrow_mut() = Some(CachedIndexView {
            epoch: self.folder_data_epoch,
            search_query: self.folder_state.search_query.clone(),
            filter: self.folder_state.filter_status.clone(),
            sort_field: self.folder_state.sort_field,
            sort_direction: self.folder_state.sort_direction,
            counts: ExplorerFilterCounts::default(),
            indices: Arc::clone(&indices),
        });
        indices
    }

    pub(super) fn cached_explorer_view(&self) -> (ExplorerFilterCounts, Arc<Vec<usize>>) {
        {
            let cache = self.explorer_view_cache.borrow();
            if let Some(cached) = cache.as_ref()
                && cached.matches(
                    self.files_data_epoch,
                    &self.search_query,
                    self.filter_format.as_deref(),
                    self.explorer_sort_field,
                    self.explorer_sort_direction,
                )
            {
                return (cached.counts, Arc::clone(&cached.indices));
            }
        }

        let search_lower = if self.search_query.is_empty() {
            String::new()
        } else {
            self.search_query.to_lowercase()
        };
        let filter_fmt = self.filter_format.as_deref();

        let mut counts = ExplorerFilterCounts {
            all: self.files.len(),
            ..Default::default()
        };
        let mut filtered_indices = Vec::new();

        for (idx, f) in self.files.iter().enumerate() {
            match f.format.as_str() {
                "KFX" => counts.kfx += 1,
                "AZW3" => counts.azw3 += 1,
                "MOBI" => counts.mobi += 1,
                _ => {}
            }
            if f.has_word_wise {
                counts.word_wise += 1;
            }

            let fmt_match = match filter_fmt {
                Some("KFX") => f.format == "KFX",
                Some("WORDWISE") => f.has_word_wise,
                Some("AZW3") => f.format == "AZW3",
                Some("MOBI") => f.format == "MOBI",
                _ => true,
            };
            if !fmt_match {
                continue;
            }

            let query_match = if search_lower.is_empty() {
                true
            } else {
                contains_ascii_or_unicode_icase(&f.name, &search_lower)
                    || contains_ascii_or_unicode_icase(&f.title, &search_lower)
                    || contains_ascii_or_unicode_icase(&f.author, &search_lower)
            };
            if query_match {
                filtered_indices.push(idx);
            }
        }

        filtered_indices.sort_by(|&a_idx, &b_idx| {
            let a = &self.files[a_idx];
            let b = &self.files[b_idx];

            let cmp = match self.explorer_sort_field {
                EpubSortField::Title => case_insensitive_cmp(&a.title, &b.title),
                EpubSortField::Author => compare_authors_unknown_last(&a.author, &b.author),
                EpubSortField::FileSize => a.size_bytes.cmp(&b.size_bytes),
                EpubSortField::DateModified => a.modified_timestamp.cmp(&b.modified_timestamp),
                EpubSortField::Status => a.format.cmp(&b.format),
            };

            let ordered_cmp = match self.explorer_sort_direction {
                SortDirection::Ascending => cmp,
                SortDirection::Descending => cmp.reverse(),
            };

            ordered_cmp.then_with(|| a.name.cmp(&b.name))
        });

        let indices = Arc::new(filtered_indices);
        *self.explorer_view_cache.borrow_mut() = Some(CachedIndexView {
            epoch: self.files_data_epoch,
            search_query: self.search_query.clone(),
            filter: self.filter_format.clone(),
            sort_field: self.explorer_sort_field,
            sort_direction: self.explorer_sort_direction,
            counts,
            indices: Arc::clone(&indices),
        });
        (counts, indices)
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
