//! Session management uses the same revision-bound commands as conversation menus.
use super::*;
use crate::{
    settings::{entry::Entry, group::Group},
    theme::DialogStyle as _,
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    scroll::ScrollableElement,
};
use sailry_protocol::{Session, SessionRevision};

pub(super) fn revisions(sessions: &[Session], include_archived: bool) -> Vec<SessionRevision> {
    sessions
        .iter()
        .filter(|session| session.delegation.is_none() && (include_archived || !session.archived))
        .map(|session| SessionRevision {
            session: session.id,
            revision: session.revision,
        })
        .collect()
}

pub(super) fn confirm(
    owner: Entity<Panel>,
    expected: Vec<SessionRevision>,
    window: &mut Window,
    cx: &mut App,
) {
    if expected.is_empty() {
        return;
    }
    removal(owner, expected, None, window, cx);
}

fn removal(
    owner: Entity<Panel>,
    expected: Vec<SessionRevision>,
    title: Option<SharedString>,
    window: &mut Window,
    cx: &mut App,
) {
    let node = owner.read(cx).binding.label.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let submit = owner.clone();
        let expected = expected.clone();
        let all = title.is_none();
        dialog
            .form_title(title.clone().unwrap_or_else(|| tr("backup_delete_all")), cx)
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .child(v_flex().gap_2().child(node.clone()).child(tr(if all {
                "backup_delete_all_consequence"
            } else {
                "backup_delete_session_consequence"
            })))
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("backup-session-cancel")
                            .label(tr("settings_cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("backup-session-delete")
                            .danger()
                            .label(tr(if all {
                                "backup_delete_all_action"
                            } else {
                                "backup_delete"
                            }))
                            .disabled(owner.read(cx).locked())
                            .on_click(move |_, window, cx| {
                                if submit.update(cx, |panel, cx| {
                                    panel.submit(
                                        Command::RemoveSessions {
                                            expected: expected.clone(),
                                        },
                                        window,
                                        cx,
                                    )
                                }) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
    });
}

struct Archives {
    owner: Entity<Panel>,
}

pub(super) fn open(owner: Entity<Panel>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| {
        cx.observe_in(&owner, window, |_, _, _, cx| cx.notify())
            .detach();
        Archives { owner }
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        dialog
            .form_title(tr("backup_archived_sessions"), cx)
            .w((window.viewport_size().width - px(48.)).min(px(640.)))
            .child(view.clone())
    });
}

impl Render for Archives {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = self.owner.read(cx);
        let mut rows = Group::new("backup_archived_sessions")
            .heading(false)
            .card(false)
            .empty(IconName::Inbox, "backup_no_archived");
        for session in panel
            .sessions
            .iter()
            .filter(|session| session.archived && session.delegation.is_none())
        {
            let title = crate::activity::title(session);
            let restore = self.owner.clone();
            let delete = self.owner.clone();
            let id = session.id;
            let revision = session.revision;
            let name = title.clone();
            rows = rows.child(
                Entry::new(format!("backup-session-{id}"), title)
                    .action(
                        Button::new(SharedString::from(format!("backup-unarchive-{id}")))
                            .label(tr("backup_unarchive"))
                            .disabled(panel.locked())
                            .on_click(move |_, window, cx| {
                                restore.update(cx, |panel, cx| {
                                    panel.submit(
                                        Command::SetSessionArchived {
                                            session: id,
                                            expected_revision: revision,
                                            archived: false,
                                        },
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .action(
                        Button::new(SharedString::from(format!("backup-delete-{id}")))
                            .danger()
                            .label(tr("backup_delete"))
                            .disabled(panel.locked())
                            .on_click(move |_, window, cx| {
                                removal(
                                    delete.clone(),
                                    vec![SessionRevision {
                                        session: id,
                                        revision,
                                    }],
                                    Some(name.clone()),
                                    window,
                                    cx,
                                )
                            }),
                    ),
            );
        }
        div()
            .child(rows)
            .max_h((window.viewport_size().height - px(240.)).max(px(180.)))
            .overflow_y_scrollbar()
    }
}

#[cfg(test)]
mod tests;
