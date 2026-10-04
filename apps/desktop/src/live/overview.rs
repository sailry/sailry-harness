//! Project landing page, adapted from Sailry Code d9b56405 resource_overview.dart.
use super::*;
use crate::preview::Page;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    scroll::ScrollableElement,
    *,
};

impl Shell {
    pub(crate) fn live_overview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let actions = self
            .live
            .as_ref()
            .and_then(|live| live.project)
            .and_then(|project| self.project_plugins(project, window, cx))
            .map(|registry| {
                registry.update(cx, |registry, cx| {
                    registry
                        .entries(sailry_protocol::plugin::ui::Slot::Project, cx)
                        .into_iter()
                        .map(|entry| {
                            registry.control(
                                entry,
                                crate::plugins::contributions::Form::Project,
                                cx,
                            )
                        })
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default();
        let live = self.live.as_ref().unwrap();
        let Some(project) = live.selected_project() else {
            return div().into_any_element();
        };
        v_flex()
            .id("live-overview")
            .debug_selector(|| "live-overview".into())
            .size_full()
            .overflow_y_scrollbar()
            .p_6()
            .child(
                v_flex().items_center().py_4().child(
                    v_flex()
                        .debug_selector(|| "live-overview-content".into())
                        .w_full()
                        .max_w(px(800.))
                        .items_start()
                        .gap_4()
                        .child(
                            h_flex()
                                .w_full()
                                .gap_3()
                                .child(
                                    div()
                                        .debug_selector(|| "live-overview-name".into())
                                        .min_w_0()
                                        .text_xl()
                                        .font_semibold()
                                        .truncate()
                                        .child(project.name.clone()),
                                )
                                .child(
                                    h_flex()
                                        .debug_selector(|| "live-overview-host".into())
                                        .min_w_0()
                                        .gap_2()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .child(live.name(live.selected)),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .debug_selector(|| "live-overview-path".into())
                                .w_full()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(project.path.clone()),
                        )
                        .child(
                            h_flex()
                                .max_w_full()
                                .flex_wrap()
                                .gap_2()
                                .mt_2()
                                .child(
                                    Button::new("live-new-conversation")
                                        .primary()
                                        .debug_selector(|| "live-new-conversation".into())
                                        .label(tr("chat_new"))
                                        .icon(Page::Conversation.icon())
                                        .disabled(!live.view.connected)
                                        .on_click(cx.listener(|shell, _, window, cx| {
                                            shell.new_live_conversation(window, cx)
                                        })),
                                )
                                .children(actions),
                        )
                        .child(self.project_records(cx)),
                ),
            )
            .into_any_element()
    }
}
