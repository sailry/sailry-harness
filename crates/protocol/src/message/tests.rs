use super::*;
use serde_json::{Value, json};

fn unknown(data: Value) -> Value {
    json!({ "kind": "future_result", "data": data })
}

mod network {
    use super::*;

    #[test]
    fn round_trips_discovery_reads() {
        let selection = crate::network::Selection::Custom {
            relays: vec!["https://relay.example".into()],
        };
        for command in [
            Command::ListNetworkPresets { refresh: false },
            Command::ListNetworkPresets { refresh: true },
            Command::ProbeNetwork {
                selection: selection.clone(),
            },
        ] {
            assert!(!command.durable());
            assert_eq!(
                serde_json::from_value::<Command>(serde_json::to_value(&command).unwrap()).unwrap(),
                command
            );
        }
        for output in [
            Output::NetworkPresets(crate::network::Listing {
                presets: vec![crate::network::Preset {
                    id: "example".into(),
                    label: crate::plugin::desktop::Navigation {
                        label: "Example".into(),
                        icon: None,
                        locales: Default::default(),
                    },
                    selection: selection.clone(),
                }],
                unavailable: false,
            }),
            Output::NetworkLatency(crate::network::Latency {
                selection: selection.clone(),
                millis: Some(17),
            }),
            Output::NetworkLatency(crate::network::Latency {
                selection,
                millis: None,
            }),
        ] {
            assert_eq!(
                serde_json::from_value::<Output>(serde_json::to_value(&output).unwrap()).unwrap(),
                output
            );
        }
    }
}

mod process {
    use super::*;

    #[test]
    fn follow_up_is_opt_in() {
        let turn = TurnId::new();
        let encoded = format!(
            r#"{{"kind":"run_command","data":{{"turn":"{turn}","command":"true","access":"workspace","description":null,"cwd":"","timeout_ms":1000,"background":true,"pty":false,"attachments":[]}}}}"#,
        );
        let command: Command = serde_json::from_str(&encoded).unwrap();
        assert!(matches!(
            &command,
            Command::RunCommand {
                follow_up: false,
                ..
            }
        ));
        // Omitted defaults retain the canonical bytes used by durable request digests.
        assert_eq!(serde_json::to_string(&command).unwrap(), encoded);
        let mut value = serde_json::to_value(&command).unwrap();
        for follow_up in [false, true] {
            value["data"]["follow_up"] = json!(follow_up);
            let command: Command = serde_json::from_value(value.clone()).unwrap();
            assert!(
                matches!(&command, Command::RunCommand { follow_up: actual, .. } if *actual == follow_up)
            );
            let captured = serde_json::to_value(&command).unwrap();
            assert_eq!(
                captured["data"].get("follow_up"),
                follow_up.then_some(&Value::Bool(true))
            );
            assert_eq!(
                serde_json::from_value::<Command>(captured).unwrap(),
                command
            );
        }
        for invalid in [Value::Null, json!("true"), json!(1)] {
            value["data"]["follow_up"] = invalid;
            assert!(serde_json::from_value::<Command>(value.clone()).is_err());
        }
    }
}

mod mcp {
    use super::*;

    #[test]
    fn preserves_management_contracts() {
        let package = crate::plugin::Reference {
            name: "example".into(),
            settings_revision: 3,
            digest: "a".repeat(64),
        };
        for command in [
            Command::SetPluginMcpAllowed {
                package: package.clone(),
                server: "service".into(),
                allowed: false,
            },
            Command::CheckPluginMcp {
                package: package.clone(),
                server: "service".into(),
            },
        ] {
            assert!(command.durable());
            assert_eq!(
                serde_json::from_value::<Command>(serde_json::to_value(&command).unwrap()).unwrap(),
                command
            );
        }
        let output = Output::PluginMcpChecked(crate::plugin::mcp::Check {
            package,
            server: "service".into(),
            tools: vec![crate::plugin::mcp::Tool {
                name: "read".into(),
                description: Some("Read a resource".into()),
            }],
        });
        assert_eq!(
            serde_json::from_value::<Output>(serde_json::to_value(&output).unwrap()).unwrap(),
            output
        );
    }
}

mod faults {
    use super::*;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "defaults_missing_details",
                (|scenario: &'static str| {
                    let value = json!({ "code": "busy", "message": "fixture busy" });
                    let fault: Fault = serde_json::from_value(value.clone()).unwrap();
                    assert_eq!(fault.details, None, "{scenario}");
                    assert_eq!(serde_json::to_value(&fault).unwrap(), value, "{scenario}");
                    assert_eq!(
                        fault,
                        Fault::new(ErrorCode::Busy, "fixture busy"),
                        "{scenario}"
                    );
                }) as fn(&'static str),
            ),
            (
                "round_trips_detail_variants",
                (|scenario: &'static str| {
                    for (case, code, message, details, wire, outcome) in [
                        (
                            "execution",
                            ErrorCode::Busy,
                            "Original failure",
                            FaultDetails::ModelRequest {
                                reason: ModelFailure::Unauthorized,
                            },
                            json!({"kind":"model_request","reason":"unauthorized"}),
                            false,
                        ),
                        (
                            "network",
                            ErrorCode::Unavailable,
                            "connection failed",
                            FaultDetails::Network,
                            json!({"kind":"network"}),
                            false,
                        ),
                        (
                            "path",
                            ErrorCode::NotFound,
                            "requested file is missing",
                            FaultDetails::WorktreePathNotFound,
                            json!({"kind":"worktree_path_not_found"}),
                            true,
                        ),
                        (
                            "value conflict",
                            ErrorCode::RevisionConflict,
                            "plugin value changed",
                            FaultDetails::PluginValueConflict,
                            json!({"kind":"plugin_value_conflict"}),
                            false,
                        ),
                        (
                            "storage conflict",
                            ErrorCode::Busy,
                            "fixture storage conflict",
                            FaultDetails::StorageConflict,
                            json!({"kind":"storage_conflict"}),
                            true,
                        ),
                        (
                            "removal",
                            ErrorCode::Busy,
                            "fixture project has pending tasks",
                            FaultDetails::ProjectRemoval {
                                tasks_active: true,
                                terminals_may_be_closed: true,
                            },
                            json!({"kind":"project_removal","tasks_active":true,"terminals_may_be_closed":true}),
                            true,
                        ),
                    ] {
                        let mut fault = Fault::new(code, message);
                        fault.details = Some(details);
                        let encoded = serde_json::to_value(&fault).unwrap();
                        assert_eq!(encoded["details"], wire, "{scenario}: {case}: detail wire");
                        if case == "storage conflict" {
                            assert_eq!(
                                encoded,
                                json!({
                                    "code":"busy", "message":"fixture storage conflict",
                                    "details":{"kind":"storage_conflict"}
                                }),
                                "{scenario}: {case}: fault wire"
                            );
                        }
                        assert_eq!(
                            serde_json::from_value::<Fault>(encoded).unwrap(),
                            fault,
                            "{scenario}: {case}: fault round trip"
                        );
                        if outcome {
                            let output = Output::RequestOutcome {
                                id: RequestId::new(),
                                outcome: RequestOutcome::Completed(Box::new(Err(fault))),
                            };
                            assert_eq!(
                                serde_json::from_value::<Output>(
                                    serde_json::to_value(&output).unwrap()
                                )
                                .unwrap(),
                                output,
                                "{scenario}: {case}: outcome round trip",
                            );
                        }
                    }
                }),
            ),
            (
                "rejects_malformed_details",
                (|scenario: &'static str| {
                    for details in [
                        json!({ "kind": "future" }),
                        json!({ "kind": "project_removal" }),
                        json!({ "kind": "project_removal", "tasks_active": "true", "terminals_may_be_closed": false }),
                    ] {
                        assert!(
                            serde_json::from_value::<Fault>(json!({
                                "code": "busy",
                                "message": "fixture busy",
                                "details": details,
                            }))
                            .is_err(),
                            "{scenario}"
                        );
                    }
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}

mod history {
    use super::*;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "preserves_activity_event_shapes",
                (|scenario: &'static str| {
                    let session = SessionId::new();
                    let activity = json!({
                        "title": "Queued task",
                        "attention": {"revision": 2, "unread": true},
                        "run": null,
                        "waiting": null,
                        "queued": 1,
                    });
                    let turn = QueuedTurn {
                        id: TurnId::new(),
                        kind: crate::conversation::RunKind::Task,
                        session,
                        request: RequestId::new(),
                        revision: 1,
                        config: crate::SessionConfig {
                            features: Default::default(),
                            assistant: None,
                            resource: None,
                            provider: crate::ProviderId::new(),
                            model: "fixture".into(),
                            effort: crate::Effort::Default,
                            mode: crate::WorkMode::Code,
                            options: Default::default(),
                            permission: crate::Permission::Ask,
                            credential: None,
                        },
                        roles: Default::default(),
                        plugins: Vec::new(),
                    };
                    for wire in [
                        json!({"kind": "conversation_changed", "data": {"session": session, "activity": activity}}),
                        json!({"kind": "turn_queued", "data": {"turn": turn, "activity": activity}}),
                    ] {
                        let event: Event = serde_json::from_value(wire.clone()).unwrap();
                        assert_eq!(serde_json::to_value(event).unwrap(), wire, "{scenario}");
                    }
                }) as fn(&'static str),
            ),
            (
                "rejects_unknown_kinds",
                (|scenario: &'static str| {
                    for data in [
                        json!({ "private": "value" }),
                        json!([1, true]),
                        json!("text"),
                        json!(42),
                        json!(null),
                    ] {
                        assert!(
                            serde_json::from_value::<Event>(unknown(data)).is_err(),
                            "{scenario}"
                        );
                    }
                    assert!(
                        serde_json::from_value::<Event>(json!({ "kind": "future_event" })).is_err(),
                        "{scenario}"
                    );
                }),
            ),
            (
                "malformed_known_kinds_remain_errors",
                (|scenario: &'static str| {
                    for value in [
                        json!({ "kind": "worktree_removed", "data": {} }),
                        json!({ "kind": "worktree_removed", "data": { "id": 1 } }),
                        json!({ "kind": "worktree_removed", "data": { "id": "invalid" } }),
                        json!({ "kind": "notifications_dismissed", "data": { "ids": "invalid" } }),
                        json!({ "data": {} }),
                        json!({ "kind": 1, "data": {} }),
                    ] {
                        assert!(
                            serde_json::from_value::<Event>(value).is_err(),
                            "{scenario}"
                        );
                    }
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}

mod subscriptions {
    use super::*;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "preserves_scoped_request_shape",
                (|scenario: &'static str| {
                    let request = Request::new(
                        NodeId([7; 32]),
                        Command::ReadInstallations {
                            requests: vec![RequestId::new()],
                        },
                    );
                    let wire = json!({"kind": "scoped_installations", "data": request});
                    let topic: Topic = serde_json::from_value(wire.clone()).unwrap();
                    assert_eq!(
                        topic,
                        Topic::ScopedInstallations(Box::new(request)),
                        "{scenario}"
                    );
                    assert_eq!(serde_json::to_value(topic).unwrap(), wire, "{scenario}");
                }) as fn(&'static str),
            ),
            (
                "round_trips_scoped_status",
                (|scenario: &'static str| {
                    let scope = crate::plugin::ui::status::Scope {
                        worktree: Some(WorktreeId::new()),
                        session: Some(SessionId::new()),
                    };
                    let wire = json!({"kind": "status", "data": scope});
                    let topic: Topic = serde_json::from_value(wire.clone()).unwrap();
                    assert_eq!(topic, Topic::Status(scope), "{scenario}");
                    assert_eq!(serde_json::to_value(topic).unwrap(), wire, "{scenario}");
                    let snapshot = serde_json::from_value(json!({
                    "node": NodeId([7; 32]),
                    "scope": scope,
                    "entries": [{
                        "package": {"name": "fixture", "digest": "captured", "settings_revision": 3},
                        "declaration": {
                            "id": "badge", "slot": "status", "kind": "button",
                            "label": {"label": "Badge"}, "handler": "desktop"
                        },
                        "state": {
                            "id": "badge", "details": [{"label": {"label": "Files"}, "value": "3"}]
                        },
                        "invoke": "activate"
                    }],
                    "failures": [{
                        "package": {"name": "another", "digest": "captured", "settings_revision": 2},
                        "fault": Fault::new(ErrorCode::Unavailable, "status unavailable")
                    }]
                }))
                .unwrap();
                    let update = Update::Status(snapshot);
                    let wire = serde_json::to_value(&update).unwrap();
                    assert_eq!(wire["kind"], "status", "{scenario}");
                    assert_eq!(
                        wire["data"]["scope"],
                        serde_json::to_value(scope).unwrap(),
                        "{scenario}"
                    );
                    assert_eq!(
                        serde_json::from_value::<Update>(wire).unwrap(),
                        update,
                        "{scenario}"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}

mod receipts {
    use super::*;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "rejects_unknown_kinds",
                (|scenario: &'static str| {
                    for data in [
                        json!({ "private": "value" }),
                        json!([1, true]),
                        json!("text"),
                        json!(42),
                        json!(null),
                    ] {
                        assert!(
                            serde_json::from_value::<Output>(unknown(data.clone())).is_err(),
                            "{scenario}"
                        );
                        assert!(
                            serde_json::from_value::<crate::dispatch::Output>(unknown(data))
                                .is_err(),
                            "{scenario}"
                        );
                    }
                }) as fn(&'static str),
            ),
            (
                "rejects_nested_unknown_results",
                (|scenario: &'static str| {
                    let values = [
                        json!({ "kind": "plugin_transaction", "data": [{ "kind": "browser_completed" }, unknown(json!({}))] }),
                        json!({ "kind": "dispatch", "data": unknown(json!({})) }),
                        json!({ "kind": "dispatch", "data": { "kind": "result", "data": { "kind": "completed", "data": { "Ok": unknown(json!({})) } } } }),
                        json!({ "kind": "request_outcome", "data": { "id": RequestId::new(), "outcome": { "kind": "completed", "data": { "Ok": { "kind": "plugin_transaction", "data": [{ "kind": "dispatch", "data": unknown(json!({})) }] } } } } }),
                    ];
                    for value in values {
                        assert!(
                            serde_json::from_value::<Output>(value).is_err(),
                            "{scenario}"
                        );
                    }
                }),
            ),
            (
                "round_trips_results_and_faults",
                (|scenario: &'static str| {
                    let fault = Fault::new(ErrorCode::Unavailable, "unavailable");
                    for output in [
                        Output::BrowserCompleted,
                        Output::PluginTransaction(vec![Output::BrowserCompleted]),
                        Output::Dispatch(crate::dispatch::Output::Removed),
                        Output::Dispatch(crate::dispatch::Output::Result(RequestOutcome::Unknown)),
                        Output::RequestOutcome {
                            id: RequestId::new(),
                            outcome: RequestOutcome::Completed(Box::new(Err(fault))),
                        },
                    ] {
                        let encoded = serde_json::to_value(&output).unwrap();
                        assert_eq!(
                            serde_json::from_value::<Output>(encoded).unwrap(),
                            output,
                            "{scenario}"
                        );
                    }
                }),
            ),
            (
                "malformed_known_kinds_remain_errors",
                (|scenario: &'static str| {
                    for value in [
                        json!({ "kind": "file_transfer_cancelled", "data": {} }),
                        json!({ "kind": "file_transfer_cancelled", "data": { "stream": 1 } }),
                        json!({ "kind": "plugin_transaction", "data": {} }),
                        json!({ "kind": "browser_completed", "data": "invalid" }),
                        json!({ "data": {} }),
                        json!({ "kind": 1, "data": {} }),
                    ] {
                        assert!(
                            serde_json::from_value::<Output>(value).is_err(),
                            "{scenario}"
                        );
                    }
                    for value in [
                        json!({ "kind": "published", "data": {} }),
                        json!({ "kind": "jobs", "data": "invalid" }),
                        json!({ "kind": "removed", "data": "invalid" }),
                    ] {
                        assert!(
                            serde_json::from_value::<crate::dispatch::Output>(value).is_err(),
                            "{scenario}"
                        );
                    }
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
