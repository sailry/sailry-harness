//! Shared menu geometry composed with GPUI Kit surfaces.
use gpui_kit::component::{
    ActiveTheme as _,
    menu::{PopupMenu, PopupMenuLayout},
};
use gpui_kit::{App, px, size};

pub(crate) fn init(cx: &mut App) {
    PopupMenu::set_layout(
        PopupMenuLayout {
            min_width: px(180.),
            padding: cx.theme().spacing_tokens().sm,
            item_gap: px(1.),
            row_padding: size(px(8.), px(6.)),
            icon_gap: px(10.),
            separator_spacing: px(4.),
            separator_thickness: px(1.),
        },
        cx,
    );
}

#[cfg(test)]
pub(crate) mod tests;
