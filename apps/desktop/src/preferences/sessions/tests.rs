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
        cx.set_global(Preferences::open(path));
        assert_eq!(get(node, first, cx).order, Some(0));
        assert_eq!(get(node, second, cx).order, Some(1));
        assert_eq!(get(other, first, cx).order, Some(0));
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
        assert_eq!(get(node, first, cx).order, Some(2));
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
fn reconciles_only_authoritative_host_sessions(cx: &mut TestAppContext) {
    use sailry_protocol::{Defaults, Snapshot};
    let node = NodeId([1; 32]);
    let other = NodeId([2; 32]);
    let present = SessionId::new();
    let archived = SessionId::new();
    let removed = SessionId::new();
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
            terminals: vec![],
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
        view.connected = true;
        reconcile(&view, cx);
        assert!(get(node, present, cx).pinned);
        assert_eq!(get(node, archived, cx), State::default());
        assert_eq!(get(node, removed, cx), State::default());
        assert!(get(other, removed, cx).pinned);
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
        reconcile(&stale, cx);
        assert!(get(node, archived, cx).pinned);
        assert!(get(node, removed, cx).pinned);
        let snapshot = view.snapshot.as_mut().unwrap();
        snapshot.cursor = 3;
        snapshot.sessions.retain(|session| session.id != removed);
        reconcile(&view, cx);
        assert!(get(node, archived, cx).pinned);
        assert_eq!(get(node, removed, cx), State::default());
    });
}
