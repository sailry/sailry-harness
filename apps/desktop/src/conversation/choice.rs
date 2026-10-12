use crate::tr;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{menu::PopupMenuItem, *},
    *,
};

// Kit's pinned ElementItem adds an 8px outer inset without a row-style hook.
const MENU_INSET: Pixels = px(8.);

pub(super) fn item(
    label: &'static str,
    description: &'static str,
    icon: IconName,
    selected: bool,
) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        h_flex()
            .id(label)
            .aria_label(format!("{}: {}", tr(label), tr(description)))
            .debug_selector(move || format!("{label}-option"))
            .flex_1()
            .min_w_0()
            .whitespace_normal()
            .mx(-MENU_INSET)
            .px(MENU_INSET)
            .py_2()
            .rounded(cx.theme().radius.min(px(8.)))
            .aria_selected(selected)
            .when(selected, |row| row.bg(cx.theme().secondary_active))
            .child(
                h_flex()
                    .debug_selector(move || format!("{label}-content"))
                    .flex_1()
                    .max_w_96()
                    .min_w_0()
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
                    ),
            )
    })
}
