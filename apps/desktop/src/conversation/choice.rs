use crate::tr;
use gpui_kit::{
    component::{menu::PopupMenuItem, *},
    *,
};

pub(super) fn item(
    label: &'static str,
    description: &'static str,
    icon: IconName,
    selected: bool,
) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        v_flex()
            .id(label)
            .aria_label(format!("{}: {}", tr(label), tr(description)))
            .debug_selector(move || format!("{label}-option"))
            .flex_1()
            .min_w_0()
            .whitespace_normal()
            .gap_0p5()
            .child(div().text_sm().child(tr(label)))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr(description)),
            )
    })
    .icon(icon)
    .checked(selected)
}
