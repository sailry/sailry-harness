use crate::tr;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        menu::{PopupMenuItem, PopupMenuLayout},
        *,
    },
    *,
};

pub(super) fn item(
    label: &'static str,
    description: &'static str,
    icon: IconName,
    selected: bool,
) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        let layout = cx.global::<PopupMenuLayout>();
        h_flex()
            .id(label)
            .aria_label(format!("{}: {}", tr(label), tr(description)))
            .debug_selector(move || format!("{label}-option"))
            .flex_1()
            .max_w_96()
            .min_w_0()
            .whitespace_normal()
            .mx(-layout.row_padding.width)
            .my(-layout.row_padding.height)
            .px(layout.row_padding.width)
            .py(layout.row_padding.height)
            .rounded(layout.item_radius(cx.theme()))
            .aria_selected(selected)
            .when(selected, |row| row.bg(cx.theme().secondary_active))
            .gap_2()
            .child(Icon::new(icon.clone()).size_4())
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_sm().line_height(relative(1.25)).child(tr(label)))
                    .child(
                        div()
                            .text_xs()
                            .line_height(relative(1.25))
                            .text_color(cx.theme().muted_foreground)
                            .child(tr(description)),
                    ),
            )
    })
}
