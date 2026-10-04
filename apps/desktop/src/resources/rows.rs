use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    component::{ActiveTheme, h_flex},
    *,
};

// Kit b79f4ce ListItem hardcodes its hover background with no override. These
// resource rows keep Kit Tree's navigation/selection and use its theme tokens.
pub(crate) fn row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .min_w_0()
        .rounded(cx.theme().radius)
        .text_sm()
        .text_color(if selected {
            cx.theme().foreground
        } else {
            cx.theme().muted_foreground
        })
        .when(selected, |row| row.bg(cx.theme().accent))
        .hover(|row| row.text_color(cx.theme().foreground))
}
