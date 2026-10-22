use super::*;
use serde_json::json;

fn picker() -> Arc<Bridge> {
    Bridge::new(vec![
        serde_json::from_value(json!({
            "id":"resource", "slot":"context", "kind":"picker",
            "label":{"label":"Resource"}, "handler":"select"
        }))
        .unwrap(),
    ])
}

fn state(value: Value) -> State {
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn exposes_current_state_before_projection() {
    let bridge = Bridge::new(vec![
        serde_json::from_value(json!({
            "id":"create", "slot":"composer", "kind":"button", "intent":"create_worktree",
            "label":{"label":"Create"}, "handler":"create"
        }))
        .unwrap(),
    ]);
    let mut updates = bridge.subscribe();
    let pending = updates.borrow().clone();
    assert!(!pending["create"].enabled);
    assert!(bridge.state("missing").is_none());

    bridge
        .publish(vec![state(json!({"id":"create", "visible":false}))])
        .unwrap();
    assert!(bridge.ready());
    assert!(!pending["create"].enabled);
    assert!(bridge.state("create").unwrap().enabled);
    let available = updates.borrow_and_update().clone();

    bridge.invoke_intent("create", Value::Null).unwrap();
    let first = bridge.next().await.unwrap();
    assert!(available["create"].enabled);
    assert!(!bridge.state("create").unwrap().enabled);
    assert!(bridge.invoke_intent("create", Value::Null).is_err());
    bridge
        .publish(vec![state(json!({"id":"create", "enabled":true}))])
        .unwrap();
    assert!(!bridge.state("create").unwrap().enabled);
    let busy = updates.borrow_and_update().clone();

    bridge
        .publish(vec![state(
            json!({"id":"create", "reply_to":first.sequence}),
        )])
        .unwrap();
    assert!(!busy["create"].enabled);
    assert!(bridge.state("create").unwrap().enabled);
    bridge.invoke_intent("create", Value::Null).unwrap();
    let latest = bridge.next().await.unwrap();
    bridge
        .publish(vec![state(
            json!({"id":"create", "reply_to":first.sequence}),
        )])
        .unwrap();
    assert!(!bridge.state("create").unwrap().enabled);
    bridge
        .publish(vec![state(
            json!({"id":"create", "reply_to":latest.sequence, "enabled":false}),
        )])
        .unwrap();
    assert!(!bridge.state("create").unwrap().enabled);
    assert!(bridge.invoke_intent("create", Value::Null).is_err());
}

#[tokio::test]
async fn named_intents_preserve_hidden_entries_and_values() {
    let bridge = Bridge::new(vec![
        serde_json::from_value(json!({
            "id":"create", "slot":"composer", "kind":"button", "intent":"create_worktree",
            "label":{"label":"Create"}, "handler":"create"
        }))
        .unwrap(),
    ]);
    bridge
        .publish(vec![state(json!({"id":"create", "visible":false}))])
        .unwrap();
    assert!(
        bridge
            .invoke("create", EventKind::Invoke, Value::Null)
            .is_err()
    );
    let frozen = json!({"branch":"topic", "commit":"captured"});
    bridge.invoke_intent("create", frozen.clone()).unwrap();
    let event = bridge.next().await.unwrap();
    assert_eq!(event.value, frozen);
    assert_eq!(event.handler, "create");
    assert!(bridge.invoke_intent("create", Value::Null).is_err());
    bridge
        .publish(vec![state(
            json!({"id":"create", "visible":false, "reply_to":event.sequence, "enabled":false}),
        )])
        .unwrap();
    assert!(bridge.invoke_intent("create", Value::Null).is_err());
}

#[tokio::test]
async fn keeps_latest_search_results() {
    let bridge = picker();
    bridge
        .publish(vec![state(json!({"id":"resource"}))])
        .unwrap();
    bridge
        .invoke("resource", EventKind::Search, json!("a"))
        .unwrap();
    let first = bridge.next().await.unwrap();
    bridge
        .invoke("resource", EventKind::Search, json!("ab"))
        .unwrap();
    let latest = bridge.next().await.unwrap();
    bridge
        .publish(vec![state(
            json!({"id":"resource", "reply_to":latest.sequence,
        "choices":[{"id":"ab", "label":{"label":"AB"}}]}),
        )])
        .unwrap();
    bridge
        .publish(vec![state(
            json!({"id":"resource", "reply_to":first.sequence,
        "choices":[{"id":"a", "label":{"label":"A"}}]}),
        )])
        .unwrap();
    assert_eq!(
        bridge.subscribe().borrow()["resource"]
            .choices
            .as_ref()
            .unwrap()[0]
            .id,
        "ab"
    );
    assert!(
        bridge
            .invoke("resource", EventKind::Change, json!("a"))
            .is_err()
    );
    bridge
        .invoke("resource", EventKind::Change, json!("ab"))
        .unwrap();
    assert_eq!(bridge.next().await.unwrap().handler, "select");
}

#[test]
fn rejects_invalid_batches_without_partial_updates() {
    let bridge = picker();
    assert!(!bridge.ready());
    bridge.publish(vec![]).unwrap();
    assert!(!bridge.ready());
    let initial = bridge.subscribe().borrow().clone();
    for invalid in [
        json!({"id":"missing"}),
        json!({"id":"resource", "reply_to":1}),
        json!({"id":"resource", "choices":[{"id":"x","label":{"label":"X"}},
            {"id":"x","label":{"label":"Duplicate"}}]}),
    ] {
        assert!(
            bridge
                .publish(vec![
                    state(json!({"id":"resource", "enabled":true})),
                    state(invalid)
                ])
                .is_err()
        );
        assert_eq!(*bridge.subscribe().borrow(), initial);
        assert!(!bridge.ready());
    }
    assert!(
        bridge
            .invoke("resource", EventKind::Search, json!("a"))
            .is_err()
    );
    bridge
        .publish(vec![state(json!({"id":"resource", "choices":[]}))])
        .unwrap();
    assert!(bridge.ready());
    assert!(
        bridge
            .invoke("resource", EventKind::Change, json!("missing"))
            .is_err()
    );
}

#[tokio::test]
async fn keeps_actions_busy_until_their_reply() {
    let mut declarations = picker().declarations.clone();
    let mut other = declarations[0].clone();
    other.id = "other".into();
    declarations.push(other);
    let bridge = Bridge::new(declarations);
    let choices = json!([{"id":"a", "label":{"label":"A"}}]);
    bridge
        .publish(vec![
            state(json!({"id":"resource", "choices":choices})),
            state(json!({"id":"other"})),
        ])
        .unwrap();
    bridge
        .invoke("resource", EventKind::Change, json!("a"))
        .unwrap();
    let event = bridge.next().await.unwrap();
    assert!(!bridge.subscribe().borrow()["resource"].enabled);
    assert!(
        bridge
            .invoke("resource", EventKind::Change, json!("a"))
            .is_err()
    );

    bridge
        .publish(vec![state(json!({"id":"other", "value":"updated"}))])
        .unwrap();
    assert!(!bridge.subscribe().borrow()["resource"].enabled);
    bridge
        .publish(vec![state(json!({"id":"resource", "choices":choices}))])
        .unwrap();
    assert!(!bridge.subscribe().borrow()["resource"].enabled);

    bridge
        .publish(vec![state(json!({
            "id":"resource", "choices":choices, "value":"a", "reply_to":event.sequence
        }))])
        .unwrap();
    assert!(bridge.subscribe().borrow()["resource"].enabled);
    bridge
        .invoke("resource", EventKind::Change, json!("a"))
        .unwrap();
}

#[tokio::test]
async fn rejects_disabled_declared_choices() {
    let bridge = Bridge::new(vec![
        serde_json::from_value(json!({
            "id": "tool", "slot": "project_menu", "kind": "menu",
            "label": { "label": "CLI" }, "handler": "launch",
            "choices": [
                { "id": "missing", "label": { "label": "Missing" }, "enabled": false },
                { "id": "available", "label": { "label": "Available" } }
            ]
        }))
        .unwrap(),
    ]);
    bridge.publish(vec![state(json!({"id": "tool"}))]).unwrap();
    assert!(
        bridge
            .invoke("tool", EventKind::Change, json!("missing"))
            .is_err()
    );
    assert!(bridge.subscribe().borrow()["tool"].enabled);
    bridge
        .invoke("tool", EventKind::Change, json!("available"))
        .unwrap();
    let action = bridge.next().await.unwrap();
    assert_eq!(action.sequence, 1);
    assert_eq!(action.value, "available");
}

#[tokio::test]
async fn uses_current_choice_availability() {
    let bridge = picker();
    bridge
        .publish(vec![state(json!({
            "id": "resource", "choices": [
                {"id": "tool", "label": {"label": "Tool"}, "enabled": false}
            ]
        }))])
        .unwrap();
    assert!(
        bridge
            .invoke("resource", EventKind::Change, json!("tool"))
            .is_err()
    );
    assert!(bridge.subscribe().borrow()["resource"].enabled);
    bridge
        .publish(vec![state(json!({
            "id": "resource", "choices": [
                {"id": "tool", "label": {"label": "Tool"}}
            ]
        }))])
        .unwrap();
    bridge
        .invoke("resource", EventKind::Change, json!("tool"))
        .unwrap();
    let action = bridge.next().await.unwrap();
    assert_eq!(action.sequence, 1);
    assert_eq!(action.value, "tool");
}

#[test]
fn hides_contributions_until_state_arrives() {
    let bridge = picker();
    assert!(!bridge.state("resource").unwrap().visible);
    bridge
        .publish(vec![state(json!({"id":"resource"}))])
        .unwrap();
    assert!(bridge.state("resource").unwrap().visible);
}

#[test]
fn fences_replaced_provider_scopes() {
    let bridge = Bridge::new(vec![
        serde_json::from_value(
            json!({"id":"usage","slot":"provider","kind":"metric","label":{"label":"Usage"}}),
        )
        .unwrap(),
    ]);
    let first = ui::ProviderScope {
        provider: sailry_protocol::ProviderId::new(),
        revision: "3:1".into(),
        credential: sailry_protocol::CredentialRef {
            node: sailry_protocol::NodeId([1; 32]),
            id: sailry_protocol::CredentialId::new(),
        },
    };
    let second = ui::ProviderScope {
        provider: sailry_protocol::ProviderId::new(),
        ..first.clone()
    };
    let metric = |scope: &ui::ProviderScope, value: &str| {
        state(json!({"id":"usage","scope":scope,"value":value}))
    };
    bridge.scopes(vec![first.clone(), second.clone()]);
    bridge
        .publish(vec![metric(&first, "10%"), metric(&second, "80%")])
        .unwrap();
    assert_eq!(
        bridge
            .subscribe()
            .borrow()
            .values()
            .filter(|state| state.scope.is_some())
            .count(),
        2
    );
    assert!(
        bridge
            .publish(vec![metric(&first, "11%"), metric(&first, "12%")])
            .is_err()
    );
    let replaced = ui::ProviderScope {
        revision: "4:1".into(),
        credential: sailry_protocol::CredentialRef {
            node: first.credential.node,
            id: sailry_protocol::CredentialId::new(),
        },
        ..first.clone()
    };
    bridge.scopes(vec![replaced.clone(), second.clone()]);
    assert!(
        !bridge
            .subscribe()
            .borrow()
            .values()
            .any(|state| state.scope.as_ref() == Some(&first))
    );
    assert!(bridge.publish(vec![metric(&first, "100%")]).is_err());
    bridge.publish(vec![metric(&replaced, "20%")]).unwrap();
    assert!(
        bridge
            .subscribe()
            .borrow()
            .values()
            .any(|state| state.scope.as_ref() == Some(&replaced) && state.value == json!("20%"))
    );
    assert!(
        bridge
            .subscribe()
            .borrow()
            .values()
            .any(|state| state.scope.as_ref() == Some(&second) && state.value == json!("80%"))
    );
    let renewed = ui::ProviderScope {
        revision: "4:2".into(),
        ..replaced.clone()
    };
    assert_eq!(renewed.credential, replaced.credential);
    bridge.scopes(vec![renewed.clone(), second.clone()]);
    assert!(bridge.publish(vec![metric(&replaced, "99%")]).is_err());
    bridge.publish(vec![metric(&renewed, "35%")]).unwrap();
    assert!(
        bridge
            .subscribe()
            .borrow()
            .values()
            .any(|state| state.scope.as_ref() == Some(&renewed) && state.value == json!("35%"))
    );
    bridge.scopes(vec![]);
    assert!(bridge.publish(vec![metric(&replaced, "100%")]).is_err());
    assert!(
        !bridge
            .subscribe()
            .borrow()
            .values()
            .any(|state| state.scope.is_some())
    );
}

#[tokio::test]
async fn indicator_uses_controlled_boolean_interactions() {
    let bridge = Bridge::new(vec![
        serde_json::from_value(json!({
            "id":"progress", "slot":"composer", "kind":"indicator",
            "label":{"label":"Progress"}, "handler":"details"
        }))
        .unwrap(),
    ]);
    bridge
        .publish(vec![state(
            json!({"id":"progress", "value":{"percent":25,"tone":"muted","hint":"Progress"}}),
        )])
        .unwrap();
    assert!(
        bridge
            .invoke("progress", EventKind::Change, json!("open"))
            .is_err()
    );
    assert!(
        bridge
            .invoke("progress", EventKind::Invoke, Value::Null)
            .is_err()
    );
    bridge
        .invoke("progress", EventKind::Change, json!(true))
        .unwrap();
    let event = bridge.next().await.unwrap();
    assert_eq!(event.handler, "details");
    assert_eq!(event.value, json!(true));
    assert!(!bridge.state("progress").unwrap().enabled);
    bridge
        .publish(vec![state(
            json!({"id":"progress", "reply_to":event.sequence,
        "value":{"percent":25,"open":true,"tone":"muted","hint":"Progress"}}),
        )])
        .unwrap();
    assert!(bridge.state("progress").unwrap().enabled);
    bridge
        .invoke("progress", EventKind::Change, json!(false))
        .unwrap();
    assert_eq!(bridge.next().await.unwrap().value, json!(false));
}
