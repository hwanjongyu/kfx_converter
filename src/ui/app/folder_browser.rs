use std::path::Path;
use std::sync::Arc;

use super::KindleStudio;
use crate::state::{
    BatchProgressState, EpubSortField, LocalEpubItem, LocalEpubStatus, ScanPhase, ScanProgressState,
};
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;
use gpui::*;

/// Formats the active folder path or the empty placeholder prompt for the header card.
fn format_folder_path_display(current_folder: Option<&Path>) -> String {
    current_folder
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "No folder selected (Click Choose Folder)".to_string())
}

/// Returns the empty-state message for the EPUB table when no items are displayed.
fn empty_folder_message(has_folder: bool) -> &'static str {
    if has_folder {
        "No EPUB books found in this folder."
    } else {
        "Click 'Choose Folder' above to select your EPUB directory."
    }
}

/// Derives `(icon, detail, badge, fraction)` for the folder scan progress card and banner.
fn scan_progress_summary(sp: &ScanProgressState) -> (&'static str, String, String, f32) {
    match &sp.phase {
        ScanPhase::Discovering { found_files } => (
            "🔍",
            "Scanning folders for EPUB files...".to_string(),
            format!("{} found", found_files),
            0.12,
        ),
        ScanPhase::ReadingMetadata {
            processed,
            total,
            current_file,
        } => {
            let pct = sp.percentage();
            let frac = sp.fraction().clamp(0.04, 1.0);
            let detail = if current_file.is_empty() {
                format!("Loading metadata: {} / {}", processed, total)
            } else {
                format!(
                    "Loading metadata: {} / {} — {}",
                    processed, total, current_file
                )
            };
            ("📚", detail, format!("{}%", pct), frac)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatchVisualTone {
    Cancelled,
    CompletedWithErrors,
    CompletedSuccess,
    Syncing,
    Converting,
}

#[derive(Debug, Clone, PartialEq)]
struct BatchProgressViewModel {
    is_done: bool,
    #[allow(dead_code)]
    pct: u8,
    clamped_fraction: f32,
    tone: BatchVisualTone,
    icon: &'static str,
    title: String,
    subtitle: String,
    failed_badge: Option<String>,
    dismiss_label: Option<&'static str>,
}

/// Builds the GPUI-independent presentation model for the bottom batch progress bar.
fn build_batch_progress_view_model(bp: &BatchProgressState) -> BatchProgressViewModel {
    let pct = bp.overall_percentage();
    let clamped_fraction = bp.overall_progress().clamp(0.02, 1.0);
    let is_cancelled = bp.is_cancelled;
    let is_done = (bp.completed_tasks.saturating_add(bp.failed_tasks) >= bp.total_tasks
        || is_cancelled)
        && bp.active_tasks == 0;

    let tone = if is_done && is_cancelled {
        BatchVisualTone::Cancelled
    } else if is_done && bp.failed_tasks > 0 {
        BatchVisualTone::CompletedWithErrors
    } else if is_done {
        BatchVisualTone::CompletedSuccess
    } else if bp.is_sync {
        BatchVisualTone::Syncing
    } else {
        BatchVisualTone::Converting
    };

    let icon = match tone {
        BatchVisualTone::Cancelled => "⏹",
        BatchVisualTone::CompletedWithErrors => "⚠",
        BatchVisualTone::CompletedSuccess => "✓",
        BatchVisualTone::Syncing => "📲",
        BatchVisualTone::Converting => "⚡",
    };

    let title = if is_done && is_cancelled {
        format!(
            "Batch cancelled ({} completed, {} cancelled/failed)",
            bp.completed_tasks, bp.failed_tasks
        )
    } else if is_cancelled {
        format!("Cancelling batch... ({} active remaining)", bp.active_tasks)
    } else if is_done && bp.failed_tasks > 0 {
        format!(
            "Finished processing {} of {} books",
            bp.completed_tasks, bp.total_tasks
        )
    } else if is_done {
        format!(
            "Finished processing {} books successfully!",
            bp.completed_tasks
        )
    } else if bp.is_sync {
        format!(
            "Transferring to Kindle: {} of {} done ({}%)",
            bp.completed_tasks, bp.total_tasks, pct
        )
    } else {
        format!(
            "Converting Books: {} of {} done ({}%)",
            bp.completed_tasks, bp.total_tasks, pct
        )
    };

    let subtitle = if is_done && is_cancelled {
        "Batch conversion cancelled. Click Dismiss to close.".to_string()
    } else if is_done {
        "All tasks finished. Click Dismiss to close.".to_string()
    } else if !bp.current_filename.is_empty() {
        format!("Active: {} ({})", bp.current_filename, bp.current_stage)
    } else {
        bp.current_stage.clone()
    };

    let failed_badge = if bp.failed_tasks > 0 && !is_cancelled {
        Some(format!("({} failed)", bp.failed_tasks))
    } else {
        None
    };

    let dismiss_label = if is_done {
        Some(if is_cancelled {
            "Dismiss"
        } else {
            "✓ Dismiss"
        })
    } else {
        None
    };

    BatchProgressViewModel {
        is_done,
        pct,
        clamped_fraction,
        tone,
        icon,
        title,
        subtitle,
        failed_badge,
        dismiss_label,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BatchToolbarCounts {
    completed: usize,
    active: usize,
    waiting: usize,
}

/// Computes toolbar counters `(completed, active, waiting)` from a `BatchProgressState`.
fn batch_toolbar_counts(bp: &BatchProgressState) -> BatchToolbarCounts {
    let accounted = bp
        .completed_tasks
        .saturating_add(bp.failed_tasks)
        .saturating_add(bp.active_tasks);
    BatchToolbarCounts {
        completed: bp.completed_tasks,
        active: bp.active_tasks,
        waiting: bp.total_tasks.saturating_sub(accounted),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusBadgeTone {
    Neutral,
    Queued,
    Processing,
    LocalReady,
    OnKindle,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
struct EpubStatusPresentation {
    tone: StatusBadgeTone,
    label: String,
    is_processing: bool,
    progress_fraction: Option<f32>,
    show_quick_convert: bool,
}

/// Derives badge styling, progress-bar fraction, and quick-convert visibility for an EPUB row.
fn derive_epub_status_presentation(status: &LocalEpubStatus) -> EpubStatusPresentation {
    match status {
        LocalEpubStatus::Unconverted => EpubStatusPresentation {
            tone: StatusBadgeTone::Neutral,
            label: "⚪ Unconverted".to_string(),
            is_processing: false,
            progress_fraction: None,
            show_quick_convert: true,
        },
        LocalEpubStatus::InQueue => EpubStatusPresentation {
            tone: StatusBadgeTone::Queued,
            label: "⏳ In Queue".to_string(),
            is_processing: false,
            progress_fraction: None,
            show_quick_convert: false,
        },
        LocalEpubStatus::Processing { stage, progress } => EpubStatusPresentation {
            tone: StatusBadgeTone::Processing,
            label: format!("🟡 {} ({}%)", stage, progress),
            is_processing: true,
            progress_fraction: Some((*progress as f32 / 100.0).clamp(0.08, 1.0)),
            show_quick_convert: false,
        },
        LocalEpubStatus::ConvertedLocal => EpubStatusPresentation {
            tone: StatusBadgeTone::LocalReady,
            label: "🔵 Local KFX".to_string(),
            is_processing: false,
            progress_fraction: None,
            show_quick_convert: false,
        },
        LocalEpubStatus::OnDevice => EpubStatusPresentation {
            tone: StatusBadgeTone::OnKindle,
            label: "🟢 On Kindle".to_string(),
            is_processing: false,
            progress_fraction: None,
            show_quick_convert: false,
        },
        LocalEpubStatus::Error(err) => EpubStatusPresentation {
            tone: StatusBadgeTone::Error,
            label: format!("Error: {}", err),
            is_processing: false,
            progress_fraction: None,
            show_quick_convert: false,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FolderToolbarState {
    selection_summary: String,
    rescan_label: &'static str,
    actions_disabled: bool,
    can_batch_convert: bool,
    can_batch_sync: bool,
    convert_button_label: String,
    sync_button_label: String,
}

/// Computes toolbar summary text, rescan label, and batch action enablement flags.
fn compute_folder_toolbar_state(
    selected_count: usize,
    total_count: usize,
    is_scanning: bool,
    is_batch_active: bool,
    eject_in_progress: bool,
    device_connected: bool,
) -> FolderToolbarState {
    let actions_disabled = is_batch_active || eject_in_progress;
    let can_batch_convert = selected_count > 0 && !actions_disabled;
    let can_batch_sync = can_batch_convert && device_connected;
    FolderToolbarState {
        selection_summary: format!("{}/{} books selected", selected_count, total_count),
        rescan_label: if is_scanning {
            "⏳ Scanning..."
        } else {
            "🔄 Refresh"
        },
        actions_disabled,
        can_batch_convert,
        can_batch_sync,
        convert_button_label: format!("⚡ Convert Selected ({})", selected_count),
        sync_button_label: format!("📲 Transfer to Kindle ({})", selected_count),
    }
}

impl KindleStudio {
    /// Renders the Local Folder EPUB Browser View (Path selector, Filter/Selection bar, Table, Batch actions).
    pub(super) fn render_folder_browser_view(
        &self,
        cx: &mut Context<Self>,
        in_split: bool,
    ) -> impl IntoElement {
        let _ = in_split;
        let selected_count = self
            .folder_state
            .items
            .iter()
            .filter(|i| i.is_selected)
            .count();
        let total_count = self.folder_state.items.len();
        let toolbar = compute_folder_toolbar_state(
            selected_count,
            total_count,
            self.folder_state.is_scanning,
            self.is_batch_active(),
            self.eject_in_progress,
            self.device.connected,
        );

        let filtered_indices = self.cached_folder_indices();

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
                                    .child(format_folder_path_display(
                                        self.folder_state.current_folder.as_deref(),
                                    )),
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
                                    .child(toolbar.rescan_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.rescan_folder(cx);
                                    })),
                            ),
                    )
            )
            .when(
                self.folder_state.is_scanning && !self.folder_state.items.is_empty(),
                |this| {
                    let fallback = ScanProgressState::discovering();
                    let sp = self.folder_state.scan_progress.as_ref().unwrap_or(&fallback);
                    this.child(self.render_scan_progress_banner(sp, cx))
                },
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
                            )
                            .child(
                                div()
                                    .w(px(1.0))
                                    .h(px(16.0))
                                    .bg(Theme::border())
                            )
                            .child(
                                div()
                                    .id("folder_toggle_content_type")
                                    .px_2p5()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(Theme::bg_card_hover())
                                    .border_1()
                                    .border_color(Theme::border())
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::text_primary())
                                    .child(format!("Format: {}", self.settings.content_type))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.content_type = if this.settings.content_type == "PDOC" {
                                            "EBOK".into()
                                        } else {
                                            "PDOC".into()
                                        };
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("folder_toggle_word_wise")
                                    .px_2p5()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(if self.settings.word_wise { Theme::accent_purple() } else { Theme::bg_card_hover() })
                                    .border_1()
                                    .border_color(if self.settings.word_wise { Theme::accent_purple() } else { Theme::border() })
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if self.settings.word_wise { Theme::text_primary() } else { Theme::text_muted() })
                                    .child(if self.settings.word_wise { "🟣 Word Wise ON" } else { "⚪ Word Wise OFF" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.word_wise = !this.settings.word_wise;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        if let Some(ref bp) = self.folder_state.batch_progress {
                            let counts = batch_toolbar_counts(bp);
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_md()
                                        .bg(Theme::bg_card_hover())
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(Theme::accent_green())
                                        .child(format!("✓ {} done", counts.completed)),
                                )
                                .child(
                                    div()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_md()
                                        .bg(Theme::bg_card_hover())
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(Theme::accent_amber())
                                        .child(format!("⚡ {} active", counts.active)),
                                )
                                .child(
                                    div()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_md()
                                        .bg(Theme::bg_card_hover())
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(Theme::text_muted())
                                        .child(format!("⏳ {} waiting", counts.waiting)),
                                )
                                .into_any_element()
                        } else {
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(if selected_count > 0 { Theme::bg_card_hover() } else { hsla(0.0, 0.0, 0.0, 0.0) })
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(if selected_count > 0 { Theme::text_primary() } else { Theme::text_muted() })
                                        .child(toolbar.selection_summary),
                                )
                                .into_any_element()
                        }
                    ),
            )
            // 3. Scrollable EPUB List Table with Column Headers (Option B)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .overflow_hidden()
                    .rounded_xl()
                    .bg(Theme::bg_card())
                    .border_1()
                    .border_color(Theme::border())
                    .child(self.render_table_column_headers(cx))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .w_full()
                            .min_h_0()
                            .overflow_hidden()
                            .p_2()
                            .gap_1p5()
                            .child(
                                div()
                                    .id("folder_epub_list_scroll")
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_hidden()
                                    .child(
                                        if self.folder_state.is_scanning && self.folder_state.items.is_empty() {
                                            let fallback = ScanProgressState::discovering();
                                            let sp = self.folder_state.scan_progress.as_ref().unwrap_or(&fallback);
                                            self.render_scan_progress_card(sp, cx)
                                        } else if filtered_indices.is_empty() {
                                            div()
                                                .size_full()
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
                                                        .child(empty_folder_message(
                                                            self.folder_state.current_folder.is_some(),
                                                        )),
                                                )
                                                .into_any_element()
                                        } else {
                                            let view = cx.entity().downgrade();
                                            let indices = Arc::clone(&filtered_indices);
                                            let actions_disabled = toolbar.actions_disabled;
                                            uniform_list(
                                                "folder_epub_uniform_list",
                                                indices.len(),
                                                cx.processor(move |this, range: std::ops::Range<usize>, _window, _cx| {
                                                    range
                                                        .filter_map(|r_idx| {
                                                            let &item_idx = indices.get(r_idx)?;
                                                            let item = this
                                                                .folder_state
                                                                .items
                                                                .get(item_idx)?;
                                                            Some(Self::render_epub_card(
                                                                item,
                                                                &view,
                                                                actions_disabled,
                                                            ))
                                                        })
                                                        .collect()
                                                }),
                                            )
                                            .track_scroll(self.folder_scroll_handle.clone())
                                            .flex_1()
                                            .min_w_0()
                                            .h_full()
                                            .into_any_element()
                                        }
                                    )
                            )
                            .child(
                                self.render_custom_scrollbar(&self.folder_scroll_handle, filtered_indices.len(), px(68.0), "folder", cx)
                            )
                    )
            )
            // 4. Bottom Batch Actions Bar
            .child(
                if let Some(ref bp) = self.folder_state.batch_progress {
                    self.render_batch_progress_bar(bp, cx)
                } else {
                    let can_batch_convert = toolbar.can_batch_convert;
                    let can_batch_sync = toolbar.can_batch_sync;

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
                                .bg(if can_batch_convert { Theme::accent_amber() } else { Theme::bg_card_hover() })
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(if can_batch_convert { Theme::bg_root() } else { Theme::text_muted() })
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(toolbar.convert_button_label)
                                .when(can_batch_convert, |el| {
                                    el.cursor_pointer().on_click(cx.listener(|this, _, _, cx| {
                                        this.convert_selected_epubs(false, cx);
                                    }))
                                }),
                        )
                        .child(
                            div()
                                .id("btn_batch_convert_sync")
                                .flex_1()
                                .py_2p5()
                                .rounded_xl()
                                .bg(if can_batch_sync { Theme::accent_green() } else { Theme::bg_card_hover() })
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(if can_batch_sync { Theme::bg_root() } else { Theme::text_muted() })
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(toolbar.sync_button_label)
                                .when(can_batch_sync, |el| {
                                    el.cursor_pointer().on_click(cx.listener(|this, _, _, cx| {
                                        this.convert_selected_epubs(true, cx);
                                    }))
                                }),
                        )
                        .into_any_element()
                }
            )
    }

    /// Renders the full-screen Local Folder Tab view.
    pub(super) fn render_local_folder_full_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .p_6()
            .child(self.render_folder_browser_view(cx, false))
    }

    /// Renders sortable column headers for the EPUB library table view (Option B).
    pub(super) fn render_table_column_headers(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(34.0))
            .px_3()
            .bg(Theme::bg_card_hover())
            .border_b_1()
            .border_color(Theme::border())
            .gap_2()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            // Checkbox spacing placeholder
            .child(
                div()
                    .w(px(28.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(Theme::text_muted())
                    .child("☐"),
            )
            // 1. Title Column Header
            .child(self.render_sort_column_cell(
                "Title",
                EpubSortField::Title,
                div().flex_1().min_w_0(),
                cx,
            ))
            // 2. Author Column Header
            .child(self.render_sort_column_cell(
                "Author",
                EpubSortField::Author,
                div().w(px(140.0)).flex_shrink_0().min_w_0(),
                cx,
            ))
            // 3. Size Column Header
            .child(self.render_sort_column_cell(
                "Size",
                EpubSortField::FileSize,
                div().w(px(70.0)).flex_shrink_0(),
                cx,
            ))
            // 4. Date Column Header
            .child(self.render_sort_column_cell(
                "Date",
                EpubSortField::DateModified,
                div().w(px(85.0)).flex_shrink_0(),
                cx,
            ))
            // 5. Status Column Header
            .child(self.render_sort_column_cell(
                "Status",
                EpubSortField::Status,
                div().w(px(160.0)).flex_shrink_0(),
                cx,
            ))
            // Scrollbar spacer (14px)
            .child(div().w(px(14.0)).flex_shrink_0())
    }

    /// Renders a single EPUB item card with strict 68px height for virtualization.
    pub(super) fn render_epub_card(
        item: &LocalEpubItem,
        view: &WeakEntity<Self>,
        actions_disabled: bool,
    ) -> AnyElement {
        let item_id = item.id;
        let item_path = item.path.clone();
        let is_sel = item.is_selected;
        let status_vm = derive_epub_status_presentation(&item.status);

        let status_badge = {
            let base = div().px_2().py_0p5().rounded_md().text_xs();
            match status_vm.tone {
                StatusBadgeTone::Neutral => base
                    .bg(Theme::bg_card_hover())
                    .text_color(Theme::text_muted()),
                StatusBadgeTone::Queued => base
                    .bg(Theme::bg_card_hover())
                    .border_1()
                    .border_color(Theme::border())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(Theme::text_muted()),
                StatusBadgeTone::Processing => base
                    .bg(Theme::accent_amber())
                    .font_weight(FontWeight::BOLD)
                    .text_color(Theme::bg_root()),
                StatusBadgeTone::LocalReady => base
                    .bg(Theme::accent_blue())
                    .font_weight(FontWeight::BOLD)
                    .text_color(Theme::text_primary()),
                StatusBadgeTone::OnKindle => base
                    .bg(Theme::accent_green())
                    .font_weight(FontWeight::BOLD)
                    .text_color(Theme::bg_root()),
                StatusBadgeTone::Error => base
                    .bg(Theme::accent_red())
                    .text_color(Theme::text_primary()),
            }
            .child(status_vm.label)
        };

        let quick_btn = if status_vm.show_quick_convert {
            let p = item_path.clone();
            let view_quick = view.clone();
            Some(
                div()
                    .id(("btn_quick_convert", item_id as usize))
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(if actions_disabled {
                        Theme::bg_card_hover()
                    } else {
                        Theme::accent_amber()
                    })
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(if actions_disabled {
                        Theme::text_muted()
                    } else {
                        Theme::bg_root()
                    })
                    .child("⚡ Convert")
                    .when(!actions_disabled, |el| {
                        el.cursor_pointer().on_click(move |_ev, _window, cx| {
                            cx.stop_propagation();
                            view_quick
                                .update(cx, |this, cx| {
                                    this.convert_single_epub(p.clone(), false, cx);
                                })
                                .ok();
                        })
                    }),
            )
        } else {
            None
        };

        let view_select = view.clone();

        div()
            .w_full()
            .h(px(68.0))
            .pb(px(6.0))
            .child(
                div()
                    .id(("epub_item", item_id as usize))
                    .relative()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .w_full()
                    .h(px(62.0))
                    .px_3()
                    .rounded_xl()
                    .bg(if is_sel {
                        Theme::bg_card_hover()
                    } else {
                        Theme::bg_card()
                    })
                    .border_1()
                    .border_color(if is_sel {
                        Theme::accent_amber()
                    } else {
                        Theme::border()
                    })
                    .cursor_pointer()
                    .gap_2()
                    .on_click(move |_ev, _window, cx| {
                        view_select
                            .update(cx, |this, cx| {
                                this.toggle_item_selection(item_id, cx);
                            })
                            .ok();
                    })
                    .child(
                        if status_vm.is_processing
                            && let Some(progress_fraction) = status_vm.progress_fraction
                        {
                            div()
                                .absolute()
                                .bottom_0()
                                .left_0()
                                .h(px(3.0))
                                .bg(Theme::accent_amber())
                                .w(DefiniteLength::Fraction(progress_fraction))
                                .into_any_element()
                        } else {
                            div().into_any_element()
                        },
                    )
                    // Checkbox
                    .child(
                        div()
                            .w(px(28.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .w_5()
                                    .h_5()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(if is_sel {
                                        Theme::accent_amber()
                                    } else {
                                        Theme::border()
                                    })
                                    .bg(if is_sel {
                                        Theme::accent_amber()
                                    } else {
                                        hsla(0.0, 0.0, 0.0, 0.0)
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::bg_root())
                                    .child(if is_sel { "✓" } else { "" }),
                            ),
                    )
                    // 1. Title Column
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .gap_0p5()
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
                                    .text_xs()
                                    .text_color(Theme::text_muted())
                                    .truncate()
                                    .child(item.filename.clone()),
                            ),
                    )
                    // 2. Author Column
                    .child(
                        div()
                            .w(px(140.0))
                            .flex_shrink_0()
                            .min_w_0()
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_primary())
                                    .truncate()
                                    .child(item.author.clone()),
                            ),
                    )
                    // 3. Size Column
                    .child(
                        div().w(px(70.0)).flex_shrink_0().child(
                            div()
                                .text_xs()
                                .text_color(Theme::text_muted())
                                .child(item.file_size_str.clone()),
                        ),
                    )
                    // 4. Date Column
                    .child(
                        div().w(px(85.0)).flex_shrink_0().child(
                            div()
                                .text_xs()
                                .text_color(Theme::text_muted())
                                .child(item.modified_str.clone()),
                        ),
                    )
                    // 5. Status & Actions Column
                    .child(
                        div()
                            .w(px(160.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(status_badge)
                            .children(quick_btn),
                    ),
            )
            .into_any_element()
    }

    fn scan_progress_summary(sp: &ScanProgressState) -> (&'static str, String, String, f32) {
        scan_progress_summary(sp)
    }

    fn render_scan_progress_card(
        &self,
        sp: &ScanProgressState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (icon, detail, badge, frac) = Self::scan_progress_summary(sp);

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p_8()
            .child(
                div()
                    .w(px(420.0))
                    .p_5()
                    .rounded_xl()
                    .bg(Theme::bg_card())
                    .border_1()
                    .border_color(Theme::border())
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().text_base().child(icon))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(Theme::text_primary())
                                            .child("Loading EPUB Folder"),
                                    ),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(Theme::bg_root())
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::accent_amber())
                                    .child(badge),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(8.0))
                            .rounded_full()
                            .bg(Theme::bg_root())
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .bg(Theme::accent_amber())
                                    .w(DefiniteLength::Fraction(frac)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_xs()
                                    .text_color(Theme::text_muted())
                                    .child(detail),
                            )
                            .child(
                                div()
                                    .id("btn_cancel_scan_card")
                                    .px_2p5()
                                    .py_1()
                                    .rounded_md()
                                    .bg(Theme::bg_card_hover())
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_primary())
                                    .cursor_pointer()
                                    .child("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_folder_scan(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_scan_progress_banner(
        &self,
        sp: &ScanProgressState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (icon, detail, badge, frac) = Self::scan_progress_summary(sp);

        div()
            .relative()
            .overflow_hidden()
            .w_full()
            .h(px(34.0))
            .rounded_lg()
            .bg(Theme::bg_card())
            .border_1()
            .border_color(Theme::border())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .h_full()
                    .bg(hsla(38.0 / 360.0, 0.92, 0.50, 0.20))
                    .w(DefiniteLength::Fraction(frac)),
            )
            .child(
                div()
                    .relative()
                    .size_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(div().text_xs().child(icon))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text_primary())
                                    .child(detail),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::accent_amber())
                                    .child(badge),
                            )
                            .child(
                                div()
                                    .id("btn_cancel_scan_banner")
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(Theme::bg_card_hover())
                                    .text_xs()
                                    .text_color(Theme::text_primary())
                                    .cursor_pointer()
                                    .child("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_folder_scan(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_batch_progress_bar(
        &self,
        bp: &BatchProgressState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let vm = build_batch_progress_view_model(bp);
        let border_color = match vm.tone {
            BatchVisualTone::Cancelled => Theme::accent_amber(),
            BatchVisualTone::CompletedWithErrors => Theme::accent_red(),
            BatchVisualTone::CompletedSuccess => Theme::accent_green(),
            BatchVisualTone::Syncing | BatchVisualTone::Converting => Theme::border(),
        };
        let fill_color = match vm.tone {
            BatchVisualTone::Cancelled => hsla(38.0 / 360.0, 0.92, 0.50, 0.25),
            BatchVisualTone::CompletedWithErrors => hsla(0.0, 0.84, 0.60, 0.22),
            BatchVisualTone::CompletedSuccess => hsla(142.0 / 360.0, 0.70, 0.45, 0.25),
            BatchVisualTone::Syncing => hsla(160.0 / 360.0, 0.84, 0.39, 0.25),
            BatchVisualTone::Converting => hsla(38.0 / 360.0, 0.92, 0.50, 0.25),
        };
        let dismiss_bg = match vm.tone {
            BatchVisualTone::Cancelled => Theme::accent_amber(),
            _ => Theme::accent_green(),
        };

        div()
            .w_full()
            .pt_2()
            .child(
                div()
                    .relative()
                    .overflow_hidden()
                    .w_full()
                    .h(px(52.0))
                    .rounded_xl()
                    .bg(Theme::bg_card())
                    .border_1()
                    .border_color(border_color)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .h_full()
                            .bg(fill_color)
                            .w(DefiniteLength::Fraction(vm.clamped_fraction)),
                    )
                    .child(
                        div()
                            .relative()
                            .size_full()
                            .px_4()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(div().text_base().child(vm.icon))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_0p5()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_weight(FontWeight::BOLD)
                                                            .text_color(Theme::text_primary())
                                                            .child(vm.title),
                                                    )
                                                    .child(if let Some(badge) = vm.failed_badge {
                                                        div()
                                                            .text_xs()
                                                            .text_color(Theme::accent_red())
                                                            .child(badge)
                                                            .into_any_element()
                                                    } else {
                                                        div().into_any_element()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(Theme::text_muted())
                                                    .child(vm.subtitle),
                                            ),
                                    ),
                            )
                            .child(if vm.is_done {
                                div()
                                    .id("btn_batch_dismiss")
                                    .cursor_pointer()
                                    .px_3()
                                    .py_1p5()
                                    .rounded_lg()
                                    .bg(dismiss_bg)
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::bg_root())
                                    .child(vm.dismiss_label.unwrap_or("Dismiss"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.folder_state.batch_progress = None;
                                        cx.notify();
                                    }))
                                    .into_any_element()
                            } else {
                                div()
                                    .id("btn_batch_cancel")
                                    .cursor_pointer()
                                    .px_3()
                                    .py_1p5()
                                    .rounded_lg()
                                    .bg(Theme::bg_card_hover())
                                    .border_1()
                                    .border_color(Theme::border())
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::accent_red())
                                    .child("⏹ Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_batch_conversion(cx);
                                    }))
                                    .into_any_element()
                            }),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BatchProgressState, BatchToolbarCounts, BatchVisualTone, LocalEpubStatus, ScanPhase,
        ScanProgressState, StatusBadgeTone, batch_toolbar_counts, build_batch_progress_view_model,
        compute_folder_toolbar_state, derive_epub_status_presentation, empty_folder_message,
        format_folder_path_display, scan_progress_summary,
    };
    use std::path::PathBuf;

    #[test]
    fn test_format_folder_path_display_and_empty_messages() {
        assert_eq!(
            format_folder_path_display(None),
            "No folder selected (Click Choose Folder)"
        );
        let sample_path = PathBuf::from("/tmp/ebooks/library");
        assert_eq!(
            format_folder_path_display(Some(&sample_path)),
            "/tmp/ebooks/library"
        );
        assert_eq!(
            empty_folder_message(false),
            "Click 'Choose Folder' above to select your EPUB directory."
        );
        assert_eq!(
            empty_folder_message(true),
            "No EPUB books found in this folder."
        );
    }

    #[test]
    fn test_scan_progress_summary_discovering_and_reading_metadata() {
        let discovering = ScanProgressState {
            phase: ScanPhase::Discovering { found_files: 17 },
        };
        let (icon, detail, badge, frac) = scan_progress_summary(&discovering);
        assert_eq!(icon, "🔍");
        assert_eq!(detail, "Scanning folders for EPUB files...");
        assert_eq!(badge, "17 found");
        assert!((frac - 0.12).abs() < 1e-6);

        let reading_empty_file = ScanProgressState {
            phase: ScanPhase::ReadingMetadata {
                processed: 0,
                total: 100,
                current_file: String::new(),
            },
        };
        let (icon, detail, badge, frac) = scan_progress_summary(&reading_empty_file);
        assert_eq!(icon, "📚");
        assert_eq!(detail, "Loading metadata: 0 / 100");
        assert_eq!(badge, "0%");
        assert!((frac - 0.04).abs() < 1e-6);

        let reading_with_file = ScanProgressState {
            phase: ScanPhase::ReadingMetadata {
                processed: 3,
                total: 4,
                current_file: "novel.epub".to_string(),
            },
        };
        let (icon, detail, badge, frac) = scan_progress_summary(&reading_with_file);
        assert_eq!(icon, "📚");
        assert_eq!(detail, "Loading metadata: 3 / 4 — novel.epub");
        assert_eq!(badge, "75%");
        assert!((frac - 0.75).abs() < 1e-6);

        let reading_over = ScanProgressState {
            phase: ScanPhase::ReadingMetadata {
                processed: 10,
                total: 10,
                current_file: "done.epub".to_string(),
            },
        };
        let (_, _, badge_done, frac_done) = scan_progress_summary(&reading_over);
        assert_eq!(badge_done, "100%");
        assert!((frac_done - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_batch_progress_view_model_states_and_toolbar_counts() {
        // 1. Active converting (0% clamps fraction to 0.02)
        let active_convert = BatchProgressState {
            total_tasks: 5,
            completed_tasks: 0,
            failed_tasks: 0,
            active_tasks: 2,
            current_filename: "book1.epub".to_string(),
            current_stage: "Packaging".to_string(),
            is_sync: false,
            is_cancelled: false,
        };
        let vm = build_batch_progress_view_model(&active_convert);
        assert!(!vm.is_done);
        assert_eq!(vm.pct, 0);
        assert!((vm.clamped_fraction - 0.02).abs() < 1e-6);
        assert_eq!(vm.tone, BatchVisualTone::Converting);
        assert_eq!(vm.icon, "⚡");
        assert_eq!(vm.title, "Converting Books: 0 of 5 done (0%)");
        assert_eq!(vm.subtitle, "Active: book1.epub (Packaging)");
        assert_eq!(vm.failed_badge, None);
        assert_eq!(vm.dismiss_label, None);
        assert_eq!(
            batch_toolbar_counts(&active_convert),
            BatchToolbarCounts {
                completed: 0,
                active: 2,
                waiting: 3,
            }
        );

        // 2. Active syncing with empty filename and 1 failure
        let active_sync = BatchProgressState {
            total_tasks: 4,
            completed_tasks: 1,
            failed_tasks: 1,
            active_tasks: 1,
            current_filename: String::new(),
            current_stage: "Transferring".to_string(),
            is_sync: true,
            is_cancelled: false,
        };
        let vm = build_batch_progress_view_model(&active_sync);
        assert!(!vm.is_done);
        assert_eq!(vm.pct, 50);
        assert!((vm.clamped_fraction - 0.5).abs() < 1e-6);
        assert_eq!(vm.tone, BatchVisualTone::Syncing);
        assert_eq!(vm.icon, "📲");
        assert_eq!(vm.title, "Transferring to Kindle: 1 of 4 done (50%)");
        assert_eq!(vm.subtitle, "Transferring");
        assert_eq!(vm.failed_badge.as_deref(), Some("(1 failed)"));
        assert_eq!(vm.dismiss_label, None);

        // 3. Cancelling with active > 0
        let cancelling = BatchProgressState {
            total_tasks: 4,
            completed_tasks: 1,
            failed_tasks: 1,
            active_tasks: 2,
            current_filename: "a.epub".to_string(),
            current_stage: "Stopping".to_string(),
            is_sync: false,
            is_cancelled: true,
        };
        let vm = build_batch_progress_view_model(&cancelling);
        assert!(!vm.is_done);
        assert_eq!(vm.tone, BatchVisualTone::Converting);
        assert_eq!(vm.title, "Cancelling batch... (2 active remaining)");
        assert_eq!(vm.failed_badge, None);
        assert_eq!(vm.dismiss_label, None);

        // 4. Cancelled done
        let cancelled_done = BatchProgressState {
            total_tasks: 4,
            completed_tasks: 2,
            failed_tasks: 2,
            active_tasks: 0,
            current_filename: String::new(),
            current_stage: String::new(),
            is_sync: true,
            is_cancelled: true,
        };
        let vm = build_batch_progress_view_model(&cancelled_done);
        assert!(vm.is_done);
        assert_eq!(vm.tone, BatchVisualTone::Cancelled);
        assert_eq!(vm.icon, "⏹");
        assert_eq!(
            vm.title,
            "Batch cancelled (2 completed, 2 cancelled/failed)"
        );
        assert_eq!(
            vm.subtitle,
            "Batch conversion cancelled. Click Dismiss to close."
        );
        assert_eq!(vm.failed_badge, None);
        assert_eq!(vm.dismiss_label, Some("Dismiss"));

        // 5. Completed with failures
        let completed_errors = BatchProgressState {
            total_tasks: 3,
            completed_tasks: 2,
            failed_tasks: 1,
            active_tasks: 0,
            current_filename: String::new(),
            current_stage: String::new(),
            is_sync: false,
            is_cancelled: false,
        };
        let vm = build_batch_progress_view_model(&completed_errors);
        assert!(vm.is_done);
        assert_eq!(vm.pct, 100);
        assert!((vm.clamped_fraction - 1.0).abs() < 1e-6);
        assert_eq!(vm.tone, BatchVisualTone::CompletedWithErrors);
        assert_eq!(vm.icon, "⚠");
        assert_eq!(vm.title, "Finished processing 2 of 3 books");
        assert_eq!(vm.subtitle, "All tasks finished. Click Dismiss to close.");
        assert_eq!(vm.failed_badge.as_deref(), Some("(1 failed)"));
        assert_eq!(vm.dismiss_label, Some("✓ Dismiss"));

        // 6. Completed clean
        let completed_clean = BatchProgressState {
            total_tasks: 3,
            completed_tasks: 3,
            failed_tasks: 0,
            active_tasks: 0,
            current_filename: String::new(),
            current_stage: String::new(),
            is_sync: false,
            is_cancelled: false,
        };
        let vm = build_batch_progress_view_model(&completed_clean);
        assert!(vm.is_done);
        assert_eq!(vm.tone, BatchVisualTone::CompletedSuccess);
        assert_eq!(vm.icon, "✓");
        assert_eq!(vm.title, "Finished processing 3 books successfully!");
        assert_eq!(vm.failed_badge, None);
        assert_eq!(vm.dismiss_label, Some("✓ Dismiss"));

        // 7. Over-reported counts saturate waiting to 0
        let over_reported = BatchProgressState {
            total_tasks: 2,
            completed_tasks: 2,
            failed_tasks: 1,
            active_tasks: 1,
            ..Default::default()
        };
        assert_eq!(
            batch_toolbar_counts(&over_reported),
            BatchToolbarCounts {
                completed: 2,
                active: 1,
                waiting: 0,
            }
        );
    }

    #[test]
    fn test_derive_epub_status_presentation_all_variants() {
        let unconverted = derive_epub_status_presentation(&LocalEpubStatus::Unconverted);
        assert_eq!(unconverted.tone, StatusBadgeTone::Neutral);
        assert_eq!(unconverted.label, "⚪ Unconverted");
        assert!(!unconverted.is_processing);
        assert_eq!(unconverted.progress_fraction, None);
        assert!(unconverted.show_quick_convert);

        let queued = derive_epub_status_presentation(&LocalEpubStatus::InQueue);
        assert_eq!(queued.tone, StatusBadgeTone::Queued);
        assert_eq!(queued.label, "⏳ In Queue");
        assert!(!queued.is_processing);
        assert_eq!(queued.progress_fraction, None);
        assert!(!queued.show_quick_convert);

        let proc_zero = derive_epub_status_presentation(&LocalEpubStatus::Processing {
            stage: "Parsing".to_string(),
            progress: 0,
        });
        assert_eq!(proc_zero.tone, StatusBadgeTone::Processing);
        assert_eq!(proc_zero.label, "🟡 Parsing (0%)");
        assert!(proc_zero.is_processing);
        assert!((proc_zero.progress_fraction.unwrap() - 0.08).abs() < 1e-6);
        assert!(!proc_zero.show_quick_convert);

        let proc_full = derive_epub_status_presentation(&LocalEpubStatus::Processing {
            stage: "Syncing".to_string(),
            progress: 120,
        });
        assert_eq!(proc_full.tone, StatusBadgeTone::Processing);
        assert!((proc_full.progress_fraction.unwrap() - 1.0).abs() < 1e-6);
        assert!(!proc_full.show_quick_convert);

        let local = derive_epub_status_presentation(&LocalEpubStatus::ConvertedLocal);
        assert_eq!(local.tone, StatusBadgeTone::LocalReady);
        assert_eq!(local.label, "🔵 Local KFX");
        assert!(!local.is_processing);
        assert_eq!(local.progress_fraction, None);
        assert!(!local.show_quick_convert);

        let on_dev = derive_epub_status_presentation(&LocalEpubStatus::OnDevice);
        assert_eq!(on_dev.tone, StatusBadgeTone::OnKindle);
        assert_eq!(on_dev.label, "🟢 On Kindle");
        assert!(!on_dev.is_processing);
        assert_eq!(on_dev.progress_fraction, None);
        assert!(!on_dev.show_quick_convert);

        let err = derive_epub_status_presentation(&LocalEpubStatus::Error("IO".to_string()));
        assert_eq!(err.tone, StatusBadgeTone::Error);
        assert_eq!(err.label, "Error: IO");
        assert!(!err.is_processing);
        assert_eq!(err.progress_fraction, None);
        assert!(!err.show_quick_convert);
    }

    #[test]
    fn test_compute_folder_toolbar_state_gating() {
        let none_selected = compute_folder_toolbar_state(0, 10, false, false, false, true);
        assert_eq!(none_selected.selection_summary, "0/10 books selected");
        assert_eq!(none_selected.rescan_label, "🔄 Refresh");
        assert!(!none_selected.actions_disabled);
        assert!(!none_selected.can_batch_convert);
        assert!(!none_selected.can_batch_sync);
        assert_eq!(
            none_selected.convert_button_label,
            "⚡ Convert Selected (0)"
        );
        assert_eq!(none_selected.sync_button_label, "📲 Transfer to Kindle (0)");

        let selected_disconnected = compute_folder_toolbar_state(3, 10, true, false, false, false);
        assert_eq!(selected_disconnected.rescan_label, "⏳ Scanning...");
        assert!(!selected_disconnected.actions_disabled);
        assert!(selected_disconnected.can_batch_convert);
        assert!(!selected_disconnected.can_batch_sync);
        assert_eq!(
            selected_disconnected.convert_button_label,
            "⚡ Convert Selected (3)"
        );

        let selected_connected = compute_folder_toolbar_state(2, 5, false, false, false, true);
        assert!(selected_connected.can_batch_convert);
        assert!(selected_connected.can_batch_sync);

        let batch_busy = compute_folder_toolbar_state(2, 5, false, true, false, true);
        assert!(batch_busy.actions_disabled);
        assert!(!batch_busy.can_batch_convert);
        assert!(!batch_busy.can_batch_sync);

        let eject_busy = compute_folder_toolbar_state(2, 5, false, false, true, true);
        assert!(eject_busy.actions_disabled);
        assert!(!eject_busy.can_batch_convert);
        assert!(!eject_busy.can_batch_sync);
    }
}
