use super::*;
use core::prelude::v1::test;
use sailry_protocol::{
    Delegation, Permission, ProviderId, SessionConfig, SessionId, TurnId, WorkMode, WorktreeId,
};

fn session(archived: bool) -> Session {
    Session {
        id: SessionId::new(),
        creation_order: 0,
        archived,
        activity: Default::default(),
        project: None,
        worktree: WorktreeId::new(),
        revision: 7,
        config: SessionConfig {
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Default::default(),
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

#[test]
fn captures_root_revisions_for_bulk_actions() {
    let live = session(false);
    let archived = session(true);
    let mut child = session(false);
    child.delegation = Some(Box::new(Delegation {
        session: live.id,
        turn: TurnId::new(),
        entry: "fixture".into(),
        index: 0,
        role: None,
    }));
    let sessions = [live.clone(), archived.clone(), child];
    assert_eq!(
        revisions(&sessions, false),
        vec![SessionRevision {
            session: live.id,
            revision: 7
        }]
    );
    assert_eq!(
        revisions(&sessions, true),
        vec![
            SessionRevision {
                session: live.id,
                revision: 7
            },
            SessionRevision {
                session: archived.id,
                revision: 7
            }
        ]
    );
    assert_eq!(sessions[0], live);
    assert_eq!(sessions[1], archived);
}
