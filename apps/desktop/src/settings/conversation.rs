use super::{
    Workspace,
    group::{Group, Row},
};
use crate::{
    preferences::{self, MessageDisplay},
    tr,
};
use gpui_kit::{
    component::tab::{Tab, TabBar},
    *,
};

impl Workspace {
    pub(super) fn conversation(&self, cx: &mut Context<Self>) -> AnyElement {
        let display = preferences::data(cx).message_display.unwrap_or_default();
        Group::new("settings_messages")
            .child(
                Row::new(
                    "settings_message_display",
                    TabBar::new("message-display")
                        .segmented()
                        .selected_index(usize::from(display == MessageDisplay::Compact))
                        .children(["message_display_detailed", "message_display_compact"].map(
                            |key| Tab::new().debug_selector(move || key.into()).label(tr(key)),
                        ))
                        .on_click(|index, _, cx| {
                            preferences::update(cx, |data| {
                                data.message_display = Some(
                                    [MessageDisplay::Detailed, MessageDisplay::Compact][*index],
                                );
                            });
                        }),
                )
                .description("settings_message_display_description"),
            )
            .into_any_element()
    }
}
