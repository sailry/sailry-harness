//! Retained Kit diffs with owned background parsing for captured patches.
mod parse;
mod view;

pub use parse::{files, patch};
pub use view::{State, rows, surface};

pub fn background(cx: &gpui_kit::App) -> gpui_kit::Hsla {
    use gpui_kit::component::ActiveTheme;

    let theme = cx.theme();
    theme.background.alpha(1.0).blend(theme.group_box)
}
