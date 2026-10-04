use super::*;
use crate::conversation::live::View;
use sailry_protocol::{SessionId, TurnId};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::conversation) enum Choice {
    Preview(Key),
    Live(SessionId),
}

impl Choice {
    pub(super) fn tree_id(self) -> SharedString {
        match self {
            Self::Preview(key) => key.tree_id(),
            Self::Live(id) => id.to_string().into(),
        }
    }

    pub(super) fn debug_id(self) -> String {
        match self {
            Self::Preview(key) => key.id.to_string(),
            Self::Live(id) => id.to_string(),
        }
    }
}

#[derive(Clone)]
pub(in crate::conversation) struct Row {
    pub key: Choice,
    pub parent: Option<Choice>,
    pub name: SharedString,
    pub status: &'static str,
    pub icon: IconName,
    pub order: u64,
}

#[derive(Clone)]
pub(super) enum Source {
    Preview {
        owner: WeakEntity<Shell>,
        location: Location,
    },
    Live {
        owner: WeakEntity<View>,
        turn: TurnId,
    },
}

impl Source {
    pub(super) fn cache_key(&self) -> String {
        match self {
            Self::Preview { location, .. } => format!(
                "subagent-picker-{}-{}-{}",
                location.session.0, location.session.1, location.turn
            ),
            Self::Live { turn, .. } => format!("live-subagent-picker-{turn}"),
        }
    }

    pub(super) fn rows(&self, cx: &App) -> Vec<Row> {
        match self {
            Self::Preview { owner, location } => owner
                .upgrade()
                .map(|owner| owner.read(cx).subagent_rows(*location)),
            Self::Live { owner, turn } => owner
                .upgrade()
                .map(|owner| owner.read(cx).child_rows(*turn)),
        }
        .unwrap_or_default()
    }

    pub(super) fn observe(&self, cx: &mut Context<Picker>) {
        match self {
            Self::Preview { owner, .. } => {
                if let Some(owner) = owner.upgrade() {
                    cx.observe(&owner, |picker, _, cx| picker.refresh(cx))
                        .detach();
                }
            }
            Self::Live { owner, .. } => {
                if let Some(owner) = owner.upgrade() {
                    cx.observe(&owner, |picker, _, cx| picker.refresh(cx))
                        .detach();
                }
            }
        }
    }

    pub(super) fn open(&self, choice: Choice, cx: &mut App) {
        if !self.rows(cx).iter().any(|row| row.key == choice) {
            return;
        }
        match (self, choice) {
            (Self::Preview { owner, location }, Choice::Preview(key)) => {
                _ = owner.update(cx, |shell, cx| {
                    shell.open_subagent(
                        Location {
                            child: Some(key),
                            ..*location
                        },
                        cx,
                    )
                });
            }
            (Self::Live { owner, .. }, Choice::Live(id)) => {
                _ = owner.update(cx, |view, cx| view.open_child(id, cx));
            }
            _ => {}
        }
    }
}
