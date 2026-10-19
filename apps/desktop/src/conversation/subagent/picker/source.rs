use super::*;
use crate::conversation::live::View;
use sailry_protocol::SessionId;

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
    pub running: bool,
    pub icon: IconName,
    pub order: u64,
    pub elapsed: Option<SharedString>,
}

#[derive(Clone)]
pub(super) enum Source {
    Preview {
        owner: WeakEntity<Shell>,
        location: Location,
    },
    Activity {
        owner: WeakEntity<View>,
        session: SessionId,
    },
}

impl Source {
    pub(super) fn cache_key(&self) -> String {
        match self {
            Self::Preview { location, .. } => format!(
                "subagent-picker-{}-{}-{}",
                location.session.0, location.session.1, location.turn
            ),
            Self::Activity { session, .. } => format!("live-subagent-activity-{session}"),
        }
    }

    pub(super) fn rows(&self, cx: &App) -> Vec<Row> {
        match self {
            Self::Preview { owner, location } => owner
                .upgrade()
                .map(|owner| owner.read(cx).subagent_rows(*location)),
            Self::Activity { owner, session } => owner
                .upgrade()
                .filter(|owner| owner.read(cx).session() == Some(*session))
                .map(|owner| owner.read(cx).activity_rows()),
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
            Self::Activity { owner, .. } => {
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
            (Self::Activity { owner, .. }, Choice::Live(id)) => {
                _ = owner.update(cx, |view, cx| view.open_child(id, cx));
            }
            _ => {}
        }
    }
}
