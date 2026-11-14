use super::KindleStudio;
use crate::state::{EpubSortField, SortDirection};
use crate::theme::Theme;
use gpui::*;

impl KindleStudio {
    pub(super) fn render_sort_column_cell(
        &self,
        label: &'static str,
        target_field: EpubSortField,
        active_field: EpubSortField,
        active_dir: SortDirection,
        container: Div,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_active = active_field == target_field;
        let arrow_symbol = if is_active {
            active_dir.symbol()
        } else {
            "↕"
        };

        container
            .id(ElementId::Name(
                format!("col_sort_{}", label.to_lowercase()).into(),
            ))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .text_color(if is_active {
                        Theme::accent_amber()
                    } else {
                        Theme::text_muted()
                    })
                    .child(label),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if is_active {
                        Theme::accent_amber()
                    } else {
                        Theme::text_muted()
                    })
                    .child(arrow_symbol),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.handle_sort_column_click(target_field, cx);
            }))
            .into_any_element()
    }

    pub(super) fn render_explorer_sort_column_cell(
        &self,
        label: &'static str,
        target_field: EpubSortField,
        active_field: EpubSortField,
        active_dir: SortDirection,
        container: Div,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_active = active_field == target_field;
        let arrow_symbol = if is_active {
            active_dir.symbol()
        } else {
            "↕"
        };

        container
            .id(ElementId::Name(
                format!("col_explorer_sort_{}", label.to_lowercase()).into(),
            ))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .text_color(if is_active {
                        Theme::accent_amber()
                    } else {
                        Theme::text_muted()
                    })
                    .child(label),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if is_active {
                        Theme::accent_amber()
                    } else {
                        Theme::text_muted()
                    })
                    .child(arrow_symbol),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.handle_explorer_sort_column_click(target_field, cx);
            }))
            .into_any_element()
    }

    pub(super) fn render_custom_scrollbar(
        &self,
        scroll_handle: &UniformListScrollHandle,
        item_count: usize,
        item_height: Pixels,
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_this, ev: &MouseDownEvent, _, cx| {
                    let state = handle_for_down.0.borrow();
                    let track_bounds = state.base_handle.bounds();
                    let viewport_h = track_bounds.size.height;
                    let (measured_max, total_content_h) = if let Some(size) = state.last_item_size {
                        (
                            (size.contents.height - size.item.height).max(px(0.0)),
                            size.contents.height,
                        )
                    } else {
                        let max = state.base_handle.max_offset().height;
                        let est = item_height * item_count;
                        (max.max(px(0.0)), est)
                    };

                    let effective_max = if measured_max > px(1.0) {
                        measured_max
                    } else if item_count > 0 && total_content_h > viewport_h {
                        total_content_h - viewport_h
                    } else {
                        px(0.0)
                    };

                    if viewport_h > px(0.0) && effective_max > px(1.0) {
                        let click_y = ev.position.y;
                        let rel_y = (click_y - track_bounds.origin.y).clamp(px(0.0), viewport_h);
                        let target_ratio = (rel_y / viewport_h).clamp(0.0, 1.0);
                        let target_offset = -(effective_max * target_ratio);
                        state.base_handle.set_offset(point(px(0.0), target_offset));
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(move |_this, ev: &MouseMoveEvent, _, cx| {
                if ev.dragging() {
                    let state = handle_for_move.0.borrow();
                    let track_bounds = state.base_handle.bounds();
                    let viewport_h = track_bounds.size.height;
                    let (measured_max, total_content_h) = if let Some(size) = state.last_item_size {
                        (
                            (size.contents.height - size.item.height).max(px(0.0)),
                            size.contents.height,
                        )
                    } else {
                        let max = state.base_handle.max_offset().height;
                        let est = item_height * item_count;
                        (max.max(px(0.0)), est)
                    };

                    let effective_max = if measured_max > px(1.0) {
                        measured_max
                    } else if item_count > 0 && total_content_h > viewport_h {
                        total_content_h - viewport_h
                    } else {
                        px(0.0)
                    };

                    if viewport_h > px(0.0) && effective_max > px(1.0) {
                        let click_y = ev.position.y;
                        let rel_y = (click_y - track_bounds.origin.y).clamp(px(0.0), viewport_h);
                        let target_ratio = (rel_y / viewport_h).clamp(0.0, 1.0);
                        let target_offset = -(effective_max * target_ratio);
                        state.base_handle.set_offset(point(px(0.0), target_offset));
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
                        let state = handle_for_paint.0.borrow();
                        let viewport_h = track_bounds.size.height;
                        let (measured_max, total_content_h) =
                            if let Some(size) = state.last_item_size {
                                (
                                    (size.contents.height - size.item.height).max(px(0.0)),
                                    size.contents.height,
                                )
                            } else {
                                let max = state.base_handle.max_offset().height;
                                let est = item_height * item_count;
                                (max.max(px(0.0)), est)
                            };

                        let effective_max = if measured_max > px(1.0) {
                            measured_max
                        } else if item_count > 0 && total_content_h > viewport_h {
                            total_content_h - viewport_h
                        } else {
                            px(0.0)
                        };

                        // Only render thumb if there is scrollable content
                        if effective_max > px(1.0) && viewport_h > px(20.0) {
                            let total_content = total_content_h.max(viewport_h + px(1.0));
                            let scroll_offset_y =
                                (-state.base_handle.offset().y).clamp(px(0.0), effective_max);
                            let scroll_ratio = (scroll_offset_y / effective_max).clamp(0.0, 1.0);

                            // Proportional thumb size, clamped:
                            // Min: 36px so it's always easily draggable
                            // Max: 70% of viewport so it NEVER fills the entire height and always leaves dragging room
                            let raw_thumb_h = (viewport_h / total_content) * viewport_h;
                            let max_allowed_thumb = (viewport_h * 0.70).max(px(36.0));
                            let thumb_h = raw_thumb_h
                                .clamp(px(36.0), max_allowed_thumb.min(viewport_h - px(10.0)));

                            let available_travel = (viewport_h - thumb_h - px(6.0)).max(px(0.0));
                            let thumb_top =
                                track_bounds.origin.y + px(3.0) + (available_travel * scroll_ratio);
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
}
