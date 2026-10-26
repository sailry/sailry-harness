use super::*;
use sailry_protocol::Defaults;

fn snapshot(node: NodeId, cursor: u64) -> NodeSnapshot {
    NodeSnapshot {
        node,
        cursor,
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
fn requires_current_authoritative_absence() {
    let node = NodeId([1; 32]);
    let session = SessionId::new();
    let mut view = NodeView::default();
    assert!(!confirmed(&view, session, &snapshot(node, 20)));
    view.snapshot = Some(snapshot(node, 10));
    assert!(!confirmed(&view, session, &snapshot(node, 20)));
    view.connected = true;
    assert!(!confirmed(&view, session, &snapshot(node, 9)));
    assert!(!confirmed(&view, session, &snapshot(NodeId([2; 32]), 20)));
    assert!(confirmed(&view, session, &snapshot(node, 10)));
    view.snapshot = Some(snapshot(node, 0));
    assert!(confirmed(&view, session, &snapshot(node, 0)));
}
