use crate::{shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Backspace, Enter, Escape, IndentInline, MoveDown, MoveUp},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub(super) fn send_icon(active: bool) -> Icon {
    if active {
        Icon::new(IconName::WindowMaximize)
    } else {
        Icon::new(IconName::ArrowUp)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Options {
    pub choices: [usize; 4],
    pub attachment: bool,
    pub model: Option<super::models::Selection>,
    pub effort: Option<sailry_protocol::Effort>,
    pub references: Vec<super::references::Reference>,
}

impl Shell {
    pub(super) fn composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let key = (self.host, self.session);
        let conversation = &self.conversations[&key];
        let input = conversation.input.clone();
        let value = input.read(cx).value();
        let attachment = conversation.options.attachment;
        let has_references =
            !super::references::active(&input.read(cx).content(), &conversation.options.references)
                .is_empty();
        let active = conversation
            .turns
            .last()
            .is_some_and(|turn| turn.status.active());
        let owner = cx.entity().downgrade();
        let leading = h_flex()
            .gap_1()
            .flex_wrap()
            .child(
                Button::new("composer-attach")
                    .ghost()
                    .rounded_full()
                    .icon(IconName::Plus)
                    .tooltip(tr("composer_attachment"))
                    .accessibility_label(tr("composer_attachment"))
                    .debug_selector(|| "composer-attach".into())
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                        let owner = owner.clone();
                        menu.item(PopupMenuItem::new(tr("composer_attach_example")).on_click(
                            move |_, _, cx| {
                                // Attachment preview uses a label only; no project file is read.
                                _ = owner.update(cx, |this, cx| {
                                    if let Some(conversation) = this.conversations.get_mut(&key) {
                                        conversation.options.attachment = true;
                                        cx.notify();
                                    }
                                });
                            },
                        ))
                    }),
            )
            .child(self.composer_mode(cx))
            .child(self.composer_permission(cx));
        let trailing = h_flex()
            .ml_auto()
            .gap_1()
            .flex_wrap()
            .justify_end()
            .child(self.composer_model(cx))
            .child(self.composer_effort(cx))
            .child(super::usage::context_progress(cx))
            .when(
                active && (!value.trim().is_empty() || attachment || has_references),
                |bar| {
                    bar.child(
                        Button::new("composer-enqueue")
                            .ghost()
                            .rounded_full()
                            .icon(IconName::Inbox)
                            .debug_selector(|| "composer-enqueue".into())
                            .tooltip(tr("queue_add"))
                            .accessibility_label(tr("queue_add"))
                            .on_click(cx.listener(move |shell, _, window, cx| {
                                shell.send_preview(key, window, cx)
                            })),
                    )
                },
            )
            .child(
                crate::theme::send_button(
                    Button::new("send").primary(),
                    !active && value.trim().is_empty() && !attachment && !has_references,
                    cx,
                )
                .icon(send_icon(active))
                .debug_selector(|| "composer-send".into())
                .rounded_full()
                .tooltip(tr(if active { "turn_stop" } else { "send" }))
                .accessibility_label(tr(if active { "turn_stop" } else { "send" }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if active {
                        this.stop_preview(key, cx);
                    } else {
                        this.send_preview(key, window, cx);
                    }
                })),
            );
        let toolbar = h_flex()
            .debug_selector(|| "composer-toolbar".into())
            .gap_1()
            .flex_wrap()
            .child(leading)
            .child(trailing);
        let notices = [
            (!conversation.turns.is_empty()).then(|| self.composer_activity(cx).into_any_element()),
            self.queue_bar(cx),
            self.pending_approvals(cx),
            self.pending_interaction(cx),
        ]
        .into_iter()
        .flatten();
        let content = v_flex()
            .gap_2()
            .when(attachment, |body| {
                body.child(
                    Button::new("composer-remove-attachment")
                        .rounded_full()
                        .ghost()
                        .label(tr("composer_example_file"))
                        .icon(IconName::Close)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.conversations.get_mut(&key).unwrap().options.attachment = false;
                            cx.notify();
                        })),
                )
            })
            .child(
                div()
                    .id("composer-input")
                    .debug_selector(|| "composer-input".into())
                    .capture_action(cx.listener(move |shell, action: &Enter, window, cx| {
                        if action.shift || action.secondary {
                            cx.propagate();
                        } else {
                            shell.reference_action(key, "enter", window, cx);
                        }
                    }))
                    .capture_action(cx.listener(move |shell, _: &Escape, window, cx| {
                        shell.reference_action(key, "escape", window, cx)
                    }))
                    .capture_action(cx.listener(move |shell, _: &MoveUp, window, cx| {
                        shell.reference_action(key, "up", window, cx)
                    }))
                    .capture_action(cx.listener(move |shell, _: &MoveDown, window, cx| {
                        shell.reference_action(key, "down", window, cx)
                    }))
                    .capture_action(cx.listener(move |shell, _: &IndentInline, window, cx| {
                        shell.reference_action(key, "tab", window, cx)
                    }))
                    .capture_action(cx.listener(move |shell, _: &Backspace, window, cx| {
                        shell.reference_action(key, "backspace", window, cx)
                    }))
                    // Kit emits submission, then propagates Enter to the parent.
                    // Consume it here so the platform cannot insert a newline as well.
                    .on_action(|action: &Enter, _, cx| {
                        if action.shift {
                            cx.propagate();
                        }
                    })
                    .child(
                        super::references::textarea(&input, &conversation.options.references)
                            .appearance(false)
                            .bordered(false)
                            .aria_label(tr("composer"))
                            .on_token_click(cx.listener(move |shell, event, window, cx| {
                                shell.activate_reference(key, event, window, cx)
                            })),
                    ),
            )
            .child(toolbar);
        super::layout::composer(
            notices,
            content.into_any_element(),
            Some(self.composer_context(cx).into_any_element()),
            Some(self.composer_references(cx).into_any_element()),
            false,
            cx,
        )
    }

    pub(super) fn composer_choice(
        &self,
        field: usize,
        button: Button,
        options: &'static [&'static str],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let key = (self.host, self.session);
        let selected = self.conversations[&key].options.choices[field];
        let owner = cx.entity().downgrade();
        let project = self.workspace.sessions[&key].owner.project;
        let labels: Vec<_> = if field == 1 {
            self.workspace
                .worktrees
                .iter()
                .filter(|(_, worktree)| worktree.project == project)
                .map(|(&worktree, tree)| {
                    if tree.main {
                        return self.workspace.projects[&project].name.clone();
                    }
                    self.workspace.branch_label(crate::workspace::Owner {
                        worktree,
                        ..self.workspace.sessions[&key].owner
                    })
                })
                .collect()
        } else {
            options
                .iter()
                .enumerate()
                .map(|(index, label)| {
                    if field == 2 && index == 0 {
                        self.workspace.projects[&project].name.clone()
                    } else {
                        tr(label)
                    }
                })
                .collect()
        };
        button
            .custom(crate::theme::subtle_button(cx))
            .label(labels[selected].clone())
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                labels
                    .iter()
                    .enumerate()
                    .fold(menu, |menu, (index, label)| {
                        let owner = owner.clone();
                        let item = PopupMenuItem::new(label.clone());
                        menu.item(
                            item.checked(selected == index)
                                .on_click(move |_, window, cx| {
                                    _ = owner.update(cx, |this, cx| {
                                        if field == 1 {
                                            let mut target = this.workspace.sessions[&key].owner;
                                            if let Some((&worktree, _)) = this
                                                .workspace
                                                .worktrees
                                                .iter()
                                                .filter(|(_, worktree)| {
                                                    worktree.project == target.project
                                                })
                                                .nth(index)
                                            {
                                                target.worktree = worktree;
                                                this.workspace
                                                    .sessions
                                                    .get_mut(&key)
                                                    .unwrap()
                                                    .owner = target;
                                                this.workspace.select(target);
                                                this.files =
                                                    crate::resources::PreviewFiles::new(window, cx);
                                                this.git =
                                                    crate::resources::PreviewGit::new(window, cx);
                                                this.close_resource_panel(cx);
                                            }
                                        }
                                        if let Some(conversation) = this.conversations.get_mut(&key)
                                        {
                                            conversation.options.choices[field] = index;
                                            cx.notify();
                                        }
                                    });
                                }),
                        )
                    })
            })
    }
}
