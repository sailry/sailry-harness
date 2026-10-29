//! Shared spacing for titled form dialogs; Kit retains overlay and focus behavior.
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{ActiveTheme as _, dialog::Dialog},
    *,
};

pub(crate) trait DialogStyle {
    fn floating_surface(self, cx: &App) -> Self;
    fn form_title(self, title: impl IntoElement, cx: &App) -> Self;
}

impl DialogStyle for Dialog {
    fn floating_surface(self, cx: &App) -> Self {
        // Kit's default dialog fill follows the possibly translucent window background,
        // and the shared popover token keeps a slight translucency for menus. A dialog
        // holds forms and long text, so content beneath must not show through it.
        self.bg(cx.theme().popover.alpha(1.))
            .text_color(cx.theme().popover_foreground)
            .when(!cx.theme().is_dark(), |dialog| dialog.border_0())
    }

    fn form_title(self, title: impl IntoElement, cx: &App) -> Self {
        self.floating_surface(cx)
            .p_6()
            .title(div().pb_4().child(title))
    }
}
