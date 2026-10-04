mod picker;
mod view;
pub(in crate::conversation) use picker::{Choice, Row, live as live_picker};

use std::collections::BTreeMap;

use super::{
    transcript::Location,
    turn::{Status, Turn},
};
use crate::{preview::Page, resources::SideResource, shell::Shell};
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Key {
    pub id: usize,
    pub generation: usize,
}

impl Key {
    fn tree_id(self) -> SharedString {
        format!("{}:{}", self.id, self.generation).into()
    }
}

/// Child-turn snapshots from the preview inventory, not running agents.
#[derive(Clone, Debug)]
pub(crate) struct Agent {
    pub key: Key,
    pub parent: Option<Key>,
    pub name: SharedString,
    pub role: SharedString,
    pub order: u64,
    pub turn: Turn,
}

pub(crate) struct Panel {
    pub location: Location,
    pub scroller: Entity<MessageScrollerState>,
    pub expanded: BTreeMap<usize, bool>,
}

impl Shell {
    fn subagent_rows(&self, location: Location) -> Vec<Row> {
        self.transcript_turn(Location {
            child: None,
            ..location
        })
        .into_iter()
        .flat_map(Turn::subagents)
        .map(|agent| Row {
            key: Choice::Preview(agent.key),
            parent: agent.parent.map(Choice::Preview),
            name: agent.name.clone(),
            status: if matches!(agent.turn.status, Status::Running(_)) {
                "subagent_running"
            } else {
                agent.turn.status.label()
            },
            icon: super::transcript::status_icon(agent.turn.status),
            order: agent.order,
        })
        .collect()
    }

    pub(crate) fn open_subagent(&mut self, location: Location, cx: &mut Context<Self>) -> bool {
        if location.child.is_none()
            || location.session != (self.host, self.session)
            || self.page != Page::Conversation
            || self.transcript_turn(location).is_none()
        {
            return false;
        }
        if !matches!(&self.side_resource, Some(SideResource::Subagent(panel)) if panel.location == location)
        {
            let scroller = cx.new(|cx| MessageScrollerState::new(1, cx));
            cx.observe(&scroller, |_, _, cx| cx.notify()).detach();
            self.side_resource = Some(SideResource::Subagent(Panel {
                location,
                scroller,
                expanded: BTreeMap::new(),
            }));
        }
        self.layout.panel_open[0] = true;
        cx.notify();
        true
    }

    pub(super) fn refresh_subagent_panel(
        &mut self,
        session: (usize, usize),
        cx: &mut Context<Self>,
    ) {
        if let Some(SideResource::Subagent(panel)) = &self.side_resource
            && panel.location.session == session
        {
            panel.scroller.update(cx, |state, cx| state.remeasure(cx));
        }
    }
}

pub(in crate::conversation) use view::status_icon;
