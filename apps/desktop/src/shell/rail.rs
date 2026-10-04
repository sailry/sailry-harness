//! Feature selection and independently owned navigation slots.
use super::*;
use gpui_kit::component::scroll::ScrollableElement;
mod features;
mod hosts;
mod more;
mod order;
pub(super) use features::{Destination, Feature};

impl Shell {
    pub(crate) fn rail_popover_offset(window: &Window) -> Point<Pixels> {
        // Pinned Kit's block prepaint canvas follows the trigger, so BottomLeft
        // resolves to trigger.top. Add size_9 and undo the content's bottom_1 gap.
        let height = rems(2.25).to_pixels(window.rem_size());
        let gap = rems(0.25).to_pixels(window.rem_size());
        point(px(RAIL_WIDTH), height + gap)
    }

    pub(super) fn feature_rail(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .debug_selector(|| "shell-feature-rail".into())
            .size_full()
            .items_center()
            .py_2()
            .gap_2()
            .child(
                v_flex()
                    .id("feature-buttons")
                    .flex_1()
                    .min_h_0()
                    .items_center()
                    .gap_2()
                    .overflow_y_scrollbar()
                    .children(
                        self.features(cx)
                            .into_iter()
                            .filter(|feature| feature.pinned(cx))
                            .map(|feature| {
                                let selector = feature.selector();
                                let selected = feature.selected(self, cx);
                                let icon = match feature.destination {
                                    Destination::Page(page) => {
                                        crate::theme::icon(page.key(), feature.icon.clone(), cx)
                                    }
                                    _ => feature.icon.clone().into_any_element(),
                                };
                                div().relative().child(
                                    Button::new(SharedString::from(feature.key.clone()))
                                        .debug_selector(move || selector.clone())
                                        .ghost()
                                        .when(!cx.theme().is_dark(), |button| {
                                            button.custom(crate::theme::subtle_button(cx))
                                        })
                                        .size_9()
                                        .child(icon)
                                        .selected(selected)
                                        .tooltip(feature.label.clone())
                                        .accessibility_label(feature.label.clone())
                                        .on_click(cx.listener(move |shell, _, window, cx| {
                                            feature.open(shell, window, cx)
                                        })),
                                )
                            }),
                    )
                    .child(self.more_features(self.features(cx), window, cx)),
            )
            .child(self.notifications(window, cx))
            .child(self.host_picker(window, cx))
            .child(
                Button::new("feature-search")
                    .ghost()
                    .size_9()
                    .icon(IconName::Search)
                    .debug_selector(|| "sidebar-search".into())
                    .tooltip(tr("search"))
                    .accessibility_label(tr("search"))
                    .on_click(
                        cx.listener(|shell, _, window, cx| shell.search(&Search, window, cx)),
                    ),
            )
            .child(
                Button::new("feature-settings")
                    .debug_selector(|| "sidebar-settings".into())
                    .ghost()
                    .when(!cx.theme().is_dark(), |button| {
                        button.custom(crate::theme::subtle_button(cx))
                    })
                    .size_9()
                    .icon(IconName::Settings)
                    .selected(self.page == Page::Settings)
                    .tooltip(tr("settings"))
                    .accessibility_label(tr("settings"))
                    .on_click(cx.listener(|shell, _, window, cx| {
                        shell.navigate(Page::Settings, window, cx)
                    })),
            )
    }

    pub(super) fn feature_navigation(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match self.page {
            Page::Conversation | Page::Terminal | Page::Host | Page::Project | Page::Settings => {
                Some(self.sidebar(window, cx).into_any_element())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
