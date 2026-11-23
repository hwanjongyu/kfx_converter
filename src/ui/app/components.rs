use super::KindleStudio;
use crate::state::EpubSortField;
use crate::theme::Theme;
use gpui::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SortColumnScope {
    LocalFolder,
    KindleExplorer,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ScrollbarMetrics {
    pub total_content_h: Pixels,
    pub effective_max: Pixels,
}

/// Computes clamped scrollbar thumb height while ensuring `min_thumb <= max_thumb`
/// for all viewport heights (including tight heights `20.0 < viewport_h < 46.0`).
fn compute_thumb_height(viewport_h: Pixels, total_content: Pixels) -> Pixels {
    let safe_content = total_content.max(viewport_h + px(1.0));
    let raw_thumb_h = (viewport_h / safe_content) * viewport_h;
    let max_cap = (viewport_h - px(10.0)).max(px(4.0));
    let max_thumb = (viewport_h * 0.70).max(px(36.0)).min(max_cap);
    let min_thumb = px(36.0).min(max_thumb);
    raw_thumb_h.clamp(min_thumb, max_thumb)
}

pub(super) fn compute_scrollbar_metrics(
    state: &UniformListScrollState,
    viewport_h: Pixels,
    item_count: usize,
    item_height: Pixels,
) -> ScrollbarMetrics {
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

    ScrollbarMetrics {
        total_content_h,
        effective_max,
    }
}

pub(super) fn compute_pointer_scroll_offset(
    track_bounds: Bounds<Pixels>,
    pointer_y: Pixels,
    effective_max: Pixels,
) -> Option<Pixels> {
    let viewport_h = track_bounds.size.height;
    if viewport_h > px(0.0) && effective_max > px(1.0) {
        let rel_y = (pointer_y - track_bounds.origin.y).clamp(px(0.0), viewport_h);
        let target_ratio = (rel_y / viewport_h).clamp(0.0, 1.0);
        Some(-(effective_max * target_ratio))
    } else {
        None
    }
}

pub(super) fn apply_scrollbar_pointer_y(
    scroll_handle: &UniformListScrollHandle,
    pointer_y: Pixels,
    item_count: usize,
    item_height: Pixels,
) -> bool {
    let state = scroll_handle.0.borrow();
    let track_bounds = state.base_handle.bounds();
    let metrics =
        compute_scrollbar_metrics(&state, track_bounds.size.height, item_count, item_height);
    if let Some(target_offset) =
        compute_pointer_scroll_offset(track_bounds, pointer_y, metrics.effective_max)
    {
        state.base_handle.set_offset(point(px(0.0), target_offset));
        true
    } else {
        false
    }
}

/// Computes thumb bounds within `track_bounds` from `metrics` and `scroll_offset_y`
/// (where `scroll_offset_y` matches `ScrollHandle::offset().y`: `0.0` at top, negative downward).
pub(super) fn compute_thumb_bounds(
    track_bounds: Bounds<Pixels>,
    metrics: ScrollbarMetrics,
    scroll_offset_y: Pixels,
) -> Option<Bounds<Pixels>> {
    let viewport_h = track_bounds.size.height;
    if metrics.effective_max <= px(1.0) || viewport_h <= px(20.0) {
        return None;
    }

    let total_content = metrics.total_content_h.max(viewport_h + px(1.0));
    let clamped_offset = (-scroll_offset_y).clamp(px(0.0), metrics.effective_max);
    let scroll_ratio = (clamped_offset / metrics.effective_max).clamp(0.0, 1.0);

    // Proportional thumb size, clamped:
    // Min: 36px when viewport allows, scaling safely down for tight viewports
    // Max: 70% of viewport (and never exceeding track minus padding)
    let thumb_h = compute_thumb_height(viewport_h, total_content);

    let available_travel = (viewport_h - thumb_h - px(6.0)).max(px(0.0));
    let thumb_top = track_bounds.origin.y + px(3.0) + (available_travel * scroll_ratio);
    let thumb_left = track_bounds.origin.x + px(2.0);
    let thumb_w = (track_bounds.size.width - px(4.0)).max(px(4.0));

    Some(Bounds {
        origin: point(thumb_left, thumb_top),
        size: size(thumb_w, thumb_h),
    })
}

impl KindleStudio {
    pub(super) fn render_sort_column_cell_with_scope(
        &self,
        scope: SortColumnScope,
        label: &'static str,
        target_field: EpubSortField,
        container: Div,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (active_field, active_dir, id_prefix) = match scope {
            SortColumnScope::LocalFolder => (
                self.folder_state.sort_field,
                self.folder_state.sort_direction,
                "col_sort_",
            ),
            SortColumnScope::KindleExplorer => (
                self.explorer_sort_field,
                self.explorer_sort_direction,
                "col_explorer_sort_",
            ),
        };
        let is_active = active_field == target_field;
        let arrow_symbol = if is_active {
            active_dir.symbol()
        } else {
            "↕"
        };

        container
            .id(ElementId::Name(
                format!("{id_prefix}{}", label.to_lowercase()).into(),
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
            .on_click(cx.listener(move |this, _, _, cx| match scope {
                SortColumnScope::LocalFolder => this.handle_sort_column_click(target_field, cx),
                SortColumnScope::KindleExplorer => {
                    this.handle_explorer_sort_column_click(target_field, cx)
                }
            }))
            .into_any_element()
    }

    pub(super) fn render_sort_column_cell(
        &self,
        label: &'static str,
        target_field: EpubSortField,
        container: Div,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_sort_column_cell_with_scope(
            SortColumnScope::LocalFolder,
            label,
            target_field,
            container,
            cx,
        )
    }

    pub(super) fn render_explorer_sort_column_cell(
        &self,
        label: &'static str,
        target_field: EpubSortField,
        container: Div,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_sort_column_cell_with_scope(
            SortColumnScope::KindleExplorer,
            label,
            target_field,
            container,
            cx,
        )
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
        let is_hovered = self.hovered_scrollbar == Some(id_prefix);

        let track_id = ElementId::Name(format!("{}_scrollbar_track", id_prefix).into());

        div()
            .id(track_id)
            .flex_shrink_0()
            .w(px(14.0))
            .h_full()
            .relative()
            .rounded_md()
            .cursor_pointer()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    if this.hovered_scrollbar != Some(id_prefix) {
                        this.hovered_scrollbar = Some(id_prefix);
                        cx.notify();
                    }
                } else if this.hovered_scrollbar == Some(id_prefix) {
                    this.hovered_scrollbar = None;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_this, ev: &MouseDownEvent, _, cx| {
                    if apply_scrollbar_pointer_y(
                        &handle_for_down,
                        ev.position.y,
                        item_count,
                        item_height,
                    ) {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(move |_this, ev: &MouseMoveEvent, _, cx| {
                if ev.dragging()
                    && apply_scrollbar_pointer_y(
                        &handle_for_move,
                        ev.position.y,
                        item_count,
                        item_height,
                    )
                {
                    cx.notify();
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
                        let metrics = compute_scrollbar_metrics(
                            &state,
                            track_bounds.size.height,
                            item_count,
                            item_height,
                        );

                        if let Some(thumb_bounds) = compute_thumb_bounds(
                            track_bounds,
                            metrics,
                            state.base_handle.offset().y,
                        ) {
                            // 3. Paint vibrant golden scrollbar thumb
                            let thumb_color = if is_hovered {
                                Theme::scrollbar_thumb_hover()
                            } else {
                                Theme::scrollbar_thumb()
                            };
                            let mut thumb_quad = fill(thumb_bounds, thumb_color);
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

#[cfg(test)]
mod tests {
    use super::{
        ScrollbarMetrics, compute_pointer_scroll_offset, compute_scrollbar_metrics,
        compute_thumb_bounds, compute_thumb_height,
    };
    use gpui::{Bounds, ItemSize, ScrollHandle, UniformListScrollState, point, px, size};

    #[test]
    fn test_compute_thumb_height_small_viewports_no_panic() {
        let viewport_heights = [0.0, 10.0, 20.0, 20.5, 25.0, 35.0, 45.9, 46.0, 100.0, 600.0];
        let content_heights = [50.0, 500.0, 10_000.0];

        for vh in viewport_heights {
            let viewport_h = px(vh);
            for ch in content_heights {
                let thumb_h = compute_thumb_height(viewport_h, px(ch));
                assert!(
                    thumb_h >= px(4.0),
                    "thumb_h ({:?}) must be >= 4px for viewport_h={vh}, total_content={ch}",
                    thumb_h
                );
                assert!(
                    thumb_h <= viewport_h.max(px(4.0)),
                    "thumb_h ({:?}) must be <= viewport_h.max(4px) for viewport_h={vh}, total_content={ch}",
                    thumb_h
                );
                if vh >= 46.0 {
                    assert!(
                        thumb_h >= px(36.0) && thumb_h <= viewport_h - px(10.0),
                        "normal viewport_h={vh} (content={ch}) thumb_h={:?} out of [36.0, {}]",
                        thumb_h,
                        vh - 10.0
                    );
                }
            }
        }
    }

    #[test]
    fn test_compute_scrollbar_metrics_measured_and_fallback() {
        // 1. Measured layout via last_item_size
        let measured_state = UniformListScrollState {
            base_handle: ScrollHandle::new(),
            deferred_scroll_to_item: None,
            last_item_size: Some(ItemSize {
                item: size(px(400.0), px(300.0)),
                contents: size(px(400.0), px(1200.0)),
            }),
            y_flipped: false,
        };
        let m1 = compute_scrollbar_metrics(&measured_state, px(300.0), 20, px(60.0));
        assert_eq!(m1.total_content_h, px(1200.0));
        assert_eq!(m1.effective_max, px(900.0));

        // 2. Fallback estimate when last_item_size is None
        let unmeasured_state = UniformListScrollState::default();
        let m2 = compute_scrollbar_metrics(&unmeasured_state, px(300.0), 10, px(68.0));
        assert_eq!(m2.total_content_h, px(680.0));
        assert_eq!(m2.effective_max, px(380.0));

        // 3. Non-scrollable when content fits inside viewport
        let m3 = compute_scrollbar_metrics(&unmeasured_state, px(500.0), 5, px(68.0));
        assert_eq!(m3.total_content_h, px(340.0));
        assert_eq!(m3.effective_max, px(0.0));
    }

    #[test]
    fn test_compute_thumb_bounds_clamping_and_hidden() {
        let track_bounds = Bounds {
            origin: point(px(100.0), px(50.0)),
            size: size(px(14.0), px(300.0)),
        };

        // Hidden when non-scrollable
        let no_scroll = ScrollbarMetrics {
            total_content_h: px(250.0),
            effective_max: px(0.0),
        };
        assert!(compute_thumb_bounds(track_bounds, no_scroll, px(0.0)).is_none());

        // Hidden when viewport <= 20px
        let tiny_track = Bounds {
            origin: point(px(100.0), px(50.0)),
            size: size(px(14.0), px(20.0)),
        };
        let scrollable = ScrollbarMetrics {
            total_content_h: px(1200.0),
            effective_max: px(900.0),
        };
        assert!(compute_thumb_bounds(tiny_track, scrollable, px(0.0)).is_none());

        // Top scroll offset
        let top = compute_thumb_bounds(track_bounds, scrollable, px(0.0)).unwrap();
        assert_eq!(top.origin.x, px(102.0));
        assert_eq!(top.origin.y, px(53.0));
        assert_eq!(top.size.width, px(10.0));

        // Bottom scroll offset (-effective_max) and over-scrolled clamping
        let bottom = compute_thumb_bounds(track_bounds, scrollable, px(-900.0)).unwrap();
        let clamped_bottom = compute_thumb_bounds(track_bounds, scrollable, px(-5000.0)).unwrap();
        assert_eq!(bottom, clamped_bottom);
        assert!(bottom.origin.y > top.origin.y);
        assert!(bottom.origin.y + bottom.size.height <= px(50.0 + 300.0 - 3.0));

        // Tight viewport (25px) does not panic or exceed track bounds
        let tight_track = Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(14.0), px(25.0)),
        };
        let tight = compute_thumb_bounds(tight_track, scrollable, px(-450.0)).unwrap();
        assert!(tight.size.height >= px(4.0));
        assert!(tight.origin.y + tight.size.height <= px(25.0));
    }

    #[test]
    fn test_compute_pointer_scroll_offset() {
        let track_bounds = Bounds {
            origin: point(px(10.0), px(40.0)),
            size: size(px(14.0), px(200.0)),
        };

        assert_eq!(
            compute_pointer_scroll_offset(track_bounds, px(140.0), px(0.0)),
            None
        );
        assert_eq!(
            compute_pointer_scroll_offset(track_bounds, px(10.0), px(800.0)),
            Some(px(0.0))
        );
        assert_eq!(
            compute_pointer_scroll_offset(track_bounds, px(140.0), px(800.0)),
            Some(px(-400.0))
        );
        assert_eq!(
            compute_pointer_scroll_offset(track_bounds, px(500.0), px(800.0)),
            Some(px(-800.0))
        );
    }
}
