use super::KindleStudio;
use crate::state::{BatchProgressState, EpubSortField, LocalEpubItem, LocalEpubStatus};
use crate::theme::Theme;
use gpui::*;

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

        let filtered_indices = self.folder_state.sorted_and_filtered_indices();

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
                            let waiting = bp.total_tasks.saturating_sub(bp.completed_tasks + bp.failed_tasks + bp.active_tasks);
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
                                        .child(format!("✓ {} done", bp.completed_tasks)),
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
                                        .child(format!("⚡ {} active", bp.active_tasks)),
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
                                        .child(format!("⏳ {} waiting", waiting)),
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
                                        .child(format!("{}/{} books selected", selected_count, total_count)),
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
                                        if filtered_indices.is_empty() {
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
                                                        .child(
                                                            if self.folder_state.current_folder.is_none() {
                                                                "Click 'Choose Folder' above to select your EPUB directory."
                                                            } else {
                                                                "No EPUB books found in this folder."
                                                            }
                                                        ),
                                                )
                                                .into_any_element()
                                        } else {
                                            let view = cx.entity().downgrade();
                                            let indices = filtered_indices.clone();
                                            uniform_list(
                                                "folder_epub_uniform_list",
                                                indices.len(),
                                                cx.processor(move |this, range: std::ops::Range<usize>, _window, _cx| {
                                                    range
                                                        .map(|r_idx| {
                                                             let item_idx = indices[r_idx];
                                                             let item = &this.folder_state.items[item_idx];
                                                             Self::render_epub_card(item, &view)
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
                                .child(format!("📲 Transfer to Kindle ({})", selected_count))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.convert_selected_epubs(true, cx);
                                })),
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
        let sort_field = self.folder_state.sort_field;
        let sort_dir = self.folder_state.sort_direction;

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
                sort_field,
                sort_dir,
                div().flex_1().min_w_0(),
                cx,
            ))
            // 2. Author Column Header
            .child(self.render_sort_column_cell(
                "Author",
                EpubSortField::Author,
                sort_field,
                sort_dir,
                div().w(px(140.0)).flex_shrink_0().min_w_0(),
                cx,
            ))
            // 3. Size Column Header
            .child(self.render_sort_column_cell(
                "Size",
                EpubSortField::FileSize,
                sort_field,
                sort_dir,
                div().w(px(70.0)).flex_shrink_0(),
                cx,
            ))
            // 4. Date Column Header
            .child(self.render_sort_column_cell(
                "Date",
                EpubSortField::DateModified,
                sort_field,
                sort_dir,
                div().w(px(85.0)).flex_shrink_0(),
                cx,
            ))
            // 5. Status Column Header
            .child(self.render_sort_column_cell(
                "Status",
                EpubSortField::Status,
                sort_field,
                sort_dir,
                div().w(px(160.0)).flex_shrink_0(),
                cx,
            ))
            // Scrollbar spacer (14px)
            .child(div().w(px(14.0)).flex_shrink_0())
    }

    /// Renders a single EPUB item card with strict 68px height for virtualization.
    pub(super) fn render_epub_card(item: &LocalEpubItem, view: &WeakEntity<Self>) -> AnyElement {
        let item_id = item.id;
        let item_path = item.path.clone();
        let is_sel = item.is_selected;
        let is_processing = matches!(&item.status, LocalEpubStatus::Processing { .. });
        let progress_val = if let LocalEpubStatus::Processing { progress, .. } = &item.status {
            *progress
        } else {
            0
        };

        let status_badge = match &item.status {
            LocalEpubStatus::Unconverted => div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .bg(Theme::bg_card_hover())
                .text_xs()
                .text_color(Theme::text_muted())
                .child("⚪ Unconverted"),
            LocalEpubStatus::InQueue => div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .bg(Theme::bg_card_hover())
                .border_1()
                .border_color(Theme::border())
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(Theme::text_muted())
                .child("⏳ In Queue"),
            LocalEpubStatus::Processing { stage, progress } => div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .bg(Theme::accent_amber())
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(Theme::bg_root())
                .child(format!("🟡 {} ({}%)", stage, progress)),
            LocalEpubStatus::ConvertedLocal => div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .bg(Theme::accent_blue())
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(Theme::text_primary())
                .child("🔵 Local KFX"),
            LocalEpubStatus::OnDevice => div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .bg(Theme::accent_green())
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(Theme::bg_root())
                .child("🟢 On Kindle"),
            LocalEpubStatus::Error(err) => div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .bg(Theme::accent_red())
                .text_xs()
                .text_color(Theme::text_primary())
                .child(format!("Error: {}", err)),
        };

        let quick_btn = if item.status == LocalEpubStatus::Unconverted {
            let p = item_path.clone();
            let view_quick = view.clone();
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
                    .on_click(move |_ev, _window, cx| {
                        cx.stop_propagation();
                        view_quick
                            .update(cx, |this, cx| {
                                this.convert_single_epub(p.clone(), false, cx);
                            })
                            .ok();
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
                    .child(if is_processing {
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .h(px(3.0))
                            .bg(Theme::accent_amber())
                            .w(DefiniteLength::Fraction(
                                (progress_val as f32 / 100.0).clamp(0.08, 1.0),
                            ))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    })
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

    pub(super) fn render_batch_progress_bar(
        &self,
        bp: &BatchProgressState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pct = bp.overall_percentage();
        let frac = bp.overall_progress();
        let is_done = (bp.completed_tasks + bp.failed_tasks >= bp.total_tasks || bp.is_cancelled)
            && bp.active_tasks == 0;
        let is_sync = bp.is_sync;

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
                    .border_color(if is_done { Theme::accent_green() } else { Theme::border() })
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .h_full()
                            .bg(if is_done {
                                hsla(142.0 / 360.0, 0.70, 0.45, 0.25)
                            } else if is_sync {
                                hsla(160.0 / 360.0, 0.84, 0.39, 0.25)
                            } else {
                                hsla(38.0 / 360.0, 0.92, 0.50, 0.25)
                            })
                            .w(DefiniteLength::Fraction(frac.clamp(0.02, 1.0))),
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
                                    .child(
                                        div()
                                            .text_base()
                                            .child(if is_done { "✓" } else if is_sync { "📲" } else { "⚡" }),
                                    )
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
                                                            .child(if is_done {
                                                                format!("Finished processing {} books successfully!", bp.completed_tasks)
                                                            } else if is_sync {
                                                                format!("Transferring to Kindle: {} of {} done ({}%)", bp.completed_tasks, bp.total_tasks, pct)
                                                            } else {
                                                                format!("Converting Books: {} of {} done ({}%)", bp.completed_tasks, bp.total_tasks, pct)
                                                            }),
                                                    )
                                                    .child(
                                                        if bp.failed_tasks > 0 {
                                                            div()
                                                                .text_xs()
                                                                .text_color(Theme::accent_red())
                                                                .child(format!("({} failed)", bp.failed_tasks))
                                                                .into_any_element()
                                                        } else {
                                                            div().into_any_element()
                                                        }
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(Theme::text_muted())
                                                    .child(if is_done {
                                                        "All tasks finished. Click Dismiss to close.".to_string()
                                                    } else if !bp.current_filename.is_empty() {
                                                        format!("Active: {} ({})", bp.current_filename, bp.current_stage)
                                                    } else {
                                                        bp.current_stage.clone()
                                                    }),
                                            ),
                                    ),
                            )
                            .child(
                                if is_done {
                                    div()
                                        .id("btn_batch_dismiss")
                                        .cursor_pointer()
                                        .px_3()
                                        .py_1p5()
                                        .rounded_lg()
                                        .bg(Theme::accent_green())
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(Theme::bg_root())
                                        .child("✓ Dismiss")
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
                                }
                            ),
                    ),
            )
            .into_any_element()
    }
}
