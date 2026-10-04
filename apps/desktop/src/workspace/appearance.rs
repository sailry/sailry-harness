//! Resource identities use Reicon glyphs and Kit's named colors.
use gpui_kit::{component::*, *};
use sailry_protocol::projects::Appearance;
mod picker;
pub(crate) use picker::{Selection, picker};

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

pub(crate) fn color(name: &str, cx: &App) -> Hsla {
    if name == "none" {
        return cx.theme().muted_foreground;
    }
    let name = if name == "magenta" { "fuchsia" } else { name };
    ColorName::try_from(name)
        .unwrap_or(ColorName::Blue)
        .scale(if cx.theme().is_dark() { 400 } else { 600 })
}
