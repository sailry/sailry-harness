//! Resource identities use Reicon glyphs and Kit's named colors.
use gpui_kit::{component::*, *};
use sailry_protocol::projects::Appearance;
mod picker;
pub(crate) use picker::{Selection, Trigger, picker};

pub(crate) fn project(value: &Appearance, cx: &App) -> Div {
    // Match the sidebar's text-sm host icons.
    div()
        .size_3p5()
        .flex_shrink_0()
        .debug_selector(|| "project-icon".into())
        .child(
            Icon::default()
                .path(format!("icons/reicon/{}.svg", value.icon))
                .size_3p5()
                .text_color(color(&value.color, cx)),
        )
}

/// An avatar glyph on its color: a white glyph on the named color, or a neutral
/// glyph when the identity has no color. Returns the glyph and the fill.
pub(crate) fn filled(value: Option<&Appearance>, fallback: Icon, cx: &App) -> (Icon, Hsla) {
    let icon = match value {
        Some(value) => Icon::default().path(format!("icons/reicon/{}.svg", value.icon)),
        None => fallback,
    };
    match value.filter(|value| value.color != "none") {
        Some(value) => {
            let name = if value.color == "magenta" {
                "fuchsia"
            } else {
                value.color.as_str()
            };
            let fill = ColorName::try_from(name)
                .unwrap_or(ColorName::Blue)
                .scale(500);
            (icon.text_color(gpui_kit::white()), fill)
        }
        None => (
            icon.text_color(cx.theme().muted_foreground),
            cx.theme().secondary,
        ),
    }
}

pub(crate) fn color(name: &str, cx: &App) -> Hsla {
    if name == "none" {
        return cx.theme().muted_foreground;
    }
    let name = if name == "magenta" { "fuchsia" } else { name };
    ColorName::try_from(name)
        .unwrap_or(ColorName::Blue)
        .scale(if cx.theme().is_dark() { 400 } else { 600 })
}
