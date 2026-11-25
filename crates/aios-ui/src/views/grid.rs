//! Multi-column aspect-locked `Grid` view renderer (`1..=8` columns, 10px gap).

use aios_core::view::{EmptyView, GridAspectRatio, GridItemContent, GridView};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ParentElement, Styled, div, prelude::*, px,
};

use crate::components::{
    highlighted_text, render_empty_state, render_loading_state, section_header,
};
use crate::shell::LauncherWindow;
use crate::theme::{AiosTheme, parse_hex_color};

/// Renders a `GridView` with aspect-locked tiles on `bg.subtle`, `2px` `border.focus`
/// outline + `bg.selected` caption bar on selection, and full keyboard + click navigation.
pub fn render_grid_view(
    win: &LauncherWindow,
    grid: &GridView,
    theme: &AiosTheme,
    cx: &mut Context<LauncherWindow>,
) -> AnyElement {
    let frame = win.state.current();
    let entries = frame.filtered_grid_entries();

    if entries.is_empty() {
        if grid.is_loading {
            let msg = grid
                .loading_message
                .as_deref()
                .unwrap_or("Loading grid items...");
            return render_loading_state(msg, theme).into_any_element();
        }
        let fallback_empty = EmptyView {
            title: "No matching grid tiles".to_string(),
            description: Some(format!(
                "No tiles matched '{}'. Press Esc to clear the search filter.",
                frame.search_query
            )),
            icon: Some("0".to_string()),
            actions: Default::default(),
        };
        let empty = grid.empty_view.as_ref().unwrap_or(&fallback_empty);
        return render_empty_state(empty, theme).into_any_element();
    }

    let c = &theme.colors;
    let cols = usize::from(grid.columns.clamp(1, 8));
    let tile_visual_height = match grid.aspect_ratio {
        GridAspectRatio::Square => px(84.0),
        GridAspectRatio::Wide16x9 => px(64.0),
        GridAspectRatio::Portrait3x4 => px(112.0),
    };

    let mut scroll = div()
        .id("aios-grid-scroll")
        .size_full()
        .overflow_y_scroll()
        .track_scroll(&win.grid_scroll)
        .flex()
        .flex_col()
        .p(px(12.0))
        .gap(px(10.0));

    struct GridBucket<'a> {
        title: Option<String>,
        subtitle: Option<String>,
        items: Vec<(usize, &'a crate::state::FilteredGridEntry)>,
    }

    let showing_sections = frame.search_query.trim().is_empty();
    let mut buckets: Vec<GridBucket<'_>> = Vec::new();

    if showing_sections {
        for (flat_idx, entry) in entries.iter().enumerate() {
            let sec = &grid.sections[entry.section_index];
            let need_new = buckets
                .last()
                .and_then(|b| b.items.first())
                .is_none_or(|(_, first)| first.section_index != entry.section_index);
            if need_new {
                buckets.push(GridBucket {
                    title: sec.title.clone(),
                    subtitle: sec.subtitle.clone(),
                    items: vec![(flat_idx, entry)],
                });
            } else if let Some(last) = buckets.last_mut() {
                last.items.push((flat_idx, entry));
            }
        }
    } else {
        buckets.push(GridBucket {
            title: None,
            subtitle: None,
            items: entries.iter().enumerate().collect(),
        });
    }

    let mut child_cursor = 0usize;
    let mut selected_row_child_idx = 0usize;

    for bucket in buckets {
        if let Some(title) = bucket.title {
            scroll = scroll.child(section_header(&title, bucket.subtitle.as_deref(), theme));
            child_cursor += 1;
        }

        for row_chunk in bucket.items.chunks(cols) {
            let mut row = div()
                .w_full()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .gap(px(10.0));
            for &(flat_idx, entry) in row_chunk {
                let item = &grid.sections[entry.section_index].items[entry.item_index];
                let is_selected = flat_idx == frame.selected_index;
                if is_selected {
                    selected_row_child_idx = child_cursor;
                }

                let visual_body = match &item.content {
                    GridItemContent::ColorSwatch { hex } => div()
                        .w_full()
                        .h(tile_visual_height)
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(10.0))
                        .child(
                            div()
                                .size_full()
                                .rounded(theme.metrics.radius_sm)
                                .bg(parse_hex_color(hex))
                                .border_1()
                                .border_color(c.border_default),
                        ),
                    GridItemContent::Symbol { glyph } => div()
                        .w_full()
                        .h(tile_visual_height)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(if is_selected {
                            c.accent_fg_on_selected
                        } else {
                            c.text_primary
                        })
                        .font(theme.mono_font())
                        .text_size(px(30.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child(glyph.clone()),
                    GridItemContent::Monogram { text, accent } => div()
                        .w_full()
                        .h(tile_visual_height)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(if *accent || is_selected {
                            c.accent_fg
                        } else {
                            c.text_secondary
                        })
                        .font_family(theme.font_mono)
                        .text_size(px(22.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(text.clone()),
                };

                let mut caption = div()
                    .w_full()
                    .px(px(8.0))
                    .py(px(6.0))
                    .border_t_1()
                    .border_color(c.border_subtle)
                    .bg(if is_selected {
                        c.bg_selected
                    } else {
                        c.bg_canvas
                    })
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_color(c.text_primary)
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(px(16.0))
                            .child(highlighted_text(
                                &item.title,
                                &entry.match_result.primary_indices,
                                is_selected,
                                theme,
                            )),
                    );

                if let Some(sub) = &item.subtitle {
                    caption = caption.child(
                        div()
                            .text_color(c.text_muted)
                            .font_family(theme.font_mono)
                            .text_size(px(10.5))
                            .line_height(px(14.0))
                            .truncate()
                            .child(sub.clone()),
                    );
                }

                // Keep the tile's Taffy box model 100% identical between selected and unselected
                // states (constant 1px border, zero conditional padding). The 2nd pixel of the
                // 2px `border.focus` ring is painted as an absolute inner overlay so neither
                // neighboring tiles nor inner content can shift by even 1 sub-pixel.
                let tile = div()
                    .id(("aios-grid-tile", flat_idx))
                    .relative()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .rounded(theme.metrics.radius_md)
                    .bg(c.bg_subtle)
                    .border_1()
                    .border_color(if is_selected {
                        c.border_focus
                    } else {
                        c.border_subtle
                    })
                    .cursor_pointer()
                    .when(!is_selected, |d| {
                        d.hover(|s| s.border_color(c.border_default).bg(c.bg_hover))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.state.current().selected_index == flat_idx {
                            this.trigger_primary_action(window, cx);
                        } else {
                            this.state.current_mut().selected_index = flat_idx;
                            cx.notify();
                        }
                    }))
                    .child(visual_body)
                    .child(caption)
                    .when(is_selected, |d| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(c.border_focus),
                        )
                    });

                row = row.child(tile);
            }

            for _ in row_chunk.len()..cols {
                row = row.child(div().flex_1().min_w(px(0.0)));
            }

            scroll = scroll.child(row);
            child_cursor += 1;
        }
    }

    if win.take_scroll_sync() {
        win.grid_scroll.scroll_to_item(selected_row_child_idx);
    }

    scroll.into_any_element()
}
