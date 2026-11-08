use super::*;
use gpui_kit::TestAppContext;

#[gpui_kit::test]
fn appends_and_persists_order(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let node = NodeId([1; 32]);
    let other = NodeId([2; 32]);
    let first = SessionId::new();
    let second = SessionId::new();
    let terminal: TerminalId = first.to_string().parse().unwrap();
    cx.update(|cx| {
        cx.set_global(Preferences::open(path.clone()));
        for (node, session) in [(node, first), (node, second), (other, first)] {
            set(
                node,
                session,
                State {
                    pinned: true,
                    order: None,
                },
                cx,
            );
        }
        set(
            node,
            terminal,
            State {
                pinned: true,
                order: None,
            },
            cx,
        );
        cx.set_global(Preferences::open(path));
        assert_eq!(get(node, first, cx).order, Some(0));
        assert_eq!(get(node, second, cx).order, Some(1));
        assert_eq!(get(other, first, cx).order, Some(0));
        assert_eq!(get(node, terminal, cx).order, Some(2));
        assert_eq!(get(other, terminal, cx), State::default());
        set(node, first, State::default(), cx);
        assert_eq!(get(node, first, cx), State::default());
        set(
            node,
            first,
            State {
                pinned: true,
                order: None,
            },
            cx,
        );
        assert_eq!(get(node, first, cx).order, Some(3));
        set(node, terminal, State::default(), cx);
        assert_eq!(get(node, terminal, cx), State::default());
        assert_eq!(get(node, first, cx).order, Some(3));
    });
}

#[gpui_kit::test]
fn preserves_legacy_pins(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let node = NodeId([1; 32]);
    let legacy = SessionId::new();
    let appended = SessionId::new();
    let mut data = serde_json::to_value(Data::default()).unwrap();
    data.as_object_mut().unwrap().remove("pins");
    data["sessions"] = serde_json::json!({key(node, legacy): {"pinned": true}});
    std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    cx.update(|cx| {
        cx.set_global(Preferences::open(path));
        assert_eq!(
            get(node, legacy, cx),
            State {
                pinned: true,
                order: None
            }
        );
        set(
            node,
            appended,
            State {
                pinned: true,
                order: None,
            },
            cx,
        );
        assert_eq!(get(node, legacy, cx).order, None);
        assert_eq!(get(node, appended, cx).order, Some(0));
    });
}

#[gpui_kit::test]
fn reconciles_only_authoritative_host_resources(cx: &mut TestAppContext) {
    use sailry_protocol::{Defaults, Snapshot};
    let node = NodeId([1; 32]);
    let other = NodeId([2; 32]);
    let present = SessionId::new();
    let archived = SessionId::new();
    let removed = SessionId::new();
    let running = TerminalId::new();
    let closed = TerminalId::new();
    let deleted = TerminalId::new();
    let terminals = [(running, Status::Running), (closed, Status::Closed)].map(|(id, status)| {
        sailry_protocol::terminal::Info {
            id,
            worktree: Some(sailry_protocol::WorktreeId::new()),
            ssh: None,
            tool: None,
            access: None,
            status,
            activity: None,
            title: None,
            directory: None,
            owner: Some(node),
            revision: 1,
        }
    });
    let sessions = [(present, false), (archived, true)].map(|(id, archived)| {
        serde_json::from_value(serde_json::json!({
            "id": id, "archived": archived, "activity": {
                "title": "Fixture", "attention": {"revision": 0, "unread": false}, "queued": 0
            },
            "project": null, "worktree": sailry_protocol::WorktreeId::new(), "revision": 1,
            "config": {
                "provider": sailry_protocol::ProviderId::new(), "model": "fixture",
                "mode": "code", "permission": "ask"
            },
            "roles": {"profiles": [], "providers": []}
        }))
        .unwrap()
    });
    cx.update(|cx| {
        for (owner, id) in [
            (node, present),
            (node, archived),
            (node, removed),
            (other, removed),
        ] {
            set(
                owner,
                id,
                State {
                    pinned: true,
                    order: None,
                },
                cx,
            );
        }
        for (owner, id) in [
            (node, running),
            (node, closed),
            (node, deleted),
            (other, deleted),
        ] {
            set(
                owner,
                id,
                State {
                    pinned: true,
                    order: None,
                },
                cx,
            );
        }
        let mut view = sailry_client::View::default();
        reconcile(&view, cx);
        assert!(get(node, removed, cx).pinned);
        view.snapshot = Some(Snapshot {
            node,
            cursor: 1,
            defaults: Defaults {
                revision: 0,
                config: None,
            },
            sessions: sessions.into(),
            projects: vec![],
            worktrees: vec![],
            terminals: terminals.into(),
            terminal_settings_revision: 0,
            power_settings_revision: 0,
            sandbox_settings_revision: 0,
            search_settings_revision: 0,
            team_settings_revision: 0,
            compaction_settings_revision: 0,
            media_settings: Default::default(),
            providers: vec![],
            provider_quotas: vec![],
            model_catalog: Default::default(),
            roles: vec![],
            teams: vec![],
            ssh: vec![],
            databases: vec![],
            plugins: vec![],
            notifications: vec![],
            turns: vec![],
        });
        reconcile(&view, cx);
        assert!(get(node, removed, cx).pinned);
        assert!(get(node, deleted, cx).pinned);
        view.connected = true;
        reconcile(&view, cx);
        assert!(get(node, present, cx).pinned);
        assert_eq!(get(node, archived, cx), State::default());
        assert_eq!(get(node, removed, cx), State::default());
        assert!(get(other, removed, cx).pinned);
        assert!(get(node, running, cx).pinned);
        assert_eq!(get(node, closed, cx), State::default());
        assert_eq!(get(node, deleted, cx), State::default());
        assert!(get(other, deleted, cx).pinned);
        let stale = view.clone();
        let snapshot = view.snapshot.as_mut().unwrap();
        snapshot.cursor = 2;
        snapshot
            .sessions
            .iter_mut()
            .find(|session| session.id == archived)
            .unwrap()
            .archived = false;
        let mut created = snapshot.sessions[0].clone();
        created.id = removed;
        snapshot.sessions.push(created);
        let mut created = snapshot.terminals[0].clone();
        created.id = deleted;
        snapshot.terminals.push(created);
        reconcile(&view, cx);
        for id in [archived, removed] {
            set(
                node,
                id,
                State {
                    pinned: true,
                    order: None,
                },
                cx,
            );
        }
        set(
            node,
            deleted,
            State {
                pinned: true,
                order: None,
            },
            cx,
        );
        reconcile(&stale, cx);
        assert!(get(node, archived, cx).pinned);
        assert!(get(node, removed, cx).pinned);
        assert!(get(node, deleted, cx).pinned);
        let snapshot = view.snapshot.as_mut().unwrap();
        snapshot.cursor = 3;
        snapshot.sessions.retain(|session| session.id != removed);
        snapshot.terminals.retain(|terminal| terminal.id != deleted);
        snapshot.terminals[0].status = Status::Closed;
        reconcile(&view, cx);
        assert!(get(node, archived, cx).pinned);
        assert_eq!(get(node, removed, cx), State::default());
        assert_eq!(get(node, deleted, cx), State::default());
        assert_eq!(get(node, running, cx), State::default());
    });
}
