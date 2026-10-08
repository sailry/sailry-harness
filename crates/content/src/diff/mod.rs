//! Selectable, virtualized unified diffs shared by tools and Git reviews.
//! Layout adapted from Bezel 4a7505ab, gallery/patterns/diff.rs (MIT).
//! See third_party_licenses/bezel.md. Kit has no dual-number diff control.
mod parse;
mod selection;
mod text;
mod view;

pub use parse::{Kind, Line, additions, is_binary, unified};
pub use view::{State, rows, surface};

pub fn background(cx: &gpui_kit::App) -> gpui_kit::Hsla {
    use gpui_kit::component::ActiveTheme;

    let theme = cx.theme();
    theme.background.alpha(1.0).blend(theme.group_box)
}

pub fn language(path: &str) -> &str {
    std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("text")
}
