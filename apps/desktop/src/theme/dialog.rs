//! Shared spacing for titled form dialogs; Kit retains overlay and focus behavior.
use gpui_kit::{component::dialog::Dialog, *};

pub(crate) trait DialogStyle {
    fn form_title(self, title: impl IntoElement) -> Self;
}

impl DialogStyle for Dialog {
    fn form_title(self, title: impl IntoElement) -> Self {
        self.p_6().title(div().pb_4().child(title))
    }
}
