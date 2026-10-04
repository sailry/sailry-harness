//! Activity presentation shares the Client's session summaries and read state.
use super::*;
mod strip;
use sailry_client::activity::{Lane, lane};
use sailry_protocol::{NodeId, Session, Snapshot, projects::Appearance};

impl Shell {
    pub(crate) fn session_project_name(&self, node: NodeId, session: &Session) -> SharedString {
        self.activity_snapshot(node)
            .and_then(|snapshot| {
                snapshot
                    .projects
                    .iter()
                    .find(|project| Some(project.id) == session.project)
            })
            .map(|project| project.name.clone().into())
            .unwrap_or_else(|| tr("activity_no_project"))
    }

    pub(crate) fn activity_snapshot(&self, node: NodeId) -> Option<&Snapshot> {
        self.live
            .as_ref()
            .filter(|live| live.selected == node)
            .and_then(|live| live.view.snapshot.as_ref())
            .into_iter()
            .chain(self.activity.snapshot(node))
            .max_by_key(|snapshot| snapshot.cursor)
    }

    pub(crate) fn active_sessions(&self) -> Vec<(NodeId, Session)> {
        let Some(live) = &self.live else {
            return Vec::new();
        };
        live.hosts
            .keys()
            .flat_map(|node| {
                self.activity_snapshot(*node)
                    .into_iter()
                    .flat_map(|snapshot| &snapshot.sessions)
                    .filter(|session| {
                        session.delegation.is_none()
                            && !session.archived
                            && match lane(session) {
                                Lane::Running | Lane::Waiting => true,
                                Lane::Completed | Lane::Failed => session.activity.attention.unread,
                                Lane::Idle => false,
                            }
                    })
                    .map(|session| (*node, session.clone()))
            })
            .collect()
    }

    pub(crate) fn session_appearance(&self, node: NodeId, session: &Session) -> Appearance {
        let snapshot = self.activity_snapshot(node);
        snapshot
            .and_then(|snapshot| {
                snapshot
                    .projects
                    .iter()
                    .find(|project| Some(project.id) == session.project)
            })
            .map(|project| project.appearance.clone())
            .unwrap_or_default()
    }
}
