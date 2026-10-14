//! Shared spacing for titled form dialogs; Kit retains overlay and focus behavior.
use gpui_kit::{
    component::{ActiveTheme as _, dialog::Dialog},
    *,
};

pub(crate) trait DialogStyle {
    fn form_title(self, title: impl IntoElement, cx: &App) -> Self;
}

impl DialogStyle for Dialog {
    fn form_title(self, title: impl IntoElement, cx: &App) -> Self {
        self.bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .p_6()
            .title(div().pb_4().child(title))
    }
}
