//! `Form` view renderer (`max-width: 580px` centered, persistent labels, 2px focus ring,
//! concrete inline validation errors, and full Tab/Shift+Tab + click focus support).

use aios_core::view::{FormField, FormView, SemanticTone};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ParentElement, Styled, div, prelude::*, px,
};

use crate::components::{checkmark_icon, semantic_tag};
use crate::shell::LauncherWindow;
use crate::theme::AiosTheme;

/// Renders a `FormView` supporting TextField, PasswordField, TextArea, Checkbox,
/// Dropdown, DatePicker, Description, and Separator, with automatic scroll-into-view
/// whenever the focused field changes.
pub fn render_form_view(
    win: &LauncherWindow,
    form: &FormView,
    theme: &AiosTheme,
    cx: &mut Context<LauncherWindow>,
) -> AnyElement {
    let frame = win.state.current();
    let c = &theme.colors;

    // Make each field a direct child of the tracked scroll container so
    // `ScrollHandle::scroll_to_item(focused_form_field)` scrolls it directly into view.
    let mut scroll = div()
        .id("aios-form-scroll")
        .size_full()
        .overflow_y_scroll()
        .track_scroll(&win.form_scroll)
        .flex()
        .flex_col()
        .items_center()
        .gap(px(14.0))
        .py(px(16.0))
        .px(px(16.0));

    if win.take_scroll_sync() {
        win.form_scroll.scroll_to_item(frame.focused_form_field);
    }

    for (idx, field) in form.fields.iter().enumerate() {
        let is_focused = idx == frame.focused_form_field;

        match field {
            FormField::TextField {
                label,
                placeholder,
                value,
                required,
                error,
                ..
            } => {
                scroll = scroll.child(
                    div()
                        .id(("aios-form-field", idx))
                        .w_full()
                        .max_w(px(580.0))
                        .flex_shrink_0()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.state.current_mut().focused_form_field = idx;
                            this.request_scroll_sync();
                            cx.notify();
                        }))
                        .child(render_text_control(
                            label,
                            placeholder.as_deref().unwrap_or(""),
                            value,
                            false,
                            false,
                            *required,
                            error.as_deref(),
                            is_focused,
                            theme,
                        )),
                );
            }
            FormField::PasswordField {
                label,
                placeholder,
                value,
                required,
                error,
                ..
            } => {
                let masked: String = "•".repeat(value.chars().count());
                scroll = scroll.child(
                    div()
                        .id(("aios-form-field", idx))
                        .w_full()
                        .max_w(px(580.0))
                        .flex_shrink_0()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.state.current_mut().focused_form_field = idx;
                            this.request_scroll_sync();
                            cx.notify();
                        }))
                        .child(render_text_control(
                            label,
                            placeholder.as_deref().unwrap_or(""),
                            &masked,
                            false,
                            true,
                            *required,
                            error.as_deref(),
                            is_focused,
                            theme,
                        )),
                );
            }
            FormField::TextArea {
                label,
                placeholder,
                value,
                required,
                error,
                ..
            } => {
                scroll = scroll.child(
                    div()
                        .id(("aios-form-field", idx))
                        .w_full()
                        .max_w(px(580.0))
                        .flex_shrink_0()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.state.current_mut().focused_form_field = idx;
                            this.request_scroll_sync();
                            cx.notify();
                        }))
                        .child(render_text_control(
                            label,
                            placeholder.as_deref().unwrap_or(""),
                            value,
                            true,
                            false,
                            *required,
                            error.as_deref(),
                            is_focused,
                            theme,
                        )),
                );
            }
            FormField::DatePicker {
                label,
                value,
                error,
                ..
            } => {
                scroll = scroll.child(
                    div()
                        .id(("aios-form-field", idx))
                        .w_full()
                        .max_w(px(580.0))
                        .flex_shrink_0()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.state.current_mut().focused_form_field = idx;
                            this.request_scroll_sync();
                            cx.notify();
                        }))
                        .child(render_text_control(
                            label,
                            "YYYY-MM-DD",
                            value,
                            false,
                            true,
                            false,
                            error.as_deref(),
                            is_focused,
                            theme,
                        )),
                );
            }
            FormField::Dropdown {
                label,
                options,
                selected_id,
                error,
                ..
            } => {
                let selected_label = options
                    .iter()
                    .find(|o| o.id == *selected_id)
                    .map(|o| o.title.as_str())
                    .unwrap_or("Select an option");

                let border_color = if error.is_some() {
                    c.status_danger
                } else if is_focused {
                    c.border_focus
                } else {
                    c.border_default
                };

                let mut field_box = div()
                    .w_full()
                    .max_w(px(580.0))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(field_label(label, false, theme))
                    .child(
                        div()
                            .id(("aios-form-dropdown", idx))
                            .relative()
                            .w_full()
                            .h(px(36.0))
                            .px(px(10.0))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .rounded(theme.metrics.radius_sm)
                            .bg(c.bg_elevated)
                            .border_1()
                            .border_color(border_color)
                            .cursor_pointer()
                            .when(is_focused, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .rounded(px(5.0))
                                        .border_1()
                                        .border_color(border_color),
                                )
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.state.current_mut().focused_form_field = idx;
                                this.state.move_horizontal(1);
                                this.request_scroll_sync();
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_color(c.text_primary)
                                    .text_size(px(13.0))
                                    .child(selected_label.to_owned()),
                            )
                            .child(
                                div()
                                    .text_color(c.text_muted)
                                    .font(theme.mono_font())
                                    .text_size(px(11.0))
                                    .child("< / >"),
                            ),
                    );

                if let Some(err) = error {
                    field_box = field_box.child(field_error_line(err, theme));
                }
                scroll = scroll.child(field_box);
            }
            FormField::Checkbox {
                label,
                title,
                checked,
                error,
                ..
            } => {
                let mut field_box = div()
                    .w_full()
                    .max_w(px(580.0))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(field_label(label, false, theme))
                    .child(
                        div()
                            .id(("aios-form-checkbox", idx))
                            .relative()
                            .w_full()
                            .px(px(10.0))
                            .py(px(8.0))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(10.0))
                            .rounded(theme.metrics.radius_sm)
                            .bg(if is_focused {
                                c.bg_selected
                            } else {
                                c.bg_subtle
                            })
                            .border_1()
                            .border_color(if is_focused {
                                c.border_focus
                            } else {
                                c.border_subtle
                            })
                            .cursor_pointer()
                            .when(is_focused, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .rounded(px(5.0))
                                        .border_1()
                                        .border_color(c.border_focus),
                                )
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.state.current_mut().focused_form_field = idx;
                                this.state.insert_text(" ");
                                this.request_scroll_sync();
                                cx.notify();
                            }))
                            // Matches FigCraft node `405/406`: 18x18px, 4px radius, accent.solid fill, vector ✓ in text.onAccent
                            .child(
                                div()
                                    .w(px(18.0))
                                    .h(px(18.0))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(theme.metrics.radius_xs)
                                    .bg(if *checked {
                                        c.accent_solid
                                    } else {
                                        c.bg_elevated
                                    })
                                    .border_1()
                                    .border_color(if *checked {
                                        c.accent_solid
                                    } else {
                                        c.border_strong
                                    })
                                    .when(*checked, |d| d.child(checkmark_icon(c.text_on_accent))),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_color(c.text_primary)
                                    .text_size(px(13.0))
                                    .child(title.clone()),
                            ),
                    );
                if let Some(err) = error {
                    field_box = field_box.child(field_error_line(err, theme));
                }
                scroll = scroll.child(field_box);
            }
            FormField::Description { title, text } => {
                scroll = scroll.child(
                    div()
                        .w_full()
                        .max_w(px(580.0))
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_color(c.text_secondary)
                                .text_size(px(11.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title.clone()),
                        )
                        .child(
                            div()
                                .text_color(c.text_muted)
                                .text_size(px(12.0))
                                .line_height(px(18.0))
                                .child(text.clone()),
                        ),
                );
            }
            FormField::Separator => {
                scroll = scroll.child(
                    div()
                        .w_full()
                        .max_w(px(580.0))
                        .flex_shrink_0()
                        .h(px(1.0))
                        .bg(c.border_subtle),
                );
            }
        }
    }

    scroll.into_any_element()
}

fn field_label(label: &str, required: bool, theme: &AiosTheme) -> gpui::Div {
    let c = &theme.colors;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .child(
            div()
                .text_color(c.text_secondary)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .line_height(px(16.0))
                .child(label.to_owned()),
        )
        .when(required, |d| {
            d.child(
                div()
                    .text_color(c.accent_fg)
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("*"),
            )
        })
}

fn field_error_line(message: &str, theme: &AiosTheme) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(semantic_tag("Error", SemanticTone::Danger, theme))
        .child(
            div()
                .text_color(theme.colors.status_danger)
                .text_size(px(11.5))
                .line_height(px(16.0))
                .child(message.to_owned()),
        )
}

#[allow(clippy::too_many_arguments)]
fn render_text_control(
    label: &str,
    placeholder: &str,
    value: &str,
    multiline: bool,
    monospace: bool,
    required: bool,
    error: Option<&str>,
    is_focused: bool,
    theme: &AiosTheme,
) -> gpui::Div {
    let c = &theme.colors;
    let border_color = if error.is_some() {
        c.status_danger
    } else if is_focused {
        c.border_focus
    } else {
        c.border_default
    };

    let mut input_box = div()
        .relative()
        .w_full()
        .min_h(if multiline { px(76.0) } else { px(36.0) })
        .px(px(10.0))
        .py(px(8.0))
        .flex()
        .flex_row()
        .items_start()
        .rounded(theme.metrics.radius_sm)
        .bg(c.bg_elevated)
        .border_1()
        .border_color(border_color)
        .when(is_focused, |d| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(5.0))
                    .border_1()
                    .border_color(border_color),
            )
        });

    if value.is_empty() {
        input_box = input_box.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .when(is_focused, |d| {
                    d.child(div().w(px(2.0)).h(px(15.0)).mr(px(2.0)).bg(c.accent_solid))
                })
                .child(
                    div()
                        .text_color(c.text_muted)
                        .text_size(px(13.0))
                        .child(placeholder.to_owned()),
                ),
        );
    } else {
        let mut text_div = div()
            .flex_1()
            .text_color(c.text_primary)
            .text_size(px(13.0))
            .line_height(px(18.0));
        if monospace {
            text_div = text_div.font_family(theme.font_mono);
        }
        input_box = input_box
            .child(text_div.child(value.to_owned()))
            .when(is_focused, |d| {
                d.child(div().w(px(2.0)).h(px(16.0)).ml(px(2.0)).bg(c.accent_solid))
            });
    }

    let mut wrapper = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(field_label(label, required, theme))
        .child(input_box);

    if let Some(err) = error {
        wrapper = wrapper.child(field_error_line(err, theme));
    }

    wrapper
}
