//! `Detail` view renderer: Markdown prose + fixed `240px` right `Metadata` sidebar.

use aios_core::view::DetailView;
use gpui::{AnyElement, IntoElement, ParentElement, Styled, div, prelude::*, px};

use crate::components::{render_loading_state, render_markdown_blocks, render_metadata_list};
use crate::theme::AiosTheme;

/// Renders a `DetailView` matching `DESIGN.md`: full-flex Markdown canvas on the left
/// and a fixed `240px` `bg.subtle` / `1px` `border.subtle` Metadata sidebar on the right.
pub fn render_detail_view(detail: &DetailView, theme: &AiosTheme) -> AnyElement {
    if detail.is_loading && detail.markdown.trim().is_empty() {
        let msg = detail
            .loading_message
            .as_deref()
            .unwrap_or("Loading detail content...");
        return render_loading_state(msg, theme).into_any_element();
    }

    let c = &theme.colors;
    let markdown_pane = div()
        .id("aios-detail-markdown-scroll")
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .flex()
        .flex_col()
        .overflow_y_scroll()
        .overflow_x_hidden()
        .p(px(16.0))
        .child(render_markdown_blocks(&detail.markdown, theme));

    if detail.metadata.is_empty() {
        return markdown_pane.into_any_element();
    }

    let sidebar = div()
        .id("aios-detail-metadata-scroll")
        .w(theme.metrics.metadata_sidebar_width)
        .min_w(theme.metrics.metadata_sidebar_width)
        .max_w(theme.metrics.metadata_sidebar_width)
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .overflow_y_scroll()
        .overflow_x_hidden()
        .p(px(16.0))
        .border_l_1()
        .border_color(c.border_subtle)
        .bg(c.bg_subtle)
        .child(render_metadata_list(&detail.metadata, theme));

    div()
        .size_full()
        .min_w(px(0.0))
        .overflow_hidden()
        .flex()
        .flex_row()
        .child(markdown_pane)
        .child(sidebar)
        .into_any_element()
}
