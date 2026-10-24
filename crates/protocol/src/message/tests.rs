use super::*;
use serde_json::{Value, json};

fn unknown(data: Value) -> Value {
    json!({ "kind": "future_result", "data": data })
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
        assert_eq!(crate::VERSION, 1);
    }
}

mod faults {
    use super::*;

    #[test]
    fn defaults_missing_details() {
        let value = json!({ "code": "busy", "message": "fixture busy" });
        let fault: Fault = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(fault.details, None);
        assert_eq!(serde_json::to_value(&fault).unwrap(), value);
        assert_eq!(fault, Fault::new(ErrorCode::Busy, "fixture busy"));
    }

    #[test]
    fn round_trips_value_conflict() {
        let mut fault = Fault::new(ErrorCode::RevisionConflict, "plugin value changed");
        fault.details = Some(FaultDetails::PluginValueConflict);
        let value = serde_json::to_value(&fault).unwrap();
        assert_eq!(value["details"], json!({"kind":"plugin_value_conflict"}));
        assert_eq!(serde_json::from_value::<Fault>(value).unwrap(), fault);
        assert_eq!(crate::VERSION, 1);
    }

    #[test]
    fn round_trips_storage_conflict() {
        let mut fault = Fault::new(ErrorCode::Busy, "fixture storage conflict");
        fault.details = Some(FaultDetails::StorageConflict);
        let value = serde_json::to_value(&fault).unwrap();
        assert_eq!(
            value,
            json!({
                "code": "busy",
                "message": "fixture storage conflict",
                "details": {"kind": "storage_conflict"},
            }),
        );
        assert_eq!(serde_json::from_value::<Fault>(value).unwrap(), fault);
        let output = Output::RequestOutcome {
            id: RequestId::new(),
            outcome: RequestOutcome::Completed(Box::new(Err(fault))),
        };
        assert_eq!(
            serde_json::from_value::<Output>(serde_json::to_value(&output).unwrap()).unwrap(),
            output,
        );
        assert_eq!(crate::VERSION, 1);
    }

    #[test]
    fn round_trips_removal_details() {
        let mut fault = Fault::new(ErrorCode::Busy, "fixture project has pending tasks");
        fault.details = Some(FaultDetails::ProjectRemoval {
            tasks_active: true,
            terminals_may_be_closed: true,
        });
        let encoded = serde_json::to_value(&fault).unwrap();
        assert_eq!(
            encoded["details"],
            json!({
                "kind": "project_removal",
                "tasks_active": true,
                "terminals_may_be_closed": true,
            }),
        );
        assert_eq!(serde_json::from_value::<Fault>(encoded).unwrap(), fault);
        let output = Output::RequestOutcome {
            id: RequestId::new(),
            outcome: RequestOutcome::Completed(Box::new(Err(fault))),
        };
        assert_eq!(
            serde_json::from_value::<Output>(serde_json::to_value(&output).unwrap()).unwrap(),
            output,
        );
    }

    #[test]
    fn rejects_malformed_details() {
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
                .is_err()
            );
        }
    }
}

mod history {
    use super::*;

    #[test]
    fn preserves_activity_event_shapes() {
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
            assert_eq!(serde_json::to_value(event).unwrap(), wire);
        }
        assert_eq!(crate::VERSION, 1);
    }

    #[test]
    fn rejects_unknown_kinds() {
        for data in [
            json!({ "private": "value" }),
            json!([1, true]),
            json!("text"),
            json!(42),
            json!(null),
        ] {
            assert!(serde_json::from_value::<Event>(unknown(data)).is_err());
        }
        assert!(serde_json::from_value::<Event>(json!({ "kind": "future_event" })).is_err());
    }

    #[test]
    fn malformed_known_kinds_remain_errors() {
        for value in [
            json!({ "kind": "worktree_removed", "data": {} }),
            json!({ "kind": "worktree_removed", "data": { "id": 1 } }),
            json!({ "kind": "worktree_removed", "data": { "id": "invalid" } }),
            json!({ "kind": "notifications_dismissed", "data": { "ids": "invalid" } }),
            json!({ "data": {} }),
            json!({ "kind": 1, "data": {} }),
        ] {
            assert!(serde_json::from_value::<Event>(value).is_err());
        }
    }
}

mod subscriptions {
    use super::*;

    #[test]
    fn preserves_scoped_request_shape() {
        let request = Request::new(
            NodeId([7; 32]),
            Command::ReadInstallations {
                requests: vec![RequestId::new()],
            },
        );
        let wire = json!({"kind": "scoped_installations", "data": request});
        let topic: Topic = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(topic, Topic::ScopedInstallations(Box::new(request)));
        assert_eq!(serde_json::to_value(topic).unwrap(), wire);
    }
}

mod receipts {
    use super::*;

    #[test]
    fn rejects_unknown_kinds() {
        for data in [
            json!({ "private": "value" }),
            json!([1, true]),
            json!("text"),
            json!(42),
            json!(null),
        ] {
            assert!(serde_json::from_value::<Output>(unknown(data.clone())).is_err());
            assert!(serde_json::from_value::<crate::dispatch::Output>(unknown(data)).is_err());
        }
    }

    #[test]
    fn rejects_nested_unknown_results() {
        let values = [
            json!({ "kind": "plugin_transaction", "data": [{ "kind": "browser_completed" }, unknown(json!({}))] }),
            json!({ "kind": "dispatch", "data": unknown(json!({})) }),
            json!({ "kind": "dispatch", "data": { "kind": "result", "data": { "kind": "completed", "data": { "Ok": unknown(json!({})) } } } }),
            json!({ "kind": "request_outcome", "data": { "id": RequestId::new(), "outcome": { "kind": "completed", "data": { "Ok": { "kind": "plugin_transaction", "data": [{ "kind": "dispatch", "data": unknown(json!({})) }] } } } } }),
        ];
        for value in values {
            assert!(serde_json::from_value::<Output>(value).is_err());
        }
    }

    #[test]
    fn round_trips_results_and_faults() {
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
            assert_eq!(serde_json::from_value::<Output>(encoded).unwrap(), output);
        }
    }

    #[test]
    fn malformed_known_kinds_remain_errors() {
        for value in [
            json!({ "kind": "file_transfer_cancelled", "data": {} }),
            json!({ "kind": "file_transfer_cancelled", "data": { "stream": 1 } }),
            json!({ "kind": "plugin_transaction", "data": {} }),
            json!({ "kind": "browser_completed", "data": "invalid" }),
            json!({ "data": {} }),
            json!({ "kind": 1, "data": {} }),
        ] {
            assert!(serde_json::from_value::<Output>(value).is_err());
        }
        for value in [
            json!({ "kind": "published", "data": {} }),
            json!({ "kind": "jobs", "data": "invalid" }),
            json!({ "kind": "removed", "data": "invalid" }),
        ] {
            assert!(serde_json::from_value::<crate::dispatch::Output>(value).is_err());
        }
    }
}
