use super::presentation;
use super::*;
use crate::tr;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{InputEvent, TextareaState},
    popover::{Popover, PopoverState},
    scroll::ScrollableElement,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;

impl Shell {
    pub(in crate::conversation) fn queue_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let session = (self.host, self.session);
        let entries = self.conversations[&session].queue.entries.clone();
        let first = entries.first()?;
        let preview = first.label();
        let owner = cx.entity().downgrade();
        Some(
            Popover::new("composer-queue-popover")
                .anchor(Anchor::BottomLeft)
                .p_2()
                .trigger(
                    presentation::trigger("composer-queue", entries.len(), preview, cx)
                        .debug_selector(|| "composer-queue".into())
                        .tooltip(tr("queue_preview")),
                )
                .content(move |_, window, cx| {
                    let dismiss = cx.entity().downgrade();
                    window.use_keyed_state(
                        format!("queue-{}-{}", session.0, session.1),
                        cx,
                        |_, cx| Panel::new(session, entries.clone(), owner.clone(), dismiss, cx),
                    )
                })
                .into_any_element(),
        )
    }
}

struct Panel {
    session: (usize, usize),
    entries: Vec<Entry>,
    owner: WeakEntity<Shell>,
    dismiss: WeakEntity<PopoverState>,
    editing: Option<(Target, Entity<TextareaState>)>,
    allowed: Vec<bool>,
}

impl Panel {
    fn new(
        session: (usize, usize),
        entries: Vec<Entry>,
        owner: WeakEntity<Shell>,
        dismiss: WeakEntity<PopoverState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let allowed = owner
            .upgrade()
            .map(|shell| {
                entries
                    .iter()
                    .map(|entry| shell.read(cx).queue_allowed(session, entry))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(shell) = owner.upgrade() {
            cx.observe(&shell, |panel, shell, cx| {
                let entries = shell
                    .read(cx)
                    .conversations
                    .get(&panel.session)
                    .map(|thread| thread.queue.entries.clone())
                    .unwrap_or_default();
                // Keyed state notifies its render owner; only changed snapshots propagate back.
                let allowed: Vec<_> = entries
                    .iter()
                    .map(|entry| shell.read(cx).queue_allowed(panel.session, entry))
                    .collect();
                if entries == panel.entries && allowed == panel.allowed {
                    return;
                }
                if panel.editing.as_ref().is_some_and(|(target, _)| {
                    !entries
                        .iter()
                        .any(|entry| entry.target(panel.session) == *target)
                }) {
                    panel.editing = None;
                }
                panel.entries = entries;
                panel.allowed = allowed;
                cx.notify();
            })
            .detach();
        }
        Self {
            session,
            entries,
            owner,
            dismiss,
            editing: None,
            allowed,
        }
    }

    fn edit(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.target(self.session) == target)
        else {
            return;
        };
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(tr("composer"))
                .default_value(entry.text.clone())
                .auto_grow(1, 3)
                .submit_on_enter(true)
        });
        let content =
            super::super::references::content(entry.text.clone(), &entry.options.references);
        input.update(cx, |input, cx| input.set_value(content, window, cx));
        cx.subscribe_in(&input, window, |panel, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                panel.save(window, cx);
            }
            cx.notify();
        })
        .detach();
        input.update(cx, |input, cx| input.focus(window, cx));
        self.editing = Some((target, input));
        cx.notify();
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(popover) = self.dismiss.upgrade() {
            popover.read(cx).focus_handle(cx).focus(window, cx);
        }
        self.editing = None;
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((target, input)) = &self.editing else {
            return;
        };
        if input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        let value = input.read(cx).content();
        if self
            .owner
            .update(cx, |shell, cx| shell.edit_queued(*target, value, cx))
            .unwrap_or(false)
        {
            self.cancel_edit(window, cx);
        }
    }

    fn row(&self, entry: &Entry, cx: &mut Context<Self>) -> AnyElement {
        let target = entry.target(self.session);
        let shell = self.owner.clone();
        let drop_owner = shell.clone();
        let drag = Drag {
            target,
            text: entry.label(),
        };
        let allowed = self
            .owner
            .upgrade()
            .is_some_and(|shell| shell.read(cx).queue_allowed(self.session, entry));
        let edit = Button::new(("queue-edit", entry.id))
            .rounded_full()
            .ghost()
            .small()
            .icon(IconName::Replace)
            .debug_selector(move || format!("queue-edit-{}", target.id))
            .tooltip(tr("queue_edit"))
            .accessibility_label(tr("queue_edit"))
            .on_click(cx.listener(move |panel, _, window, cx| {
                cx.stop_propagation();
                panel.edit(target, window, cx);
            }));
        let send = Button::new(("queue-send", entry.id))
            .rounded_full()
            .ghost()
            .small()
            .icon(IconName::ArrowUp)
            .disabled(!allowed)
            .debug_selector(move || format!("queue-send-{}", target.id))
            .tooltip(tr(if allowed {
                "queue_send"
            } else {
                "queue_blocked"
            }))
            .accessibility_label(tr("queue_send"))
            .on_click(cx.listener(move |panel, _, window, cx| {
                cx.stop_propagation();
                _ = panel
                    .owner
                    .update(cx, |shell, cx| shell.send_queued(target, window, cx));
                _ = panel
                    .dismiss
                    .update(cx, |state, cx| state.dismiss(window, cx));
            }));
        let remove = Button::new(("queue-delete", entry.id))
            .rounded_full()
            .ghost()
            .small()
            .icon(IconName::Delete)
            .debug_selector(move || format!("queue-delete-{}", target.id))
            .tooltip(tr("queue_delete"))
            .accessibility_label(tr("queue_delete"))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                _ = shell.update(cx, |shell, cx| shell.remove_queued(target, cx));
            });
        presentation::row(
            entry.id,
            entry.id.to_string().into(),
            entry.label(),
            entry.options.attachment,
            [
                edit.into_any_element(),
                send.into_any_element(),
                remove.into_any_element(),
            ],
            cx,
        )
        .debug_selector(move || format!("queue-row-{}", target.id))
        .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
        .on_drop(move |source: &Drag, _, cx| {
            _ = drop_owner.update(cx, |shell, cx| {
                shell.reorder_queued(source.target, target, cx)
            });
        })
        .into_any_element()
    }
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editing.as_ref().map(|(target, input)| {
            let target = *target;
            let input = input.clone();
            let cancel = Button::new("queue-edit-cancel")
                .rounded_full()
                .small()
                .label(tr("settings_cancel"))
                .debug_selector(|| "queue-edit-cancel".into())
                .on_click(cx.listener(|panel, _, window, cx| panel.cancel_edit(window, cx)));
            let save = Button::new("queue-edit-save")
                .rounded_full()
                .small()
                .primary()
                .label(tr("queue_save"))
                .debug_selector(|| "queue-edit-save".into())
                .disabled(
                    input.read(cx).value().trim().is_empty()
                        && !self.entries.iter().any(|entry| {
                            entry.target(self.session) == target
                                && (entry.options.attachment
                                    || !entry.options.references.is_empty())
                        }),
                )
                .on_click(cx.listener(|panel, _, window, cx| panel.save(window, cx)));
            presentation::editor(
                super::super::references::textarea(
                    &input,
                    &self
                        .entries
                        .iter()
                        .find(|entry| entry.target(self.session) == target)
                        .map(|entry| entry.options.references.clone())
                        .unwrap_or_default(),
                ),
                [cancel.into_any_element(), save.into_any_element()],
            )
        });
        presentation::panel(window)
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("queue_preview")),
            )
            .when_some(editor, |panel, editor| panel.child(editor))
            .when(self.editing.is_none(), |panel| {
                panel.child(
                    v_flex()
                        .id("queue-list")
                        .flex_grow(1.)
                        .p_1()
                        .overflow_y_scrollbar()
                        .children(self.entries.iter().map(|entry| self.row(entry, cx))),
                )
            })
    }
}

#[derive(Clone)]
struct Drag {
    target: Target,
    text: SharedString,
}

impl Render for Drag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        presentation::drag(self.text.clone(), cx)
    }
}
