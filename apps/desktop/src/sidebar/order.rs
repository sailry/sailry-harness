//! Sidebar ordering uses Kit drag events and the Node's durable session order.
use super::*;
use crate::panes::{Drag, Target};
use gpui_kit::component::{dock::AnyDrag, notification::Notification};
use sailry_protocol::{Command, ErrorCode};

impl Shell {
    fn session_order(
        &self,
        source: Target,
        target: Target,
        after: bool,
        cx: &App,
    ) -> Option<Command> {
        let (Target::Session(node, source_id), Target::Session(destination_node, target_id)) =
            (source, target)
        else {
            return None;
        };
        if node != destination_node || source == target || self.sidebar.order_pending {
            return None;
        }
        let live = self.live.as_ref().filter(|live| live.selected == node)?;
        let snapshot = live.view.snapshot.as_ref()?;
        let source_session = snapshot
            .sessions
            .iter()
            .find(|session| session.id == source_id)?;
        let destination = snapshot
            .sessions
            .iter()
            .find(|session| session.id == target_id)?;
        let project = source_session.project?;
        if destination.project != Some(project)
            || source_session.archived
            || destination.archived
            || crate::preferences::sessions::get(node, source_id, cx).pinned
                != crate::preferences::sessions::get(node, target_id, cx).pinned
        {
            return None;
        }
        let expected: Vec<_> = snapshot
            .sessions
            .iter()
            .filter(|session| session.project == Some(project))
            .map(|session| session.id)
            .collect();
        let splits = self.splits.read(cx);
        let source_members = splits.ordered_members(source, cx);
        let same_group = source_members.contains(&target);
        let members = |target, members: Vec<Target>| {
            let mut ids = members
                .into_iter()
                .filter_map(|member| match member {
                    Target::Session(owner, id) if owner == node && expected.contains(&id) => {
                        Some(id)
                    }
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            if let Target::Session(_, id) = target {
                ids.insert(id);
            }
            ids
        };
        let moving = members(
            source,
            if same_group {
                Vec::new()
            } else {
                source_members
            },
        );
        let anchor = members(
            target,
            if same_group {
                Vec::new()
            } else {
                splits.ordered_members(target, cx)
            },
        );
        let block: Vec<_> = expected
            .iter()
            .copied()
            .filter(|id| moving.contains(id))
            .collect();
        let mut sessions: Vec<_> = expected
            .iter()
            .copied()
            .filter(|id| !moving.contains(id))
            .collect();
        let position = if after {
            sessions.iter().rposition(|id| anchor.contains(id))? + 1
        } else {
            sessions.iter().position(|id| anchor.contains(id))?
        };
        sessions.splice(position..position, block);
        (sessions != expected).then_some(Command::SetSessionOrder {
            project,
            expected,
            sessions,
        })
    }

    pub(crate) fn split_destination(
        &self,
        row: ListItem,
        entry: &tree::Entry,
        cx: &mut Context<Self>,
    ) -> ListItem {
        let target = entry.target;
        let group = entry.group;
        let marker = if entry.folder {
            Row::Group(group.unwrap())
        } else {
            match target {
                Target::Session(_, id) => Row::LiveSession(id),
                Target::Terminal(_, _, id) => Row::LiveTerminal(id),
                Target::Draft => return row,
            }
        };
        let owner = cx.weak_entity();
        let highlight = self
            .sidebar
            .order_drop
            .filter(|(destination, _)| *destination == marker && cx.has_active_drag());
        row.can_drop(move |value, _, cx| {
            let Some(drag) = value
                .downcast_ref::<AnyDrag>()
                .and_then(|drag| drag.value().downcast_ref::<Drag>())
            else {
                return false;
            };
            owner.upgrade().is_some_and(|owner| {
                let shell = owner.read(cx);
                shell
                    .session_order(drag.target, target, false, cx)
                    .is_some()
                    || shell.session_order(drag.target, target, true, cx).is_some()
                    || group.is_some_and(|group| {
                        shell.splits.read(cx).can_append(group, drag.target, cx)
                    })
            })
        })
        .on_drag_move(
            cx.listener(move |shell, event: &DragMoveEvent<AnyDrag>, _, cx| {
                let source = event
                    .drag(cx)
                    .value()
                    .downcast_ref::<Drag>()
                    .map(|drag| drag.target);
                let drop = source.and_then(|source| {
                    let position = event.event.position;
                    let same_group = group.is_some_and(|group| {
                        shell
                            .splits
                            .read(cx)
                            .ordered_members(group, cx)
                            .contains(&source)
                    });
                    let edge = position.y < event.bounds.top() + px(6.)
                        || position.y > event.bounds.bottom() - px(6.);
                    let after = position.y >= event.bounds.center().y;
                    (event.bounds.contains(&position)
                        && (group.is_none() || same_group || edge)
                        && shell.session_order(source, target, after, cx).is_some())
                    .then_some((marker, after))
                });
                if (drop.is_some()
                    || shell
                        .sidebar
                        .order_drop
                        .is_some_and(|(destination, _)| destination == marker))
                    && shell.sidebar.order_drop != drop
                {
                    shell.sidebar.order_drop = drop;
                    cx.notify();
                }
            }),
        )
        .when_some(highlight, |row, (_, after)| {
            row.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .h(px(2.))
                    .bg(cx.theme().drop_target)
                    .when(after, |line| line.bottom_0())
                    .when(!after, |line| line.top_0()),
            )
        })
        .drag_over::<AnyDrag>(move |style, _, _, cx| {
            if group.is_some() && highlight.is_none() {
                style.bg(cx.theme().drop_target)
            } else {
                style
            }
        })
        .on_drop(cx.listener(move |shell, drag: &AnyDrag, window, cx| {
            let Some(item) = drag.value().downcast_ref::<Drag>() else {
                return;
            };
            cx.stop_propagation();
            if let Some((_, after)) = shell
                .sidebar
                .order_drop
                .take()
                .filter(|(destination, _)| *destination == marker)
            {
                if let Some(command) = shell.session_order(item.target, target, after, cx) {
                    shell.save_session_order(command, window, cx);
                }
            } else if let Some(group) = group {
                shell.append_split(group, item, window, cx);
            }
            cx.notify();
        }))
    }

    fn save_session_order(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.live else { return };
        let client = live.client();
        let request = client.prepare(command);
        self.sidebar.order_pending = true;
        let job = cx
            .global::<crate::backend::Services>()
            .runtime
            .spawn(async move { client.execute(request).await });
        cx.spawn_in(window, async move |shell, cx| {
            let result = job.await;
            _ = shell.update_in(cx, |shell, window, cx| {
                shell.sidebar.order_pending = false;
                let error = match result {
                    Ok(Ok(sailry_protocol::Output::SessionOrder(_))) => None,
                    Ok(Err(error)) => Some(match error.code {
                        ErrorCode::RevisionConflict | ErrorCode::NotFound => "session_changed",
                        ErrorCode::Unavailable | ErrorCode::OutcomeUnknown => {
                            "project_outcome_unknown"
                        }
                        _ => "live_request_failed",
                    }),
                    _ => Some("project_outcome_unknown"),
                };
                if let Some(key) = error {
                    crate::feedback::toast(window, tr(key), Notification::error(tr(key)), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}
