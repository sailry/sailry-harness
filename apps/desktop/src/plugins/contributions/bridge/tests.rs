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

#[test]
fn open_popovers_use_their_visible_surface() {
    let bridge = Bridge::new(vec![
        serde_json::from_value(json!({
            "id":"location", "slot":"context", "kind":"popover",
            "label":{"label":"Location"}, "handler":"location"
        }))
        .unwrap(),
        serde_json::from_value(json!({
            "id":"mode", "slot":"composer", "kind":"toggle",
            "label":{"label":"Mode"}, "handler":"mode"
        }))
        .unwrap(),
    ]);
    assert!(!bridge.popover_open(|_| true));
    bridge
        .publish(vec![state(json!({"id":"mode","value":true}))])
        .unwrap();
    assert!(!bridge.popover_open(|_| true));
    bridge
        .publish(vec![state(json!({"id":"location","value":true}))])
        .unwrap();
    assert!(bridge.popover_open(|slot| slot == ui::Slot::Context));
    assert!(!bridge.popover_open(ui::Slot::is_project));
    bridge
        .publish(vec![state(
            json!({"id":"location","value":true,"visible":false}),
        )])
        .unwrap();
    assert!(!bridge.popover_open(|_| true));
    bridge
        .publish(vec![state(json!({"id":"location","value":false}))])
        .unwrap();
    assert!(!bridge.popover_open(|_| true));
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
    assert_eq!(bridge.sequence(), Some(0));
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
    assert_eq!(bridge.sequence(), Some(first.sequence));
    assert!(available["create"].enabled);
    assert!(!bridge.state("create").unwrap().enabled);
    assert!(bridge.invoke_intent("create", Value::Null).is_err());
    assert_eq!(bridge.sequence(), Some(first.sequence));
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
    assert!(latest.sequence > first.sequence);
    assert_eq!(bridge.sequence(), Some(latest.sequence));
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
fn publication_and_scope() {
    for (scenario, check) in [
        (
            "rejects_invalid_batches_without_partial_updates",
            (|_scenario: &str| {
                let bridge = picker();
                assert!(
                    !bridge.ready(),
                    "rejects invalid batches without partial updates"
                );
                bridge.publish(vec![]).unwrap();
                assert!(
                    !bridge.ready(),
                    "rejects invalid batches without partial updates"
                );
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
                            .is_err(),
                        "rejects invalid batches without partial updates"
                    );
                    assert_eq!(
                        *bridge.subscribe().borrow(),
                        initial,
                        "rejects invalid batches without partial updates"
                    );
                    assert!(
                        !bridge.ready(),
                        "rejects invalid batches without partial updates"
                    );
                }
                assert!(
                    bridge
                        .invoke("resource", EventKind::Search, json!("a"))
                        .is_err(),
                    "rejects invalid batches without partial updates"
                );
                bridge
                    .publish(vec![state(json!({"id":"resource", "choices":[]}))])
                    .unwrap();
                assert!(
                    bridge.ready(),
                    "rejects invalid batches without partial updates"
                );
                assert!(
                    bridge
                        .invoke("resource", EventKind::Change, json!("missing"))
                        .is_err(),
                    "rejects invalid batches without partial updates"
                );
            }) as fn(&str),
        ),
        (
            "hides_contributions_until_state_arrives",
            (|_scenario: &str| {
                let bridge = picker();
                assert!(
                    !bridge.state("resource").unwrap().visible,
                    "hides contributions until state arrives"
                );
                bridge
                    .publish(vec![state(json!({"id":"resource"}))])
                    .unwrap();
                assert!(
                    bridge.state("resource").unwrap().visible,
                    "hides contributions until state arrives"
                );
            }),
        ),
        (
            "fences_replaced_provider_scopes",
            (|_scenario: &str| {
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
                    2,
                    "fences replaced provider scopes"
                );
                assert!(
                    bridge
                        .publish(vec![metric(&first, "11%"), metric(&first, "12%")])
                        .is_err(),
                    "fences replaced provider scopes"
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
                        .any(|state| state.scope.as_ref() == Some(&first)),
                    "fences replaced provider scopes"
                );
                assert!(
                    bridge.publish(vec![metric(&first, "100%")]).is_err(),
                    "fences replaced provider scopes"
                );
                bridge.publish(vec![metric(&replaced, "20%")]).unwrap();
                assert!(
                    bridge
                        .subscribe()
                        .borrow()
                        .values()
                        .any(|state| state.scope.as_ref() == Some(&replaced)
                            && state.value == json!("20%")),
                    "fences replaced provider scopes"
                );
                assert!(
                    bridge
                        .subscribe()
                        .borrow()
                        .values()
                        .any(|state| state.scope.as_ref() == Some(&second)
                            && state.value == json!("80%")),
                    "fences replaced provider scopes"
                );
                let renewed = ui::ProviderScope {
                    revision: "4:2".into(),
                    ..replaced.clone()
                };
                assert_eq!(
                    renewed.credential, replaced.credential,
                    "fences replaced provider scopes"
                );
                bridge.scopes(vec![renewed.clone(), second.clone()]);
                assert!(
                    bridge.publish(vec![metric(&replaced, "99%")]).is_err(),
                    "fences replaced provider scopes"
                );
                bridge.publish(vec![metric(&renewed, "35%")]).unwrap();
                assert!(
                    bridge
                        .subscribe()
                        .borrow()
                        .values()
                        .any(|state| state.scope.as_ref() == Some(&renewed)
                            && state.value == json!("35%")),
                    "fences replaced provider scopes"
                );
                bridge.scopes(vec![]);
                assert!(
                    bridge.publish(vec![metric(&replaced, "100%")]).is_err(),
                    "fences replaced provider scopes"
                );
                assert!(
                    !bridge
                        .subscribe()
                        .borrow()
                        .values()
                        .any(|state| state.scope.is_some()),
                    "fences replaced provider scopes"
                );
            }),
        ),
        (
            "hides_unpublished_controls",
            (|_scenario: &str| {
                let bridge = picker();
                let declaration = &bridge.declarations[0];
                assert!(
                    bridge.presentation(declaration, false, false).is_none(),
                    "hides unpublished controls"
                );
                let failed = bridge.presentation(declaration, false, true).unwrap();
                assert!(failed.visible, "hides unpublished controls");
                assert!(!failed.enabled, "hides unpublished controls");
                assert!(!failed.loading, "hides unpublished controls");
                bridge.restart();
                assert!(
                    bridge.presentation(declaration, false, false).is_none(),
                    "hides unpublished controls"
                );
                bridge
                    .publish(vec![state(json!({"id":"resource"}))])
                    .unwrap();
                assert!(
                    bridge
                        .presentation(declaration, true, false)
                        .unwrap()
                        .enabled,
                    "hides unpublished controls"
                );
                bridge
                    .publish(vec![state(json!({"id":"resource", "visible":false}))])
                    .unwrap();
                assert!(
                    bridge.presentation(declaration, true, false).is_none(),
                    "hides unpublished controls"
                );
                assert!(
                    bridge.presentation(declaration, false, true).is_none(),
                    "hides unpublished controls"
                );
            }),
        ),
        (
            "publishes_each_declaration_independently",
            (|_scenario: &str| {
                let first = picker().declarations[0].clone();
                let mut second = first.clone();
                second.id = "other".into();
                let bridge = Bridge::new(vec![first.clone(), second.clone()]);
                bridge
                    .publish(vec![state(json!({"id":"resource", "visible":false}))])
                    .unwrap();
                assert!(
                    bridge.presentation(&first, true, false).is_none(),
                    "publishes each declaration independently"
                );
                assert!(
                    bridge.published(&first.id),
                    "publishes each declaration independently"
                );
                assert!(
                    !bridge.published(&second.id),
                    "publishes each declaration independently"
                );
                assert!(
                    bridge.presentation(&second, false, false).is_none(),
                    "publishes each declaration independently"
                );
            }),
        ),
        (
            "retains_disabled_labels_until_replacement",
            (|_scenario: &str| {
                let bridge = picker();
                let declaration = &bridge.declarations[0];
                bridge
                    .publish(vec![state(
                        json!({"id":"resource", "label":{"label":"Selected checkout"}}),
                    )])
                    .unwrap();
                let disconnected = bridge.presentation(declaration, false, false).unwrap();
                assert!(
                    !disconnected.enabled,
                    "retains disabled labels until replacement"
                );
                assert_eq!(
                    disconnected.label.unwrap().label,
                    "Selected checkout",
                    "retains disabled labels until replacement"
                );
                bridge.restart();
                assert!(
                    !bridge
                        .presentation(declaration, true, false)
                        .unwrap()
                        .enabled,
                    "retains disabled labels until replacement"
                );
                bridge
                    .publish(vec![state(
                        json!({"id":"resource", "label":{"label":"Recovered checkout"}}),
                    )])
                    .unwrap();
                assert!(
                    bridge
                        .presentation(declaration, true, false)
                        .unwrap()
                        .enabled,
                    "retains disabled labels until replacement"
                );
                let replacement = picker();
                assert!(
                    replacement
                        .presentation(&replacement.declarations[0], false, false)
                        .is_none(),
                    "retains disabled labels until replacement"
                );
            }),
        ),
        (
            "rejects_actions_while_loading",
            (|_scenario: &str| {
                let bridge = picker();
                bridge
                    .publish(vec![state(json!({"id":"resource", "loading":true}))])
                    .unwrap();
                assert!(
                    !bridge
                        .presentation(&bridge.declarations[0], true, false)
                        .unwrap()
                        .enabled,
                    "rejects actions while loading"
                );
                assert!(
                    bridge
                        .invoke("resource", EventKind::Search, json!("query"))
                        .is_err(),
                    "rejects actions while loading"
                );
            }),
        ),
        (
            "retains_empty_indicators_without_placeholder_labels",
            (|_scenario: &str| {
                let entry = serde_json::from_value(json!({
                "id":"progress", "slot":"composer", "kind":"indicator",
                "label":{"label":"Progress"}
                    }))
                .unwrap();
                let bridge = Bridge::new(vec![entry]);
                for failed in [false, true] {
                    let state = bridge
                        .presentation(&bridge.declarations[0], false, failed)
                        .unwrap();
                    assert!(
                        state.visible,
                        "retains empty indicators without placeholder labels"
                    );
                    assert!(
                        !state.enabled,
                        "retains empty indicators without placeholder labels"
                    );
                    assert!(
                        state.value.is_null(),
                        "retains empty indicators without placeholder labels"
                    );
                    assert!(
                        state.label.is_none(),
                        "retains empty indicators without placeholder labels"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
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

mod status {
    use super::*;

    fn bridge() -> Arc<Bridge> {
        Bridge::new(serde_json::from_value(json!([
            {"id": "button", "slot": "status", "kind": "button", "label": {"label": "Open"}, "handler": "open"},
            {"id": "details", "slot": "status", "kind": "popover", "label": {"label": "Details"}, "handler": "details"},
            {"id": "normal", "slot": "composer", "kind": "button", "label": {"label": "Normal"}, "handler": "normal"}
        ])).unwrap())
    }

    #[test]
    fn publication_and_scope() {
        for (scenario, check) in [
            (
                "rejects_publication_without_partial_updates",
                (|_scenario: &str| {
                    let bridge = bridge();
                    assert!(
                        bridge.state("button").is_none(),
                        "rejects publication without partial updates"
                    );
                    assert!(
                        bridge.state("details").is_none(),
                        "rejects publication without partial updates"
                    );
                    let pending = bridge.state("normal").unwrap();
                    assert_eq!(
                        bridge.publish(vec![
                            state(json!({"id": "normal"})),
                            state(json!({"id": "button"})),
                        ]),
                        Err("status state is owned by the execution Node"),
                        "rejects publication without partial updates"
                    );
                    assert_eq!(
                        bridge.state("normal").unwrap(),
                        pending,
                        "rejects publication without partial updates"
                    );
                    assert!(
                        !bridge.ready(),
                        "rejects publication without partial updates"
                    );
                }) as fn(&str),
            ),
            (
                "close_commands_are_scoped_bounded_and_drained",
                (|_scenario: &str| {
                    let bridge = bridge();
                    let mut updates = bridge.subscribe();
                    let states = updates.borrow_and_update().clone();
                    for id in ["missing", "button", "normal"] {
                        assert!(
                            bridge.close(id).is_err(),
                            "close commands are scoped bounded and drained"
                        );
                    }
                    assert!(
                        !updates.has_changed().unwrap(),
                        "close commands are scoped bounded and drained"
                    );
                    for _ in 0..100 {
                        bridge.close("details").unwrap();
                    }
                    assert!(
                        updates.has_changed().unwrap(),
                        "close commands are scoped bounded and drained"
                    );
                    assert_eq!(
                        *updates.borrow_and_update(),
                        states,
                        "close commands are scoped bounded and drained"
                    );
                    assert_eq!(
                        bridge.take_closed(),
                        BTreeSet::from(["details".into()]),
                        "close commands are scoped bounded and drained"
                    );
                    assert!(
                        bridge.take_closed().is_empty(),
                        "close commands are scoped bounded and drained"
                    );
                    bridge.close("details").unwrap();
                    bridge.restart();
                    assert!(
                        bridge.take_closed().is_empty(),
                        "close commands are scoped bounded and drained"
                    );
                    assert!(
                        !bridge.ready(),
                        "close commands are scoped bounded and drained"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }

    #[tokio::test]
    async fn forwards_without_state_or_reply_barriers() {
        let bridge = bridge();
        for (id, kind, value) in [
            ("button", EventKind::Invoke, Value::Null),
            ("details", EventKind::Invoke, Value::Null),
            ("details", EventKind::Change, json!(true)),
            ("details", EventKind::Change, json!(false)),
        ] {
            bridge.forward(id, kind, value.clone()).unwrap();
            let event = bridge.next().await.unwrap();
            assert_eq!(event.id, id);
            assert_eq!(event.kind, kind);
            assert_eq!(event.value, value);
            assert!(bridge.state(id).is_none());
        }
        assert_eq!(bridge.current.lock().unwrap().sequence, 4);
        assert!(bridge.current.lock().unwrap().interactions.is_empty());
        for (id, kind, value) in [
            ("missing", EventKind::Invoke, Value::Null),
            ("normal", EventKind::Invoke, Value::Null),
            ("button", EventKind::Change, json!(true)),
            ("button", EventKind::Invoke, json!({"unexpected": true})),
            ("details", EventKind::Change, json!("open")),
            ("details", EventKind::Search, json!("query")),
        ] {
            assert!(bridge.forward(id, kind, value).is_err());
        }
        assert_eq!(bridge.current.lock().unwrap().sequence, 4);
    }
}
