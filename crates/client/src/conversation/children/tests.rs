use super::*;
use sailry_protocol::{
    Defaults, Delegation, NodeId, ProviderId, SessionConfig, Worktree,
    conversation::{Run, RunKind},
};
use serde_json::json;

fn child(parent: SessionId, session: SessionId, status: Status, sequence: u64) -> Child {
    Child {
        run: Run {
            worktree: WorktreeId::new(),
            turn: TurnId::new(),
            kind: RunKind::Task,
            session,
            sequence,
            revision: 1,
            status,
            error: None,
            started_ms: None,
            finished_ms: None,
            origin: None,
            continuation: None,
            notice: false,
            notices: vec![],
            input: None,
        },
        origin: Delegation {
            session: parent,
            turn: TurnId::new(),
            entry: "spawn".into(),
            index: 0,
            role: None,
        },
        name: None,
    }
}

fn page(session: SessionId, children: Vec<Child>) -> Page {
    Page {
        session,
        revision: 1,
        next_before: None,
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children,
        background: vec![],
        entries: vec![],
        runs: vec![],
    }
}

#[test]
fn prefers_active_then_claimed_then_latest() {
    let parent = SessionId::new();
    let session = SessionId::new();
    let mut children: Vec<_> = [
        (Status::Completed, 2),
        (Status::Running, 4),
        (Status::Queued, 5),
    ]
    .map(|(status, sequence)| child(parent, session, status, sequence))
    .into();
    assert_eq!(selected(&children, session).unwrap().run.sequence, 4);
    children.reverse();
    assert_eq!(selected(&children, session).unwrap().run.sequence, 4);
    children.push(child(parent, session, Status::Stopping, 6));
    assert_eq!(selected(&children, session).unwrap().run.sequence, 6);
    children.retain(|child| !active(child.run.status));
    assert_eq!(selected(&children, session).unwrap().run.sequence, 2);
    children.push(child(parent, session, Status::Continued, 4));
    assert_eq!(selected(&children, session).unwrap().run.sequence, 4);
    children.push(child(parent, session, Status::Cancelled, 6));
    assert_eq!(selected(&children, session).unwrap().run.sequence, 6);
    children.retain(|child| child.run.status == Status::Queued);
    children.push(child(parent, session, Status::Queued, 7));
    assert_eq!(selected(&children, session).unwrap().run.sequence, 7);
}

#[test]
fn keeps_source_associations_and_siblings() {
    let parent = SessionId::new();
    let first = child(parent, SessionId::new(), Status::Running, 1);
    let mut completed = first.clone();
    completed.run.status = Status::Completed;
    completed.run.sequence = 2;
    let mut queued = first.clone();
    queued.run.status = Status::Queued;
    queued.run.sequence = 3;
    let mut sibling = first.clone();
    sibling.run.session = SessionId::new();
    sibling.run.sequence = 4;
    let mut other = first.clone();
    other.origin.entry = "another-spawn".into();
    other.run.sequence = 10;
    let page = page(
        parent,
        vec![first.clone(), completed, queued, sibling, other],
    );
    let references = associated(&page, first.origin.turn, "spawn", 0);
    assert_eq!(
        references.iter().map(|item| item.index).collect::<Vec<_>>(),
        [0, 3]
    );
    let other = associated(&page, first.origin.turn, "another-spawn", 0);
    assert_eq!(other[0].index, 4);
    assert!(associated(&page, first.origin.turn, "spawn", 1).is_empty());
    let view = project(&page);
    assert_eq!(view.sessions[&first.run.session].index, 4);
    assert_eq!(running(&page.children, Some(parent)).len(), 2);
    assert!(running(&page.children, None).is_empty());
}

fn snapshot(child: &Child, project: Option<ProjectId>) -> Snapshot {
    let config: SessionConfig = serde_json::from_value(json!({
        "provider":ProviderId::new(),"model":"fixture","mode":"code", "permission":"ask", "options":{},
    })).unwrap();
    let parent = Session {
        id: child.origin.session,
        creation_order: 0,
        archived: false,
        activity: Default::default(),
        project,
        worktree: WorktreeId::new(),
        revision: 1,
        config,
        roles: Default::default(),
        profile: None,
        fork: None,
        delegation: None,
    };
    let mut session = parent.clone();
    session.id = child.run.session;
    session.worktree = child.run.worktree;
    session.delegation = Some(Box::new(child.origin.clone()));
    Snapshot {
        node: NodeId([1; 32]),
        cursor: 1,
        defaults: Defaults {
            revision: 0,
            config: None,
        },
        projects: vec![],
        worktrees: vec![Worktree {
            id: child.run.worktree,
            project,
            path: "/fixture/child".into(),
            main: false,
        }],
        sessions: vec![parent, session],
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
fn requires_registered_delegation_ownership() {
    let child = child(SessionId::new(), SessionId::new(), Status::Running, 1);
    for project in [Some(ProjectId::new()), None] {
        let snapshot = snapshot(&child, project);
        let key = target_key(&child);
        assert_eq!(
            session(&snapshot, project, &child).unwrap().id,
            child.run.session
        );
        assert_eq!(targets(&snapshot).get(&key), Some(&child.run.session));
        for change in [
            "project",
            "worktree",
            "delegation",
            "parent",
            "role",
            "tree",
            "tree_project",
        ] {
            let mut changed = snapshot.clone();
            let selected = &mut changed.sessions[1];
            match change {
                "project" => selected.project = Some(ProjectId::new()),
                "worktree" => selected.worktree = WorktreeId::new(),
                "delegation" => selected.delegation = None,
                "parent" => selected.delegation.as_mut().unwrap().session = SessionId::new(),
                "role" => selected.delegation.as_mut().unwrap().role = Some(RoleId::new()),
                "tree" => changed.worktrees.clear(),
                "tree_project" => changed.worktrees[0].project = Some(ProjectId::new()),
                _ => unreachable!(),
            }
            assert!(session(&changed, project, &child).is_none(), "{change}");
            assert!(!targets(&changed).contains_key(&key), "{change}");
        }
    }
}
