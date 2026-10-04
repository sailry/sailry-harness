use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    dialog::Dialog,
};

impl Render for Search {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("live-search")
            .debug_selector(|| "live-search".into())
            .ghost()
            .small()
            .icon(IconName::Search)
            .disabled(self.list.read(cx).delegate().session.is_none())
            .tooltip(tr("chat_search"))
            .accessibility_label(tr("chat_search"))
            .on_click(cx.listener(|search, _, window, cx| search.open(window, cx)))
    }
}

impl Search {
    pub(super) fn dialog(
        &self,
        dialog: Dialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        crate::command_picker::frame(
            dialog.min_h_0(),
            "live-search-panel",
            crate::command_picker::list(&self.list, tr("chat_search_query"), window)
                .search_divider(!self.list.read(cx).delegate().query.is_empty()),
            window,
        )
        .on_close(cx.listener(|search, _, _, cx| search.dismiss(cx)))
    }
}
