use gpui::*;
use crate::state::{case_insensitive_cmp, EpubSortField, KindleFile, SortDirection};
use crate::theme::Theme;
use super::KindleStudio;

impl KindleStudio {
    /// Renders the Connected Kindle Library Pane (Storage Meter, Format Filters, Book List, Delete).
    pub(super) fn render_kindle_pane(&self, cx: &mut Context<Self>, _in_split: bool) -> impl IntoElement {
        let is_offline = !self.device.connected;
        let active_filter = self.filter_format.as_deref().unwrap_or("ALL");

        let count_kfx = self.files.iter().filter(|f| f.format == "KFX").count();
        let count_wordwise = self.files.iter().filter(|f| f.has_word_wise).count();
        let count_azw3 = self.files.iter().filter(|f| f.format == "AZW3").count();
        let count_mobi = self.files.iter().filter(|f| f.format == "MOBI").count();
        let count_all = self.files.len();

        let search_lower = self.search_query.to_lowercase();
        let mut filtered_indices: Vec<usize> = self.files
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                let fmt_match = match self.filter_format.as_deref() {
                    Some("KFX") => f.format == "KFX",
                    Some("WORDWISE") => f.has_word_wise,
                    Some("AZW3") => f.format == "AZW3",
                    Some("MOBI") => f.format == "MOBI",
                    _ => true,
                };
                let query_match = if search_lower.is_empty() {
                    true
                } else {
                    f.name.to_lowercase().contains(&search_lower)
                        || f.title.to_lowercase().contains(&search_lower)
                        || f.author.to_lowercase().contains(&search_lower)
                };
                fmt_match && query_match
            })
            .map(|(idx, _)| idx)
            .collect();

        filtered_indices.sort_by(|&a_idx, &b_idx| {
            let a = &self.files[a_idx];
            let b = &self.files[b_idx];

            let cmp = match self.explorer_sort_field {
                EpubSortField::Title => case_insensitive_cmp(&a.title, &b.title),
                EpubSortField::Author => {
                    let a_unknown = a.author.eq_ignore_ascii_case("unknown");
                    let b_unknown = b.author.eq_ignore_ascii_case("unknown");
                    match (a_unknown, b_unknown) {
                        (true, false) => std::cmp::Ordering::Greater,
                        (false, true) => std::cmp::Ordering::Less,
                        _ => case_insensitive_cmp(&a.author, &b.author),
                    }
                }
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
        let file_count = filtered_indices.len();

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
                                format!("Total {} books (Filtered: {})", count_all, file_count)
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
                } else if filtered_indices.is_empty() {
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
                    let view = cx.entity().downgrade();
                    let indices = filtered_indices.clone();
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
                        .child(self.render_kindle_table_column_headers(cx))
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
                                        .id("kindle_files_list_scroll")
                                        .flex_1()
                                        .min_w_0()
                                        .h_full()
                                        .overflow_hidden()
                                        .child(
                                            uniform_list(
                                                "kindle_files_uniform_list",
                                                indices.len(),
                                                cx.processor(move |this, range: std::ops::Range<usize>, _window, _cx| {
                                                    range
                                                        .map(|r_idx| {
                                                            let file_idx = indices[r_idx];
                                                            let file = &this.files[file_idx];
                                                            let is_confirming = this.confirm_delete_file.as_deref() == Some(&file.name);
                                                            Self::render_kindle_file_row(file, &view, is_confirming)
                                                        })
                                                        .collect()
                                                }),
                                            )
                                            .track_scroll(self.explorer_scroll_handle.clone())
                                            .flex_1()
                                            .min_w_0()
                                            .h_full(),
                                        )
                                )
                                .child(
                                    self.render_custom_scrollbar(&self.explorer_scroll_handle, filtered_indices.len(), px(48.0), "kindle", cx)
                                )
                        )
                        .into_any_element()
                }
            )
    }

    /// Renders sortable column headers for the connected Kindle explorer table.
    pub(super) fn render_kindle_table_column_headers(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sort_field = self.explorer_sort_field;
        let sort_dir = self.explorer_sort_direction;

        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(32.0))
            .px_3()
            .bg(Theme::bg_card_hover())
            .border_b_1()
            .border_color(Theme::border())
            .gap_2()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            // 1. Format Header
            .child(
                self.render_explorer_sort_column_cell(
                    "Format",
                    EpubSortField::Status,
                    sort_field,
                    sort_dir,
                    div().w(px(70.0)).flex_shrink_0(),
                    cx,
                )
            )
            // 2. Title Header
            .child(
                self.render_explorer_sort_column_cell(
                    "Title",
                    EpubSortField::Title,
                    sort_field,
                    sort_dir,
                    div().flex_1().min_w_0(),
                    cx,
                )
            )
            // 3. Author Header
            .child(
                self.render_explorer_sort_column_cell(
                    "Author",
                    EpubSortField::Author,
                    sort_field,
                    sort_dir,
                    div().w(px(130.0)).flex_shrink_0().min_w_0(),
                    cx,
                )
            )
            // 4. Size Header
            .child(
                self.render_explorer_sort_column_cell(
                    "Size",
                    EpubSortField::FileSize,
                    sort_field,
                    sort_dir,
                    div().w(px(65.0)).flex_shrink_0(),
                    cx,
                )
            )
            // 5. Date Header
            .child(
                self.render_explorer_sort_column_cell(
                    "Date",
                    EpubSortField::DateModified,
                    sort_field,
                    sort_dir,
                    div().w(px(75.0)).flex_shrink_0(),
                    cx,
                )
            )
            // Action spacer
            .child(div().w(px(32.0)).flex_shrink_0())
            // Scrollbar spacer (14px)
            .child(div().w(px(14.0)).flex_shrink_0())
    }

    /// Renders a single Kindle file row with strict 48px height for virtualization.
    pub(super) fn render_kindle_file_row(
        file: &KindleFile,
        view: &WeakEntity<Self>,
        is_confirming: bool,
    ) -> AnyElement {
        let badge_bg = match file.format.as_str() {
            "KFX" => Theme::accent_amber(),
            "AZW3" => Theme::accent_blue(),
            "MOBI" => Theme::accent_purple(),
            _ => Theme::text_muted(),
        };

        let file_name = file.name.clone();
        let view_del = view.clone();
        let f_del = file_name.clone();

        div()
            .w_full()
            .h(px(48.0))
            .pb(px(4.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .w_full()
                    .h(px(44.0))
                    .px_3()
                    .rounded_md()
                    .bg(Theme::bg_card())
                    .border_1()
                    .border_color(Theme::border())
                    .gap_2()
                    // 1. Format & Word Wise badges
                    .child(
                        div()
                            .w(px(70.0))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_1()
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
                                        .child("WW"),
                                )
                            } else {
                                None
                            }),
                    )
                    // 2. Title
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::text_primary())
                            .truncate()
                            .child(file.title.clone()),
                    )
                    // 3. Author
                    .child(
                        div()
                            .w(px(130.0))
                            .flex_shrink_0()
                            .min_w_0()
                            .overflow_hidden()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .truncate()
                            .child(file.author.clone()),
                    )
                    // 4. Size
                    .child(
                        div()
                            .w(px(65.0))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child(file.size_formatted.clone()),
                    )
                    // 5. Date
                    .child(
                        div()
                            .w(px(75.0))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(Theme::text_muted())
                            .child(file.modified.clone()),
                    )
                    // 6. Delete Action Button
                    .child(
                        div()
                            .w(px(32.0))
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .child(
                                div()
                                    .id(ElementId::Name(format!("del_{}", file_name).into()))
                                    .cursor_pointer()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(if is_confirming { Theme::accent_red() } else { Theme::bg_card_hover() })
                                    .text_xs()
                                    .text_color(if is_confirming { Theme::bg_root() } else { Theme::accent_red() })
                                    .child(if is_confirming { "Confirm?" } else { "🗑" })
                                    .on_click(move |_ev, _window, cx| {
                                        view_del.update(cx, |this, cx| {
                                            if this.confirm_delete_file.as_deref() == Some(&f_del) {
                                                this.confirm_delete_file = None;
                                                this.delete_file(&f_del, cx);
                                            } else {
                                                this.confirm_delete_file = Some(f_del.clone());
                                                cx.notify();
                                            }
                                        }).ok();
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }
}
