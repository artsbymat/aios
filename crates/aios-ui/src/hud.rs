//! Separate non-interactive Wayland LayerShell HUD confirmation pill (`namespace: "aios-hud"`).

use std::time::Duration;

use gpui::{
    App, Bounds, Context, FontWeight, LinuxWindowBackground, Render, Size, Window, WindowBounds,
    WindowKind, WindowOptions, div,
    layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions},
    point,
    prelude::*,
    px,
};

use crate::components::{aios_brand_mark, popover_shadow};
use crate::theme::AiosTheme;

struct HudPillView {
    title: String,
}

impl Render for HudPillView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<AiosTheme>();
        let c = &theme.colors;

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p(px(6.0))
            .child(
                div()
                    .w_full()
                    .h_full()
                    .px(px(14.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .gap(px(10.0))
                    .rounded(theme.metrics.radius_lg)
                    .bg(c.bg_elevated)
                    .border_1()
                    .border_color(c.border_default)
                    .shadow(popover_shadow(theme))
                    .child(aios_brand_mark(theme))
                    .child(
                        div()
                            .text_color(c.text_primary)
                            .font(theme.sans_font())
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(self.title.clone()),
                    ),
            )
    }
}

/// Opens a brief, non-interactive `aios-hud` layer-shell overlay pill at bottom-center
/// and dismisses it automatically after `1.6s`.
pub fn show_hud_pill(title: String, cx: &mut App) {
    let bounds = Bounds {
        origin: point(px(0.0), px(0.0)),
        size: Size::new(px(280.0), px(52.0)),
    };

    let layer_options = WindowOptions::new()
        .titlebar(None)
        .window_bounds(Some(WindowBounds::Windowed(bounds)))
        .app_id(Some("aios-hud".to_string()))
        .kind(WindowKind::LayerShell(LayerShellOptions {
            namespace: "aios-hud".to_string(),
            layer: Layer::Overlay,
            anchor: Anchor::BOTTOM,
            margin: Some((px(0.0), px(0.0), px(64.0), px(0.0))),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }))
        .linux_window_background(LinuxWindowBackground::Transparent);

    let title_for_view = title.clone();
    let opened = cx
        .open_window(layer_options, |_, cx| {
            cx.new(|_| HudPillView {
                title: title_for_view,
            })
        })
        .or_else(|err| {
            tracing::debug!(error = %err, "LayerShell unavailable for HUD; falling back to PopUp");
            let fallback = WindowOptions::new()
                .titlebar(None)
                .window_bounds(Some(WindowBounds::Windowed(bounds)))
                .app_id(Some("aios-hud".to_string()))
                .kind(WindowKind::PopUp)
                .linux_window_background(LinuxWindowBackground::Transparent);
            cx.open_window(fallback, |_, cx| cx.new(|_| HudPillView { title }))
        });

    if let Ok(handle) = opened {
        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1_600))
                .await;
            let _ = handle.update(cx, |_, window, _| {
                window.remove_window();
            });
        })
        .detach();
    }
}
