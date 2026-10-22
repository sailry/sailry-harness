//! Shared conversation visibility and creation-order projections.
use sailry_protocol::{Session, Snapshot};
use std::cmp::Reverse;

/// Active controller-visible sessions in newest-created order for one Node.
pub fn recent(snapshot: &Snapshot) -> Vec<&Session> {
    let mut sessions: Vec<_> = snapshot
        .sessions
        .iter()
        .filter(|session| {
            !session.archived
                && session.delegation.is_none()
                && session
                    .project
                    .is_none_or(|project| snapshot.projects.iter().any(|entry| entry.id == project))
        })
        .collect();
    // Stable sorting preserves authoritative snapshot order for historical receipts
    // that predate the creation sequence.
    sessions.sort_by_key(|session| Reverse(session.creation_order));
    sessions
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{
        Defaults, Delegation, Effort, NodeId, Permission, Project, ProjectId, ProviderId,
        SessionConfig, SessionId, Snapshot, TurnId, WorkMode, WorktreeId, projects,
    };

    fn session(order: u64, project: Option<ProjectId>) -> Session {
        Session {
            id: SessionId::new(),
            creation_order: order,
            archived: false,
            activity: Default::default(),
            project,
            worktree: WorktreeId::new(),
            revision: 1,
            config: SessionConfig {
                features: Default::default(),
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::Default,
                mode: WorkMode::Code,
                options: Default::default(),
                permission: Permission::Ask,
                credential: None,
            },
            roles: Default::default(),
            profile: None,
            fork: None,
            delegation: None,
        }
    }

    fn project(id: ProjectId) -> Project {
        Project {
            id,
            name: "Project".into(),
            path: "/fixture".into(),
            appearance: projects::Appearance::default(),
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            node: NodeId([1; 32]),
            cursor: 0,
            defaults: Defaults {
                revision: 0,
                config: None,
            },
            projects: vec![],
            worktrees: vec![],
            sessions: vec![],
            terminals: vec![],
            terminal_settings_revision: 0,
            power_settings_revision: 0,
            sandbox_settings_revision: 0,
            search_settings_revision: 0,
            media_settings: Default::default(),
            providers: vec![],
            provider_quotas: vec![],
            model_catalog: Default::default(),
            roles: vec![],
            ssh: vec![],
            databases: vec![],
            plugins: vec![],
            notifications: vec![],
            turns: vec![],
        }
    }

    #[test]
    fn orders_creation_and_keeps_standalone() {
        let active = ProjectId::new();
        let removed = ProjectId::new();
        let older = session(1, Some(active));
        let standalone = session(3, None);
        let hidden = session(4, Some(removed));
        let mut archived = session(5, None);
        archived.archived = true;
        let mut delegated = session(6, Some(active));
        delegated.delegation = Some(Box::new(Delegation {
            session: older.id,
            turn: TurnId::new(),
            entry: "delegate".into(),
            index: 0,
            role: None,
        }));
        let mut snapshot = snapshot();
        snapshot.projects = vec![project(active)];
        // Project ordering and click history do not control this input order.
        snapshot.sessions = vec![
            older.clone(),
            archived,
            hidden,
            delegated,
            standalone.clone(),
        ];
        assert_eq!(
            recent(&snapshot)
                .into_iter()
                .map(|session| session.id)
                .collect::<Vec<_>>(),
            [standalone.id, older.id]
        );
    }

    #[test]
    fn keeps_historical_snapshot_order() {
        let first = session(0, None);
        let second = session(0, None);
        let mut snapshot = snapshot();
        snapshot.sessions = vec![second.clone(), first.clone()];
        assert_eq!(
            recent(&snapshot)
                .into_iter()
                .map(|session| session.id)
                .collect::<Vec<_>>(),
            [second.id, first.id]
        );
    }
}
