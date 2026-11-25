//! Reusable `DESIGN.md` chrome components: official AIOS mark (`Modular Panes · Left Focus`),
//! Keycap tokens, fuzzy-highlighted text runs, Markdown blocks, Metadata sidebar,
//! Empty/Loading/Error states, ActionPanel popover, FilterDropdown, and ConfirmAlert modal.

use std::ops::Range;
use std::path::PathBuf;
use std::time::Duration;

use aios_core::view::{
    Accessory, ConfirmAlert, EmptyView, ErrorView, MetadataEntry, SemanticTone, ToastMessage,
    ToastStyle,
};
use gpui::{
    Animation, AnimationExt, AnyElement, Background, BoxShadow, Div, FontWeight, HighlightStyle,
    Hsla, IntoElement, ObjectFit, ParentElement, PathBuilder, Rgba, Styled, StyledText, canvas,
    div, img, point, prelude::*, px, relative,
};

use crate::theme::AiosTheme;

/// Converts sorted Unicode character indices from `aios-fuzzy` into merged UTF-8 byte ranges
/// suitable for `gpui::StyledText::with_highlights`.
#[must_use]
pub fn char_indices_to_byte_ranges(text: &str, char_indices: &[usize]) -> Vec<Range<usize>> {
    if char_indices.is_empty() {
        return Vec::new();
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut target_iter = char_indices.iter().copied().peekable();

    for (char_idx, (byte_idx, ch)) in text.char_indices().enumerate() {
        while let Some(&next_target) = target_iter.peek() {
            if next_target < char_idx {
                target_iter.next();
            } else {
                break;
            }
        }
        if target_iter.peek() == Some(&char_idx) {
            target_iter.next();
            let end = byte_idx + ch.len_utf8();
            if let Some(last) = ranges.last_mut()
                && last.end == byte_idx
            {
                last.end = end;
            } else {
                ranges.push(byte_idx..end);
            }
        }
    }

    ranges
}

/// Renders text with `aios-fuzzy` matched characters highlighted in Signal Copper SemiBold.
pub fn highlighted_text(
    text: &str,
    char_indices: &[usize],
    selected: bool,
    theme: &AiosTheme,
) -> AnyElement {
    let ranges = char_indices_to_byte_ranges(text, char_indices);
    if ranges.is_empty() {
        return div().truncate().child(text.to_owned()).into_any_element();
    }

    let highlight = HighlightStyle {
        color: Some(theme.colors.accent_fg_hsla(selected)),
        font_weight: Some(FontWeight::SEMIBOLD),
        ..Default::default()
    };

    let styled = StyledText::new(text.to_owned())
        .with_highlights(ranges.into_iter().map(|range| (range, highlight)));
    div().truncate().child(styled).into_any_element()
}

/// Renders the approved AIOS brand symbol (`Modular Panes · Left Focus`):
/// top-left Signal Copper focal circle paired with three Basalt/Alabaster Wayland panes.
pub fn aios_brand_mark(theme: &AiosTheme) -> Div {
    let c = &theme.colors;
    div()
        .w(px(22.0))
        .h(px(22.0))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.5))
        .child(
            // Top row: Signal Copper focal circle (left) + search bar pane (right)
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(2.5))
                .child(
                    div()
                        .w(px(6.0))
                        .h(px(6.0))
                        .rounded(px(9999.0))
                        .bg(c.accent_solid),
                )
                .child(
                    div()
                        .flex_1()
                        .h(px(5.0))
                        .rounded(px(2.0))
                        .bg(c.text_primary),
                ),
        )
        .child(
            // Bottom row: 40/60 split modular panes
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(2.5))
                .child(
                    div()
                        .w(px(8.0))
                        .h(px(9.0))
                        .rounded(px(2.0))
                        .bg(c.text_secondary),
                )
                .child(
                    div()
                        .flex_1()
                        .h(px(9.0))
                        .rounded(px(2.0))
                        .bg(c.border_strong),
                ),
        )
}

/// Crisp vector return-arrow icon (`↵`) painted via GPUI `PathBuilder` so it never breaks on missing font glyphs.
pub fn enter_arrow_icon(color: Rgba) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let ox = bounds.origin.x;
            let oy = bounds.origin.y;
            let w = bounds.size.width;
            let h = bounds.size.height;

            // Vertical drop on right + horizontal shaft to left
            let mut shaft = PathBuilder::stroke(px(1.4));
            shaft.move_to(point(ox + w * 0.82, oy + h * 0.22));
            shaft.line_to(point(ox + w * 0.82, oy + h * 0.62));
            shaft.line_to(point(ox + w * 0.22, oy + h * 0.62));
            if let Ok(path) = shaft.build() {
                window.paint_path(path, color);
            }

            // Left-pointing arrowhead chevron
            let mut head = PathBuilder::stroke(px(1.4));
            head.move_to(point(ox + w * 0.44, oy + h * 0.38));
            head.line_to(point(ox + w * 0.20, oy + h * 0.62));
            head.line_to(point(ox + w * 0.44, oy + h * 0.86));
            if let Ok(path) = head.build() {
                window.paint_path(path, color);
            }
        },
    )
    .w(px(12.0))
    .h(px(12.0))
    .flex_shrink_0()
    .into_any_element()
}

/// Crisp vector checkmark icon (`✓`) matching FigCraft node `405/406` (`Check Box Checked`).
pub fn checkmark_icon(color: Rgba) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let ox = bounds.origin.x;
            let oy = bounds.origin.y;
            let w = bounds.size.width;
            let h = bounds.size.height;

            let mut check = PathBuilder::stroke(px(1.8));
            check.move_to(point(ox + w * 0.16, oy + h * 0.54));
            check.line_to(point(ox + w * 0.40, oy + h * 0.78));
            check.line_to(point(ox + w * 0.86, oy + h * 0.24));
            if let Ok(path) = check.build() {
                window.paint_path(path, color);
            }
        },
    )
    .w(px(11.0))
    .h(px(11.0))
    .flex_shrink_0()
    .into_any_element()
}

/// Crisp vector left-arrow icon (`←`) for header Back navigation button.
pub fn back_arrow_icon(color: Rgba) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let ox = bounds.origin.x;
            let oy = bounds.origin.y;
            let w = bounds.size.width;
            let h = bounds.size.height;

            let mut shaft = PathBuilder::stroke(px(1.5));
            shaft.move_to(point(ox + w * 0.82, oy + h * 0.50));
            shaft.line_to(point(ox + w * 0.20, oy + h * 0.50));
            if let Ok(path) = shaft.build() {
                window.paint_path(path, color);
            }

            let mut head = PathBuilder::stroke(px(1.5));
            head.move_to(point(ox + w * 0.46, oy + h * 0.24));
            head.line_to(point(ox + w * 0.20, oy + h * 0.50));
            head.line_to(point(ox + w * 0.46, oy + h * 0.76));
            if let Ok(path) = head.build() {
                window.paint_path(path, color);
            }
        },
    )
    .w(px(13.0))
    .h(px(13.0))
    .flex_shrink_0()
    .into_any_element()
}

/// Recessed tactile Enter keycap button matching `fig/aios.figcraft` (vector return icon inside keycap).
pub fn enter_keycap(theme: &AiosTheme) -> Div {
    let c = &theme.colors;
    div()
        .flex_shrink_0()
        .min_w(px(22.0))
        .h(px(20.0))
        .px(px(5.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme.metrics.radius_xs)
        .bg(c.bg_keycap)
        .border_1()
        .border_b_2()
        .border_color(c.border_default)
        .child(enter_arrow_icon(c.text_primary))
}

/// Identity motif from `DESIGN.md`: tactile recessed keycap pill (`IBM Plex Mono` 11px).
/// Automatically renders the vector Enter keycap button when `label` is `"Enter"` or `"↵"`,
/// and pairs modifier + vector Enter keycap for `"Ctrl+Enter"`.
pub fn keycap(label: &str, theme: &AiosTheme) -> Div {
    if label == "Enter" || label == "↵" {
        return enter_keycap(theme);
    }
    if label == "Ctrl+Enter" {
        return div()
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(3.0))
            .child(keycap_text_pill("Ctrl", theme))
            .child(enter_keycap(theme));
    }
    keycap_text_pill(label, theme)
}

fn keycap_text_pill(label: &str, theme: &AiosTheme) -> Div {
    let c = &theme.colors;
    div()
        .flex_shrink_0()
        .h(px(20.0))
        .px(px(6.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme.metrics.radius_xs)
        .bg(c.bg_keycap)
        .border_1()
        .border_b_2()
        .border_color(c.border_default)
        .text_color(c.text_primary)
        .font(theme.mono_font())
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .line_height(px(14.0))
        .whitespace_nowrap()
        .child(label.to_owned())
}

/// Renders a semantic status badge pairing color with readable text (never color alone).
pub fn semantic_tag(label: &str, tone: SemanticTone, theme: &AiosTheme) -> Div {
    let (fg, bg) = theme.colors.tone_colors(tone);
    div()
        .px(px(6.0))
        .py(px(2.0))
        .rounded(theme.metrics.radius_xs)
        .bg(bg)
        .border_1()
        .border_color(theme.colors.border_subtle)
        .text_color(fg)
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .line_height(px(14.0))
        .child(label.to_owned())
}

/// Renders a list item accessory (`Text`, `Tag`, or `Date`).
pub fn render_accessory(accessory: &Accessory, theme: &AiosTheme) -> AnyElement {
    match accessory {
        Accessory::Text { value } | Accessory::Date { value } => div()
            .text_color(theme.colors.text_muted)
            .font_family(theme.font_mono)
            .text_size(px(11.0))
            .line_height(px(15.0))
            .child(value.clone())
            .into_any_element(),
        Accessory::Tag { label, tone } => semantic_tag(label, *tone, theme).into_any_element(),
    }
}

/// Renders a resolved XDG application icon (PNG/SVG) inside a 24x24px frame, falling back
/// cleanly to [`item_badge`] when no icon file is present or if decoding fails.
pub fn app_icon_or_badge(
    icon_path: Option<&str>,
    fallback_label: &str,
    selected: bool,
    theme: &AiosTheme,
) -> AnyElement {
    if let Some(path) = icon_path {
        let pb = PathBuf::from(path);
        let label_owned = fallback_label.to_owned();
        let theme_clone = theme.clone();
        return div()
            .w(px(24.0))
            .h(px(24.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.metrics.radius_xs)
            .overflow_hidden()
            .child(
                img(pb)
                    .w(px(22.0))
                    .h(px(22.0))
                    .object_fit(ObjectFit::Contain)
                    .with_fallback(move || {
                        item_badge(&label_owned, selected, &theme_clone).into_any_element()
                    }),
            )
            .into_any_element();
    }
    item_badge(fallback_label, selected, theme).into_any_element()
}

/// Renders a compact 24x24 geometric item badge (monogram/glyph) without stock sparkle icons.
pub fn item_badge(label: &str, selected: bool, theme: &AiosTheme) -> Div {
    let c = &theme.colors;
    div()
        .w(px(24.0))
        .h(px(24.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme.metrics.radius_sm)
        .bg(if selected { c.bg_elevated } else { c.bg_subtle })
        .border_1()
        .border_color(if selected {
            c.border_default
        } else {
            c.border_subtle
        })
        .text_color(if selected {
            c.accent_fg_on_selected
        } else {
            c.text_secondary
        })
        .font_family(theme.font_mono)
        .text_size(px(10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label.to_owned())
}

/// Renders a section header row (`11px`, SemiBold, `text.secondary`).
pub fn section_header(title: &str, subtitle: Option<&str>, theme: &AiosTheme) -> Div {
    let c = &theme.colors;
    let mut row = div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(10.0))
        .pt(px(10.0))
        .pb(px(4.0))
        .child(
            div()
                .text_color(c.text_secondary)
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .line_height(px(16.0))
                .child(title.to_owned()),
        );
    if let Some(sub) = subtitle {
        row = row.child(
            div()
                .text_color(c.text_muted)
                .font_family(theme.font_mono)
                .text_size(px(11.0))
                .line_height(px(16.0))
                .child(sub.to_owned()),
        );
    }
    row
}

/// Lightweight Pure Rust Markdown block renderer for `Detail` and `List.Item.Detail`.
pub fn render_markdown_blocks(markdown: &str, theme: &AiosTheme) -> Div {
    let c = &theme.colors;
    let mut container = div().w_full().min_w(px(0.0)).flex().flex_col().gap(px(8.0));

    let mut in_code_fence = false;
    let mut code_lang = String::new();
    let mut code_lines: Vec<String> = Vec::new();
    let mut code_block_idx = 0usize;

    for line in markdown.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if in_code_fence {
                let block =
                    render_code_block(code_block_idx, &code_lang, &code_lines.join("\n"), theme);
                code_block_idx += 1;
                container = container.child(block);
                code_lines.clear();
                code_lang.clear();
                in_code_fence = false;
            } else {
                in_code_fence = true;
                code_lang = rest.trim().to_owned();
            }
            continue;
        }

        if in_code_fence {
            code_lines.push(line.to_owned());
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        if let Some(h1) = trimmed.strip_prefix("# ") {
            container = container.child(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .whitespace_normal()
                    .text_color(c.text_primary)
                    .text_size(px(18.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(px(24.0))
                    .child(soft_wrap_long_tokens(h1)),
            );
        } else if let Some(h2) = trimmed.strip_prefix("## ") {
            container = container.child(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .whitespace_normal()
                    .pt(px(4.0))
                    .text_color(c.text_primary)
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(px(20.0))
                    .child(soft_wrap_long_tokens(h2)),
            );
        } else if let Some(h3) = trimmed.strip_prefix("### ") {
            container = container.child(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .whitespace_normal()
                    .pt(px(2.0))
                    .text_color(c.text_primary)
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(px(20.0))
                    .child(soft_wrap_long_tokens(h3)),
            );
        } else if let Some(quote) = trimmed.strip_prefix("> ") {
            container = container.child(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .whitespace_normal()
                    .px(px(10.0))
                    .py(px(6.0))
                    .border_l_2()
                    .border_color(c.accent_solid)
                    .bg(c.bg_subtle)
                    .rounded_r(theme.metrics.radius_xs)
                    .text_color(c.text_secondary)
                    .text_size(px(12.5))
                    .line_height(px(18.0))
                    .child(soft_wrap_long_tokens(quote)),
            );
        } else if let Some(bullet) = trimmed.strip_prefix("- ") {
            container = container.child(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(8.0))
                    .child(
                        div()
                            .mt(px(7.0))
                            .w(px(4.0))
                            .h(px(4.0))
                            .flex_shrink_0()
                            .rounded(px(9999.0))
                            .bg(c.accent_solid),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .whitespace_normal()
                            .text_color(c.text_primary)
                            .text_size(px(13.0))
                            .line_height(px(20.0))
                            .child(soft_wrap_long_tokens(bullet)),
                    ),
            );
        } else {
            container = container.child(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .whitespace_normal()
                    .text_color(c.text_primary)
                    .text_size(px(13.0))
                    .line_height(px(20.0))
                    .child(soft_wrap_long_tokens(trimmed)),
            );
        }
    }

    if in_code_fence && !code_lines.is_empty() {
        container = container.child(render_code_block(
            code_block_idx,
            &code_lang,
            &code_lines.join("\n"),
            theme,
        ));
    }

    container
}

fn render_code_block(idx: usize, lang: &str, code: &str, theme: &AiosTheme) -> Div {
    let c = &theme.colors;
    let mut card = div()
        .w_full()
        .min_w(px(0.0))
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .p(px(10.0))
        .rounded(theme.metrics.radius_sm)
        .bg(c.bg_subtle)
        .border_1()
        .border_color(c.border_subtle);

    if !lang.is_empty() {
        card = card.child(
            div()
                .text_color(c.text_muted)
                .font_family(theme.font_mono)
                .text_size(px(10.5))
                .font_weight(FontWeight::MEDIUM)
                .child(lang.to_owned()),
        );
    }

    card.child(
        div()
            .id(("aios-code-block-scroll", idx))
            .w_full()
            .min_w(px(0.0))
            .whitespace_normal()
            .text_color(c.text_primary)
            .font_family(theme.font_mono)
            .text_size(px(11.5))
            .line_height(px(17.0))
            .child(soft_wrap_long_tokens(code)),
    )
}

/// Inserts zero-width break opportunities (`U+200B`) inside long unbroken tokens (such as
/// file paths, URLs, or D-Bus reverse-DNS names) so narrow columns wrap cleanly instead of clipping.
#[must_use]
pub fn soft_wrap_long_tokens(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    let mut token = String::new();

    let flush_token = |tok: &str, dst: &mut String| {
        if tok.chars().count() <= 16 {
            dst.push_str(tok);
            return;
        }
        let mut run_len = 0usize;
        for ch in tok.chars() {
            dst.push(ch);
            if matches!(ch, '/' | '\\' | ':' | '?' | '&' | '=' | ';' | '.' | '_') {
                dst.push('\u{200B}');
                run_len = 0;
            } else {
                run_len += 1;
                if run_len >= 20 {
                    dst.push('\u{200B}');
                    run_len = 0;
                }
            }
        }
    };

    for ch in input.chars() {
        if ch.is_whitespace() {
            if !token.is_empty() {
                flush_token(&token, &mut out);
                token.clear();
            }
            out.push(ch);
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        flush_token(&token, &mut out);
    }

    out
}

/// Renders structured key-value metadata rows for `Detail` sidebar or `List.Item.Detail`.
pub fn render_metadata_list(entries: &[MetadataEntry], theme: &AiosTheme) -> Div {
    let c = &theme.colors;
    let mut stack = div()
        .w_full()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap(px(10.0));

    for entry in entries {
        match entry {
            MetadataEntry::Label { title, text } => {
                stack = stack.child(
                    div()
                        .w_full()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_color(c.text_muted)
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child(title.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w(px(0.0))
                                .whitespace_normal()
                                .text_color(c.text_primary)
                                .text_size(px(12.0))
                                .line_height(px(16.0))
                                .child(soft_wrap_long_tokens(text)),
                        ),
                );
            }
            MetadataEntry::Link { title, text, .. } => {
                stack = stack.child(
                    div()
                        .w_full()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_color(c.text_muted)
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child(title.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w(px(0.0))
                                .whitespace_normal()
                                .text_color(c.accent_fg)
                                .font_family(theme.font_mono)
                                .text_size(px(11.5))
                                .line_height(px(16.0))
                                .child(soft_wrap_long_tokens(text)),
                        ),
                );
            }
            MetadataEntry::TagList { title, tags } => {
                let mut tag_row = div().flex().flex_row().flex_wrap().gap(px(6.0));
                for (label, tone) in tags {
                    tag_row = tag_row.child(semantic_tag(label, *tone, theme));
                }
                stack = stack.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_color(c.text_muted)
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child(title.clone()),
                        )
                        .child(tag_row),
                );
            }
            MetadataEntry::Separator => {
                stack = stack.child(div().w_full().h(px(1.0)).bg(c.border_subtle));
            }
        }
    }

    stack
}

/// Mandatory Empty State view explaining why the view is empty and the next keyboard action (R-27).
pub fn render_empty_state(empty: &EmptyView, theme: &AiosTheme) -> Div {
    let c = &theme.colors;
    let badge_text = empty.icon.as_deref().unwrap_or("0");

    let mut box_el = div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .p(px(24.0))
        .child(
            div()
                .w(px(40.0))
                .h(px(40.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.metrics.radius_md)
                .bg(c.bg_subtle)
                .border_1()
                .border_color(c.border_subtle)
                .text_color(c.text_secondary)
                .font_family(theme.font_mono)
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(badge_text.to_owned()),
        )
        .child(
            div()
                .text_color(c.text_primary)
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(empty.title.clone()),
        );

    if let Some(desc) = &empty.description {
        box_el = box_el.child(
            div()
                .max_w(px(440.0))
                .text_color(c.text_secondary)
                .text_size(px(13.0))
                .line_height(px(19.0))
                .child(desc.clone()),
        );
    }

    if let Some(primary) = empty.actions.primary_action() {
        box_el = box_el.child(
            div()
                .mt(px(4.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(12.0))
                .py(px(6.0))
                .rounded(theme.metrics.radius_sm)
                .bg(c.bg_elevated)
                .border_1()
                .border_color(c.border_default)
                .text_color(c.text_primary)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .child(primary.title.clone())
                .child(keycap(
                    primary.shortcut.as_deref().unwrap_or("Enter"),
                    theme,
                )),
        );
    }

    box_el
}

/// Animated indeterminate Signal Copper loading bar (static when `theme.reduced_motion` is true).
pub fn loading_progress_bar(
    id: &'static str,
    width_px: Option<f32>,
    height_px: f32,
    rounded: bool,
    theme: &AiosTheme,
) -> AnyElement {
    let c = &theme.colors;
    let accent = c.accent_solid;
    let radius = theme.metrics.radius_xs;

    let mut track = div()
        .relative()
        .h(px(height_px))
        .overflow_hidden()
        .bg(if rounded {
            c.bg_subtle
        } else {
            c.border_subtle
        });

    if let Some(w) = width_px {
        track = track.w(px(w));
    } else {
        track = track.w_full();
    }
    if rounded {
        track = track
            .rounded(radius)
            .border_1()
            .border_color(c.border_subtle);
    }

    if theme.reduced_motion {
        let mut bar = div().w(relative(0.45)).h_full().bg(accent);
        if rounded {
            bar = bar.rounded(radius);
        }
        return track.child(bar).into_any_element();
    }

    let mut runner = div()
        .absolute()
        .top_0()
        .bottom_0()
        .w(relative(0.36))
        .bg(accent);
    if rounded {
        runner = runner.rounded(radius);
    }

    let animated_runner = runner.with_animation(
        id,
        Animation::new(Duration::from_millis(1100)).repeat_synced(),
        move |el, delta| {
            // Smooth indeterminate sweep from off-left (-36%) across to off-right (100%).
            let left_frac = -0.36 + delta * 1.36;
            el.left(relative(left_frac))
        },
    );

    track.child(animated_runner).into_any_element()
}

/// Mandatory Loading State view with descriptive status copy (R-27).
pub fn render_loading_state(message: &str, theme: &AiosTheme) -> Div {
    let c = &theme.colors;
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .p(px(24.0))
        .child(loading_progress_bar(
            "aios-loading-state-bar",
            Some(180.0),
            5.0,
            true,
            theme,
        ))
        .child(
            div()
                .text_color(c.text_primary)
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Loading view data"),
        )
        .child(
            div()
                .max_w(px(460.0))
                .text_color(c.text_secondary)
                .text_size(px(13.0))
                .line_height(px(19.0))
                .child(message.to_owned()),
        )
}

/// Mandatory Error Isolation view with explicit failure reason and recovery hint (R-27).
pub fn render_error_state(error: &ErrorView, theme: &AiosTheme) -> Div {
    let c = &theme.colors;
    let mut box_el = div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .p(px(24.0))
        .child(semantic_tag("Error Isolated", SemanticTone::Danger, theme))
        .child(
            div()
                .text_color(c.text_primary)
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(error.title.clone()),
        )
        .child(
            div()
                .max_w(px(480.0))
                .text_color(c.text_secondary)
                .text_size(px(13.0))
                .line_height(px(19.0))
                .child(error.message.clone()),
        );

    if let Some(detail) = &error.detail {
        box_el = box_el.child(
            div()
                .max_w(px(520.0))
                .px(px(10.0))
                .py(px(6.0))
                .rounded(theme.metrics.radius_sm)
                .bg(c.bg_subtle)
                .border_1()
                .border_color(c.status_danger)
                .text_color(c.status_danger)
                .font_family(theme.font_mono)
                .text_size(px(11.5))
                .child(detail.clone()),
        );
    }

    if let Some(primary) = error.actions.primary_action() {
        box_el = box_el.child(
            div()
                .mt(px(4.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(12.0))
                .py(px(6.0))
                .rounded(theme.metrics.radius_sm)
                .bg(c.accent_solid)
                .text_color(c.text_on_accent)
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(primary.title.clone()),
        );
    }

    box_el
}

/// Renders an active footer Toast message replacing the left context slot without layout shift.
pub fn render_footer_toast(toast: &ToastMessage, theme: &AiosTheme) -> Div {
    let tone = match toast.style {
        ToastStyle::Success => SemanticTone::Success,
        ToastStyle::Failure => SemanticTone::Danger,
        ToastStyle::Animated => SemanticTone::Warning,
    };
    let tag_label = match toast.style {
        ToastStyle::Success => "OK",
        ToastStyle::Failure => "ERR",
        ToastStyle::Animated => "RUN",
    };

    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .overflow_hidden()
        .child(semantic_tag(tag_label, tone, theme))
        .child(
            div()
                .text_color(theme.colors.text_primary)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .truncate()
                .child(toast.title.clone()),
        );

    if let Some(msg) = &toast.message {
        row = row.child(
            div()
                .text_color(theme.colors.text_muted)
                .text_size(px(11.5))
                .truncate()
                .child(msg.clone()),
        );
    }

    row
}

/// Plane 2 elevation shadow helper (`DESIGN.md` elevation.planes.popover).
#[must_use]
pub fn popover_shadow(theme: &AiosTheme) -> Vec<BoxShadow> {
    let color: Hsla = gpui::rgb_to_hsla(theme.colors.shadow_popover);
    vec![BoxShadow {
        color: Background::from(color),
        offset: point(px(0.0), px(10.0)),
        blur_radius: px(28.0),
        spread_radius: px(0.0),
        inset: false,
    }]
}

/// Plane 1 window shadow helper (`DESIGN.md` elevation.planes.window).
#[must_use]
pub fn window_shadow(theme: &AiosTheme) -> Vec<BoxShadow> {
    let color: Hsla = gpui::rgb_to_hsla(theme.colors.shadow_window);
    vec![BoxShadow {
        color: Background::from(color),
        offset: point(px(0.0), px(18.0)),
        blur_radius: px(48.0),
        spread_radius: px(0.0),
        inset: false,
    }]
}

/// Renders the centered `ConfirmAlert` modal shell on Plane 2 over a translucent scrim.
pub fn render_confirm_alert_card(
    alert: &ConfirmAlert,
    theme: &AiosTheme,
    cancel_btn: AnyElement,
    confirm_btn: AnyElement,
) -> Div {
    let c = &theme.colors;
    div()
        .w(px(400.0))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .p(px(16.0))
        .rounded(theme.metrics.radius_lg)
        .bg(c.bg_elevated)
        .border_1()
        .border_color(c.border_default)
        .shadow(popover_shadow(theme))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_color(c.text_primary)
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(alert.title.clone()),
                )
                .when(alert.destructive, |d| {
                    d.child(semantic_tag("Destructive", SemanticTone::Danger, theme))
                }),
        )
        .child(
            div()
                .text_color(c.text_secondary)
                .text_size(px(13.0))
                .line_height(px(19.0))
                .child(alert.message.clone()),
        )
        .child(
            div()
                .pt(px(4.0))
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .child(cancel_btn)
                .child(confirm_btn),
        )
}
