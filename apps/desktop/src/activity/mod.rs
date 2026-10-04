//! Shared notifications and resource navigation; packages own activity pages.
mod delivery;
mod icons;
mod notice;
mod sessions;
pub(crate) use icons::indicator;
pub(crate) use sessions::{status, title};
#[cfg(test)]
pub(crate) mod fixture;
mod notifications;
mod observe;
#[cfg(test)]
mod tests;

use crate::{shell::Shell, tr};
use gpui_kit::{component::*, *};
use sailry_client::{
    View,
    activity::{Inbox, Target},
};
use sailry_link::{CancellationToken, EndpointAddr};
use sailry_protocol::NodeId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct State {
    inbox: Inbox,
    observers: BTreeMap<NodeId, Observer>,
    open: bool,
    received: BTreeMap<sailry_client::activity::Id, std::time::Instant>,
}

impl State {
    pub(crate) fn unread_terminals(&self, node: NodeId) -> Vec<sailry_protocol::TerminalId> {
        self.inbox
            .notices()
            .iter()
            .filter(|notice| !notice.read && notice.id.node == node)
            .filter_map(|notice| match notice.id.target {
                Target::Terminal(id) => Some(id),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(crate) fn snapshot(&self, node: NodeId) -> Option<&sailry_protocol::Snapshot> {
        self.observers.get(&node)?.view.snapshot.as_ref()
    }

    pub(crate) fn read_session(&mut self, node: NodeId, session: sailry_protocol::SessionId) {
        self.read_target(node, Target::Session(session));
    }

    fn read_target(&mut self, node: NodeId, target: Target) {
        let ids: Vec<_> = self
            .inbox
            .notices()
            .iter()
            .filter(|notice| !notice.read && notice.id.node == node && notice.id.target == target)
            .map(|notice| notice.id)
            .collect();
        for id in ids {
            self.inbox.mark_read(id);
        }
    }
}

struct Observer {
    address: EndpointAddr,
    view: View,
    stop: CancellationToken,
    _task: Task<()>,
}
impl Drop for Observer {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Shell {
    pub(crate) fn open_activity(
        &mut self,
        node: NodeId,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(snapshot) = self.activity_snapshot(node).cloned() else {
            return;
        };
        let observed = self
            .activity
            .observers
            .get(&node)
            .map(|observer| observer.view.clone());
        self.park_session_panel(window, cx);
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if !live.hosts.contains_key(&node) {
            return;
        }
        if live.selected != node {
            live.select(node, cx);
            if let Some(observed) = observed {
                live.accept_view(observed);
            }
        }
        self.activity.open = false;
        self.activity.read_target(node, target);
        match target {
            Target::Notification(id) => self.open_notification(node, id, window, cx),
            Target::Session(id) => {
                if let Some(session) = snapshot.sessions.iter().find(|s| s.id == id) {
                    if let Some(origin) = &session.delegation {
                        if let Some(parent) =
                            snapshot.sessions.iter().find(|s| s.id == origin.session)
                        {
                            self.reveal_session(parent.clone(), window, cx);
                            if let Some(source) = self.current_chat().cloned() {
                                self.show_live_child(source, session.clone(), window, cx);
                            }
                        }
                    } else {
                        self.reveal_session(session.clone(), window, cx);
                    }
                }
            }
            Target::Terminal(id) => {
                if let Some(info) = snapshot.terminals.iter().find(|t| t.id == id)
                    && let Some(tree) = snapshot
                        .worktrees
                        .iter()
                        .find(|t| Some(t.id) == info.worktree)
                {
                    self.reveal_terminal(tree.project, info, window, cx);
                }
            }
        }
        cx.notify();
    }
}
