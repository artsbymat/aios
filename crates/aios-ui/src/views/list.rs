//! `List` view renderer (+ 40/60 split `List.Item.Detail` Markdown/Metadata pane).

use aios_core::view::{EmptyView, ListView};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ParentElement, Styled, div, prelude::*, px,
};

use crate::components::{
    app_icon_or_badge, highlighted_text, render_accessory, render_empty_state,
    render_loading_state, render_markdown_blocks, render_metadata_list, section_header,
};
use crate::shell::LauncherWindow;
use crate::theme::AiosTheme;

/// Renders a `ListView` with 40px rows, 2px left Signal Copper selection rail,
/// fuzzy-matched glyph highlights, accessories, click selection/activation, and optional 40/60 split Detail pane.
pub fn render_list_view(
    win: &LauncherWindow,
    list: &ListView,
    theme: &AiosTheme,
    cx: &mut Context<LauncherWindow>,
) -> AnyElement {
    let frame = win.state.current();
    let entries = frame.filtered_list_entries();

    if entries.is_empty() {
        if list.is_loading {
            let msg = list
                .loading_message
                .as_deref()
                .unwrap_or("Loading list items...");
            return render_loading_state(msg, theme).into_any_element();
        }
        let fallback_empty = EmptyView {
            title: "No matching items".to_string(),
            description: Some(format!(
                "No results matched '{}'. Press Esc to clear the search filter.",
                frame.search_query
            )),
            icon: Some("0".to_string()),
            actions: Default::default(),
        };
        let empty = list.empty_view.as_ref().unwrap_or(&fallback_empty);
        let has_action = empty.actions.primary_action().is_some();
        return div()
            .id("aios-list-empty-trigger")
            .size_full()
            .when(has_action, |d| {
                d.cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.trigger_primary_action(window, cx);
                    }))
            })
            .child(render_empty_state(empty, theme))
            .into_any_element();
    }

    let c = &theme.colors;
    let mut list_col = div()
        .id("aios-list-scroll")
        .flex_1()
        .h_full()
        .overflow_y_scroll()
        .track_scroll(&win.list_scroll)
        .flex()
        .flex_col()
        .p(px(8.0))
        .gap(px(2.0));

    let mut last_section: Option<usize> = None;
    let showing_sections = list.filtering == aios_core::view::FilterMode::Plugin
        || frame.search_query.trim().is_empty();
    let mut child_cursor = 0usize;
    let mut selected_child_idx = 0usize;

    for (flat_idx, entry) in entries.iter().enumerate() {
        if showing_sections && last_section != Some(entry.section_index) {
            let section = &list.sections[entry.section_index];
            if let Some(title) = &section.title {
                list_col =
                    list_col.child(section_header(title, section.subtitle.as_deref(), theme));
                child_cursor += 1;
            }
            last_section = Some(entry.section_index);
        }

        let item = &list.sections[entry.section_index].items[entry.item_index];
        let is_selected = flat_idx == frame.selected_index;
        if is_selected {
            selected_child_idx = child_cursor;
        }
        child_cursor += 1;
        let badge_text = item
            .icon
            .as_deref()
            .unwrap_or_else(|| item.title.get(0..2).unwrap_or("AI"));

        let mut row_content = div()
            .id(("aios-list-row", flat_idx))
            .w_full()
            .h(theme.metrics.list_row_height)
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .px(px(10.0))
            .rounded(theme.metrics.radius_sm)
            .cursor_pointer()
            .bg(if is_selected {
                c.bg_selected
            } else {
                c.bg_canvas
            })
            .hover(|s| {
                s.bg(if is_selected {
                    c.bg_selected
                } else {
                    c.bg_hover
                })
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.state.current().selected_index == flat_idx {
                    this.trigger_primary_action(window, cx);
                } else {
                    this.state.current_mut().selected_index = flat_idx;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .w(px(2.0))
                    .h(px(22.0))
                    .flex_shrink_0()
                    .rounded(theme.metrics.radius_xs)
                    .bg(if is_selected {
                        c.accent_solid
                    } else {
                        c.bg_canvas
                    }),
            )
            .child(app_icon_or_badge(
                item.icon_path.as_deref(),
                badge_text,
                is_selected,
                theme,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .gap(px(8.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .text_color(c.text_primary)
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(px(18.0))
                            .child(highlighted_text(
                                &item.title,
                                &entry.match_result.primary_indices,
                                is_selected,
                                theme,
                            )),
                    )
                    .when(!list.is_showing_detail, |d| {
                        if let Some(sub) = &item.subtitle {
                            d.child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .text_color(c.text_secondary)
                                    .text_size(px(12.0))
                                    .line_height(px(16.0))
                                    .child(highlighted_text(
                                        sub,
                                        &entry.match_result.secondary_indices,
                                        is_selected,
                                        theme,
                                    )),
                            )
                        } else {
                            d
                        }
                    }),
            );

        if !list.is_showing_detail && !item.accessories.is_empty() {
            let mut acc_cluster = div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .flex_shrink_0();
            for acc in &item.accessories {
                acc_cluster = acc_cluster.child(render_accessory(acc, theme));
            }
            row_content = row_content.child(acc_cluster);
        }

        list_col = list_col.child(row_content);
    }

    if win.take_scroll_sync() {
        win.list_scroll.scroll_to_item(selected_child_idx);
    }

    if !list.is_showing_detail {
        return list_col.into_any_element();
    }

    let selected_item = entries
        .get(frame.selected_index)
        .map(|e| &list.sections[e.section_index].items[e.item_index]);

    let mut detail_pane = div()
        .id("aios-split-detail-scroll")
        .w_3_5()
        .min_w(px(0.0))
        .h_full()
        .overflow_y_scroll()
        .overflow_x_hidden()
        .border_l_1()
        .border_color(c.border_subtle)
        .p(px(16.0))
        .flex()
        .flex_col()
        .gap(px(14.0));

    if let Some(item) = selected_item {
        if let Some(detail) = &item.detail {
            detail_pane = detail_pane.child(render_markdown_blocks(&detail.markdown, theme));
            if !detail.metadata.is_empty() {
                detail_pane = detail_pane
                    .child(div().w_full().h(px(1.0)).bg(c.border_subtle))
                    .child(render_metadata_list(&detail.metadata, theme));
            }
        } else {
            detail_pane = detail_pane.child(
                div()
                    .text_color(c.text_muted)
                    .text_size(px(12.5))
                    .child("Select an item to preview its detail pane."),
            );
        }
    }

    div()
        .size_full()
        .flex()
        .flex_row()
        .child(div().w_2_5().min_w(px(280.0)).h_full().child(list_col))
        .child(detail_pane)
        .into_any_element()
}
