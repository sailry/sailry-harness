use super::*;
use crate::plugins::contributions::Form;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Backspace, Enter, Escape, IndentInline, MoveDown, MoveUp},
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_protocol::plugin::ui::{Align, Slot};

pub(super) mod compact;
pub(super) use compact::Panel;
#[cfg(test)]
mod tests;

impl View {
    pub(super) fn composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.active().is_some();
        let stopping = self
            .active_run()
            .is_some_and(|run| run.status == Status::Stopping);
        let text = !self.input.read(cx).value().trim().is_empty() || self.has_attachments();
        let busy = self.busy();
        let dictating = self.dictation.phase != super::dictation::Phase::Idle;
        // Keep configuration writes from briefly dimming otherwise available controls.
        let saving = self.saving_config() && self.connected() && !self.readonly();
        let send_blocked = !self.connected()
            || (!active
                && (!text
                    || !self.configured()
                    || (self.binding.project.is_some() && self.binding.worktree.is_none())
                    || !self.attachments.sendable()));
        let toolbar = h_flex()
            .debug_selector(|| "composer-toolbar".into())
            .min_h_8()
            .gap_1()
            .flex_wrap()
            .when(!self.sidebar, |bar| {
                bar.child(
                    h_flex()
                        .gap_1()
                        .flex_wrap()
                        .child(
                            Button::new("live-attach")
                                .ghost()
                                .rounded_full()
                                .icon(IconName::Plus)
                                .disabled(self.attachments_blocked())
                                .when(saving, |button| button.text_color(cx.theme().foreground))
                                .tooltip(tr("composer_attachment"))
                                .debug_selector(|| "live-attach".into())
                                .accessibility_label(tr("composer_attachment"))
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.choose_attachments(window, cx)
                                })),
                        )
                        .when(self.compact_composer, |bar| {
                            bar.child(self.composer_settings())
                        })
                        .when(!self.compact_composer, |bar| {
                            bar.children(self.registered_controls(
                                Slot::Composer,
                                Align::Start,
                                Form::Toolbar,
                                cx,
                            ))
                        }),
                )
            })
            .child(
                h_flex()
                    .gap_1()
                    .when(self.sidebar, |bar| bar.w_full().min_w_0())
                    .when(!self.sidebar, |bar| bar.ml_auto().flex_wrap().justify_end())
                    .when(self.sidebar && self.compact_composer, |bar| {
                        bar.child(self.composer_settings())
                    })
                    .when(self.sidebar && !self.compact_composer, |bar| {
                        bar.children(self.registered_controls(
                            Slot::Context,
                            Align::Start,
                            Form::Icons,
                            cx,
                        ))
                        .children(self.registered_controls(
                            Slot::Context,
                            Align::End,
                            Form::Icons,
                            cx,
                        ))
                        .children(self.registered_controls(
                            Slot::Composer,
                            Align::Start,
                            Form::Toolbar,
                            cx,
                        ))
                    })
                    .when(!self.compact_composer, |bar| {
                        bar.children(self.registered_controls(
                            Slot::Composer,
                            Align::End,
                            Form::Toolbar,
                            cx,
                        ))
                    })
                    .when(self.sidebar, |bar| bar.child(div().flex_1()))
                    .child(self.dictation_controls(cx))
                    // An executing turn keeps its Stop action during dictation.
                    .when(active || !dictating, |bar| {
                        bar.child(
                            crate::theme::send_button(
                                Button::new("live-send").primary(),
                                busy || send_blocked || stopping,
                                active,
                                cx,
                            )
                            .flex_shrink_0()
                            .rounded_full()
                            .debug_selector(|| "live-chat-send".into())
                            .when(saving && !send_blocked, |button| {
                                button
                                    .bg(cx.theme().tokens.button_primary.background)
                                    .text_color(cx.theme().button_primary_foreground)
                            })
                            .tooltip(tr(if active { "chat_stop" } else { "send" }))
                            .accessibility_label(tr(if active { "chat_stop" } else { "send" }))
                            .on_click(cx.listener(
                                move |view, _, window, cx| {
                                    if active {
                                        view.stop(window, cx)
                                    } else {
                                        view.send(window, cx)
                                    }
                                },
                            )),
                        )
                    }),
            );
        let input = self.input_field("live-chat-input", cx);
        let context = (!self.sidebar).then(|| self.context_bar(cx));
        let tray = self.queue_tray(cx);
        super::super::layout::composer(
            self.notices(cx),
            v_flex()
                .id("composer-drop")
                .gap_2()
                .on_drag_move(super::attachments::pointer_drag)
                .when(!self.attachments_blocked(), |composer| {
                    composer
                        .drag_over::<ExternalPaths>(|style, _, _, cx| {
                            style.bg(cx.theme().drop_target)
                        })
                        .on_drop(cx.listener(|view, paths: &ExternalPaths, window, cx| {
                            view.attach_paths(paths.paths().to_vec(), window, cx);
                            view.focus(window, cx);
                        }))
                })
                .children(self.attachment_drafts(cx))
                .child(input)
                .child(toolbar)
                .into_any_element(),
            tray,
            context,
            Some(self.reference_picker(cx)),
            self.sidebar,
            cx,
        )
    }

    pub(super) fn input_field(
        &self,
        selector: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(selector)
            .debug_selector(move || selector.into())
            .capture_action(cx.listener(View::paste_attachments))
            .capture_action(cx.listener(|view, action: &Enter, window, cx| {
                if action.shift || action.secondary {
                    cx.propagate();
                } else {
                    view.reference_action("enter", window, cx);
                }
            }))
            .capture_action(cx.listener(|view, _: &Escape, window, cx| {
                view.reference_action("escape", window, cx)
            }))
            .capture_action(
                cx.listener(|view, _: &MoveUp, window, cx| view.reference_action("up", window, cx)),
            )
            .capture_action(cx.listener(|view, _: &MoveDown, window, cx| {
                view.reference_action("down", window, cx)
            }))
            .capture_action(cx.listener(|view, _: &IndentInline, window, cx| {
                view.reference_action("tab", window, cx)
            }))
            .capture_action(cx.listener(|view, _: &Backspace, window, cx| {
                view.reference_action("backspace", window, cx)
            }))
            .on_action(|action: &Enter, _, cx| {
                if action.shift {
                    cx.propagate();
                }
            })
            .child(
                super::references::inline::textarea(&self.input, &self.references.selected)
                    .appearance(false)
                    .bordered(false)
                    .aria_label(tr("composer"))
                    .on_token_click(cx.listener(View::activate_reference)),
            )
    }

    pub(super) fn notices(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        vec![self.composer_activity(cx)]
    }
    pub(super) fn welcome(&self, with_composer: bool, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        super::super::welcome::content(
            std::rc::Rc::new(move |prompt, window, cx| {
                let _ = owner.update(cx, |view, cx| {
                    view.input.update(cx, |input, cx| {
                        input.set_value(prompt, window, cx);
                        input.focus(window, cx);
                    })
                });
            }),
            if with_composer {
                self.composer(cx)
            } else {
                div().into_any_element()
            },
            cx,
        )
    }
}
