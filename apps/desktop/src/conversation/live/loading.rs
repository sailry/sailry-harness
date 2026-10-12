use super::*;
use gpui_kit::component::skeleton::Skeleton;

impl View {
    pub(super) fn loading_history(&self) -> bool {
        self.session.is_some() && self.history.snapshot.is_none() && self.rows.is_empty()
    }

    pub(super) fn history_placeholder(&self) -> impl IntoElement {
        v_flex()
            .id("live-history-loading")
            .debug_selector(|| "live-history-loading".into())
            .aria_label(tr("chat_history_loading"))
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .justify_end()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(super::super::CONTENT_WIDTH))
                    .mx_auto()
                    .px_6()
                    .py_6()
                    .gap_6()
                    .child(
                        v_flex()
                            .self_end()
                            .w_64()
                            .max_w_full()
                            .gap_2()
                            .child(Skeleton::new().h_4().rounded_md())
                            .child(Skeleton::new().w_40().h_4().rounded_md()),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_3()
                            .child(Skeleton::new().w_24().h_4().rounded_md())
                            .child(Skeleton::new().h_4().rounded_md())
                            .child(Skeleton::new().h_4().rounded_md())
                            .child(Skeleton::new().w_48().max_w_full().h_4().rounded_md()),
                    ),
            )
    }
}
