use super::*;
use sailry_protocol::{Command, plugin::Context};
use serde_json::json;

fn source() -> feed::Entry {
    serde_json::from_value(json!({
        "package": {"name": "fixture", "digest": "captured", "settings_revision": 3},
        "declaration": {
            "id": "details", "slot": "status", "kind": "popover", "intent": "details",
            "label": {"label": "Details"}, "handler": "desktop"
        },
        "state": {
            "id": "details", "visible": false, "enabled": true,
            "segments": [
                {"label": {"label": "+0"}, "tone": "success"},
                {"label": {"label": "-0"}, "tone": "danger"}
            ]
        }
    }))
    .unwrap()
}

fn key(source: &feed::Entry) -> Key {
    Key {
        node: NodeId([1; 32]),
        worktree: Some(WorktreeId::new()),
        session: Some(sailry_protocol::SessionId::new()),
        package: source.package.clone(),
        id: source.declaration.id.clone(),
    }
}

fn pending(key: &Key) -> Pending {
    Pending {
        request: Request::new(
            key.node,
            Command::CallPlugin {
                handler: "activate".into(),
                input: json!({"id": key.id}),
            },
        )
        .with_plugin(Context {
            invocation: None,
            turn: None,
            surface: Surface::Composer,
            package: key.package.clone(),
            worktree: key.worktree,
            session: key.session,
        }),
        active: true,
        task: None,
    }
}

#[test]
fn open_preserves_hidden_popover_and_node_state() {
    let source = source();
    let key = key(&source);
    let closed = present(&source, key.clone(), true, false, None);
    assert!(!closed.state.visible);
    assert!(closed.state.enabled);
    assert_eq!(closed.state.value, false);
    let open = present(&source, key, true, true, None);
    assert!(open.state.visible);
    assert!(open.state.enabled);
    assert_eq!(open.state.value, true);
    assert_eq!(open.state.segments, source.state.segments);
    assert!(!open.retry);
    assert!(!source.state.visible);
    assert!(source.state.value.is_null());
    assert!(ui::status::valid(&source.state, &source.declaration));
}

#[test]
fn connection_and_flight_control_enabled_state() {
    let mut source = source();
    let key = key(&source);
    let pending = pending(&key);
    for (connected, active) in [(false, false), (false, true), (true, true)] {
        let entry = present(
            &source,
            key.clone(),
            connected,
            true,
            active.then_some(&pending),
        );
        assert!(entry.state.visible);
        assert!(!entry.state.enabled);
    }
    assert!(
        present(&source, key.clone(), true, true, None)
            .state
            .enabled
    );
    source.state.enabled = false;
    assert!(!present(&source, key, true, true, None).state.enabled);
}

#[tokio::test]
async fn intent_opens_hidden_popover_from_command_result() {
    let source = source();
    let entry = present(&source, key(&source), true, false, None);
    assert!(!entry.state.visible);
    assert!(entry.state.enabled);
    let value = intent_value(entry.declaration.kind, true.into());
    assert!(value.is_null());
    let bridge = Bridge::new(vec![entry.declaration]);
    bridge
        .forward(&entry.key.id, ui::EventKind::Invoke, value)
        .unwrap();
    let event = bridge.next().await.unwrap();
    assert_eq!(event.id, entry.key.id);
    assert_eq!(event.kind, ui::EventKind::Invoke);
    assert!(event.value.is_null());
    assert_eq!(intent_value(Kind::Button, true.into()), true);
    assert_eq!(intent_value(Kind::Popover, false.into()), false);
}

#[test]
fn tooltip_uses_localized_details_in_order() {
    assert_eq!(tooltip(&[], "en"), None);
    let details: Vec<ui::Detail> = serde_json::from_value(json!([
        {"label": {"label": "Added", "locales": {"zh-CN": "新增"}}, "value": "0"},
        {"label": {"label": "Partial results", "locales": {"zh-CN": "部分结果"}}, "value": ""},
        {"label": {"label": "Progress"}, "value": "50%", "progress": 50}
    ]))
    .unwrap();
    assert_eq!(
        tooltip(&details, "en").unwrap(),
        "Added: 0\nPartial results\nProgress: 50%"
    );
    assert_eq!(
        tooltip(&details, "zh-CN").unwrap(),
        "新增: 0\n部分结果\nProgress: 50%"
    );
}

#[test]
fn keys_fence_scope_and_package_replacements() {
    let source = source();
    let key = key(&source);
    let mut keys = BTreeSet::from([key.clone()]);
    for changed in [
        Key {
            node: NodeId([2; 32]),
            ..key.clone()
        },
        Key {
            worktree: Some(WorktreeId::new()),
            ..key.clone()
        },
        Key {
            session: Some(sailry_protocol::SessionId::new()),
            ..key.clone()
        },
        Key {
            id: "other".into(),
            ..key.clone()
        },
        {
            let mut changed = key.clone();
            changed.package.digest = "replacement".into();
            changed
        },
        {
            let mut changed = key.clone();
            changed.package.settings_revision += 1;
            changed
        },
    ] {
        assert_ne!(changed, key);
        assert!(keys.insert(changed));
    }
    assert_eq!(keys.len(), 7);
}

#[test]
fn uncertain_action_retains_exact_request_for_retry() {
    let mut source = source();
    source.declaration.kind = Kind::Button;
    source.invoke = Some("activate".into());
    source.state.visible = true;
    let identity = key(&source);
    let mut pending = pending(&identity);
    let request = pending.request.clone();
    assert!(pending.retry().is_err());
    assert!(pending.complete(&Err(Fault::new(ErrorCode::OutcomeUnknown, "unknown"))));
    assert!(!pending.active);
    assert_eq!(pending.retry().unwrap(), request);
    assert!(
        present(&source, key(&source), true, true, Some(&pending))
            .state
            .enabled
    );
    for result in [
        Ok(Output::PluginResult(json!({}))),
        Err(Fault::new(ErrorCode::Unavailable, "unavailable")),
        Err(Fault::new(ErrorCode::InvalidRequest, "invalid")),
    ] {
        pending.active = true;
        assert!(!pending.complete(&result));
        assert!(!pending.active);
    }
}

#[test]
fn uncertain_hidden_action_remains_recoverable() {
    let mut source = source();
    source.declaration.kind = Kind::Button;
    source.invoke = Some("activate".into());
    source.state.visible = true;
    let key = key(&source);
    let mut pending = pending(&key);
    let request = pending.request.clone();
    source.state.visible = false;
    assert!(pending.complete(&Err(Fault::new(ErrorCode::OutcomeUnknown, "unknown"))));
    let entry = present(&source, key.clone(), true, false, Some(&pending));
    assert!(entry.state.visible);
    assert!(entry.state.enabled);
    assert!(entry.retry);
    assert_eq!(pending.retry().unwrap(), request);
    assert!(!source.state.visible);
    let fresh = present(&source, key.clone(), true, false, None);
    assert!(!fresh.state.visible);
    pending.active = true;
    let active = present(&source, key.clone(), true, false, Some(&pending));
    assert!(active.state.visible);
    assert!(!active.state.enabled);
    assert!(!active.retry);
    assert!(pending.retry().is_err());
    pending.active = false;
    let disconnected = present(&source, key, false, false, Some(&pending));
    assert!(disconnected.state.visible);
    assert!(!disconnected.state.enabled);
    assert!(disconnected.retry);
}

#[test]
fn uncertain_disabled_action_remains_recoverable() {
    let mut source = source();
    source.declaration.kind = Kind::Button;
    source.invoke = Some("activate".into());
    source.state.visible = true;
    let key = key(&source);
    let mut pending = pending(&key);
    let request = pending.request.clone();
    source.state.enabled = false;
    assert!(pending.complete(&Err(Fault::new(ErrorCode::OutcomeUnknown, "unknown"))));
    let entry = present(&source, key.clone(), true, false, Some(&pending));
    assert!(entry.state.visible);
    assert!(entry.state.enabled);
    assert!(entry.retry);
    assert_eq!(pending.retry().unwrap(), request);
    assert!(!source.state.enabled);
    let fresh = present(&source, key, true, false, None);
    assert!(!fresh.state.enabled);
    assert!(!fresh.retry);
}
