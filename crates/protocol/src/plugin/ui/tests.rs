use super::*;
use serde_json::json;

fn contribution(slot: Slot, kind: Kind) -> Contribution {
    serde_json::from_value(json!({
        "id": "action",
        "slot": slot,
        "kind": kind,
        "label": { "label": "Action" },
        "handler": "action"
    }))
    .unwrap()
}

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "popovers_keep_boolean_state_and_supported_slots",
            (|scenario: &'static str| {
                let mut entry = contribution(Slot::Context, Kind::Popover);
                assert!(entry.valid(), "{scenario}");
                for value in [json!(true), json!(false), Value::Null] {
                    let state: State =
                        serde_json::from_value(json!({"id":"action","value":value})).unwrap();
                    assert!(state.valid(&entry), "{scenario}");
                }
                let state: State =
                    serde_json::from_value(json!({"id":"action","value":"open"})).unwrap();
                assert!(!state.valid(&entry), "{scenario}");
                entry.slot = Slot::Composer;
                assert!(entry.valid(), "{scenario}");
                entry.slot = Slot::Status;
                assert!(entry.valid(), "{scenario}");
                let mut status = State::pending(&entry);
                status.value = json!(true);
                status.segments = vec![Segment {
                    label: super::super::desktop::Navigation {
                        label: "Running".into(),
                        icon: None,
                        locales: Default::default(),
                    },
                    tone: SegmentTone::Success,
                }];
                assert!(status.valid(&entry), "{scenario}");
                entry.slot = Slot::Project;
                assert!(!entry.valid(), "{scenario}");
                entry.slot = Slot::Context;
                let mut other = entry.clone();
                other.id = "other".into();
                assert!(!valid(&[entry, other]), "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "commands_share_control_state_and_validate_routes",
            (|scenario: &'static str| {
                let mut entry = contribution(Slot::Commands, Kind::Button);
                assert!(!entry.valid(), "{scenario}");
                entry.command = Some(Command {
                    name: "task".into(),
                    kind: CommandKind::Message,
                });
                assert!(entry.valid(), "{scenario}");
                for name in ["/task", "Task", "task name", "", "plan", "model"] {
                    entry.command.as_mut().unwrap().name = name.into();
                    assert!(!entry.valid(), "{scenario}");
                }
                entry.command.as_mut().unwrap().name = "task".into();
                entry.slot = Slot::Composer;
                assert!(!entry.valid(), "{scenario}");
                entry.command.as_mut().unwrap().kind = CommandKind::Invoke;
                assert!(entry.valid(), "{scenario}");
                entry.kind = Kind::Picker;
                assert!(entry.valid(), "{scenario}");
                entry.slot = Slot::Project;
                assert!(!entry.valid(), "{scenario}");
            }),
        ),
        (
            "accepts_package_defined_intent_keys",
            (|scenario: &'static str| {
                let mut entry = contribution(Slot::Context, Kind::Button);
                entry.intent = Some("example.details".into());
                assert!(entry.valid(), "{scenario}");
                assert_eq!(
                    serde_json::to_value(&entry).unwrap()["intent"],
                    "example.details",
                    "{scenario}"
                );
                for key in ["", "show details", "/details"] {
                    entry.intent = Some(key.into());
                    assert!(!entry.valid(), "{scenario}");
                }
            }),
        ),
        (
            "choices_default_to_enabled",
            (|scenario: &'static str| {
                let choice: Choice = serde_json::from_value(json!({
                    "id": "available", "label": { "label": "Available" }
                }))
                .unwrap();
                assert!(choice.enabled, "{scenario}");
                let choice: Choice = serde_json::from_value(json!({
                    "id": "unavailable", "label": { "label": "Unavailable" }, "enabled": false
                }))
                .unwrap();
                assert!(!choice.enabled, "{scenario}");
                assert_eq!(
                    serde_json::to_value(choice).unwrap()["enabled"],
                    false,
                    "{scenario}"
                );
            }),
        ),
        (
            "project_menu_accepts_native_actions",
            (|scenario: &'static str| {
                for kind in [Kind::Button, Kind::Menu] {
                    assert!(contribution(Slot::ProjectMenu, kind).valid(), "{scenario}");
                }
                for kind in [Kind::Picker, Kind::Select, Kind::Toggle, Kind::Metric] {
                    assert!(!contribution(Slot::ProjectMenu, kind).valid(), "{scenario}");
                }
                assert!(
                    contribution(Slot::Project, Kind::Picker).valid(),
                    "{scenario}"
                );
            }),
        ),
        (
            "indicators_bound_progress_and_optional_interaction",
            (|scenario: &'static str| {
                let mut entry = contribution(Slot::Composer, Kind::Indicator);
                assert!(entry.valid(), "{scenario}");
                entry.handler = None;
                assert!(entry.valid(), "{scenario}");
                let mut state = State::pending(&entry);
                for percent in [serde_json::Value::Null, json!(0), json!(120)] {
                    state.value =
                        json!({"percent":percent,"loading":true,"tone":"warning","hint":"Context"});
                    assert!(state.valid(&entry), "{scenario}");
                }
                state.value["open"] = true.into();
                assert!(!state.valid(&entry), "{scenario}");
                entry.handler = Some("context".into());
                assert!(state.valid(&entry), "{scenario}");
                let other = contribution(Slot::Context, Kind::Popover);
                assert!(!valid(&[entry.clone(), other]), "{scenario}");
                state.value["percent"] = json!(-1);
                assert!(!state.valid(&entry), "{scenario}");
                state.value = json!({"percent":0,"tone":"danger","hint":"Context","command":"run"});
                assert!(!state.valid(&entry), "{scenario}");
                entry.slot = Slot::Statistics;
                assert!(!entry.valid(), "{scenario}");
            }),
        ),
        (
            "model_controls_share_labeled_segments",
            (|scenario: &'static str| {
                let mut entry = contribution(Slot::Model, Kind::Toggle);
                assert!(entry.valid(), "{scenario}");
                let mut state = State::pending(&entry);
                state.value = true.into();
                state.segments = vec![Segment {
                    label: super::super::desktop::Navigation {
                        label: "Active".into(),
                        icon: None,
                        locales: Default::default(),
                    },
                    tone: SegmentTone::Accent,
                }];
                assert!(state.valid(&entry), "{scenario}");
                entry.slot = Slot::Context;
                assert!(!state.valid(&entry), "{scenario}");
                assert_eq!(
                    serde_json::to_value(Slot::Model).unwrap(),
                    "model",
                    "{scenario}"
                );
            }),
        ),
        (
            "metrics_use_passive_footer_slots",
            (|scenario: &'static str| {
                let mut entry = contribution(Slot::Footer, Kind::Metric);
                assert!(!entry.valid(), "{scenario}");
                entry.handler = None;
                for slot in [Slot::Footer, Slot::Statistics] {
                    entry.slot = slot;
                    assert!(entry.valid(), "{scenario}");
                }
                entry.slot = Slot::Composer;
                assert!(!entry.valid(), "{scenario}");
                entry.slot = Slot::Footer;
                entry.kind = Kind::Button;
                entry.handler = Some("action".into());
                assert!(!entry.valid(), "{scenario}");
                assert_eq!(
                    serde_json::to_value(Slot::Footer).unwrap(),
                    "footer",
                    "{scenario}"
                );
            }),
        ),
        (
            "statistics_allow_one_plugin_detail_surface",
            (|scenario: &'static str| {
                let entry = contribution(Slot::Statistics, Kind::Metric);
                assert!(entry.valid(), "{scenario}");
                assert!(super::valid(std::slice::from_ref(&entry)), "{scenario}");
                let mut duplicate = entry.clone();
                duplicate.id = "other".into();
                assert!(!super::valid(&[entry, duplicate]), "{scenario}");
                for slot in [Slot::Footer, Slot::Provider] {
                    assert!(!contribution(slot, Kind::Metric).valid(), "{scenario}");
                }
            }),
        ),
        (
            "details_accept_bounded_optional_progress",
            (|scenario: &'static str| {
                let mut entry = contribution(Slot::Footer, Kind::Metric);
                entry.handler = None;
                let mut state = State::pending(&entry);
                state.value = json!("12");
                state.details = serde_json::from_value(json!([
                    {"label":{"label":"Balance"},"value":"12"},
                    {"label":{"label":"Short window"},"value":"50.5%","progress":50.5},
                    {"label":{"label":"Long window"},"value":"100%","progress":100}
                ]))
                .unwrap();
                assert!(state.valid(&entry), "{scenario}");
                let encoded = serde_json::to_value(&state.details).unwrap();
                assert!(encoded[0].get("progress").is_none(), "{scenario}");
                assert_eq!(encoded[1]["progress"], 50.5, "{scenario}");
                for value in [json!(0), json!(12.5), json!(100)] {
                    state.details[1].progress = Some(serde_json::from_value(value).unwrap());
                    assert!(state.valid(&entry), "{scenario}");
                }
                for value in [json!(-1), json!(100.01)] {
                    state.details[1].progress = Some(serde_json::from_value(value).unwrap());
                    assert!(!state.valid(&entry), "{scenario}");
                }
                for value in [json!("50"), json!(true)] {
                    assert!(
                        serde_json::from_value::<Detail>(
                            json!({"label":{"label":"Window"},"value":"50%","progress":value})
                        )
                        .is_err(),
                        "{scenario}"
                    );
                }
                entry.kind = Kind::Button;
                assert!(!state.valid(&entry), "{scenario}");
            }),
        ),
        (
            "provider_metrics_require_scoped_public_identity",
            (|scenario: &'static str| {
                let mut entry = contribution(Slot::Provider, Kind::Metric);
                entry.handler = None;
                assert!(entry.valid(), "{scenario}");
                let mut state = State::pending(&entry);
                assert!(!state.valid(&entry), "{scenario}");
                state.scope = Some(ProviderScope {
                    provider: crate::ProviderId::new(),
                    revision: "9007199254740993:7".into(),
                    credential: crate::CredentialRef {
                        node: crate::NodeId([1; 32]),
                        id: crate::CredentialId::new(),
                    },
                });
                state.value = json!("25%");
                assert!(state.valid(&entry), "{scenario}");
                for revision in ["1", "1:", ":2", "1:2:3", "+1:2", "1:-2", "01:2"] {
                    state.scope.as_mut().unwrap().revision = revision.into();
                    assert!(!state.valid(&entry), "{scenario}");
                }
                state.scope.as_mut().unwrap().revision = "9007199254740993:7".into();
                entry.slot = Slot::Footer;
                assert!(!state.valid(&entry), "{scenario}");
                entry.slot = Slot::Provider;
                entry.kind = Kind::Button;
                entry.handler = Some("run".into());
                assert!(!entry.valid(), "{scenario}");
            }),
        ),
        (
            "header_supports_scoped_hover_panels",
            (|scenario: &'static str| {
                let mut entry = contribution(Slot::Header, Kind::Popover);
                entry.width = Some(480);
                entry.hover = Some(Hover {
                    open_ms: 120,
                    close_ms: 230,
                });
                assert!(entry.valid(), "{scenario}");
                entry.slot = Slot::Composer;
                assert!(!entry.valid(), "{scenario}");
                entry.slot = Slot::Header;
                entry.kind = Kind::Button;
                assert!(!entry.valid(), "{scenario}");
                entry.kind = Kind::Popover;
                entry.hover.as_mut().unwrap().close_ms = 2001;
                assert!(!entry.valid(), "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

mod status {
    use super::*;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "details_keep_existing_bounds",
                (|scenario: &'static str| {
                    for kind in [Kind::Button, Kind::Popover] {
                        let mut entry = contribution(Slot::Status, kind);
                        let mut state = State::pending(&entry);
                        state.details = serde_json::from_value(json!([
                            {"label": {"label": "Files"}, "value": "3"},
                            {"label": {"label": "Progress"}, "value": "50%", "progress": 50}
                        ]))
                        .unwrap();
                        assert!(state.valid(&entry), "{scenario}");
                        assert!(super::super::status::valid(&state, &entry), "{scenario}");
                        state.details[0].value = "x".repeat(4097);
                        assert!(!super::super::status::valid(&state, &entry), "{scenario}");
                        state.details[0].value = "3".into();
                        state.details[1].progress = Some(101.into());
                        assert!(!super::super::status::valid(&state, &entry), "{scenario}");
                        state.details[1].progress = Some(50.into());
                        entry.slot = Slot::Context;
                        assert!(!state.valid(&entry), "{scenario}");
                    }
                }) as fn(&'static str),
            ),
            (
                "rejects_controller_owned_state",
                (|scenario: &'static str| {
                    for kind in [Kind::Button, Kind::Popover] {
                        let entry = contribution(Slot::Status, kind);
                        let state = State::pending(&entry);
                        assert!(super::super::status::valid(&state, &entry), "{scenario}");
                        let mut changed = state.clone();
                        changed.reply_to = Some(1);
                        assert!(!super::super::status::valid(&changed, &entry), "{scenario}");
                        for value in [json!(true), json!("Tooltip"), json!(5)] {
                            let mut changed = state.clone();
                            changed.value = value;
                            assert!(!super::super::status::valid(&changed, &entry), "{scenario}");
                        }
                        let mut changed = state.clone();
                        changed.choices = Some(vec![]);
                        assert!(!super::super::status::valid(&changed, &entry), "{scenario}");
                        let mut changed = state.clone();
                        changed.scope = Some(ProviderScope {
                            provider: crate::ProviderId::new(),
                            revision: "1:1".into(),
                            credential: crate::CredentialRef {
                                node: crate::NodeId([1; 32]),
                                id: crate::CredentialId::new(),
                            },
                        });
                        assert!(!super::super::status::valid(&changed, &entry), "{scenario}");
                        let mut changed = state.clone();
                        changed.id = "another".into();
                        assert!(!super::super::status::valid(&changed, &entry), "{scenario}");
                        let mut other = entry.clone();
                        other.slot = Slot::Composer;
                        assert!(state.valid(&other), "{scenario}");
                        assert!(!super::super::status::valid(&state, &other), "{scenario}");
                    }
                }),
            ),
            (
                "scope_requires_a_captured_resource",
                (|scenario: &'static str| {
                    use super::super::status::Scope;
                    let mut scope = Scope {
                        worktree: None,
                        session: None,
                    };
                    assert!(!scope.valid(), "{scenario}");
                    scope.worktree = Some(crate::WorktreeId::new());
                    assert!(scope.valid(), "{scenario}");
                    scope.session = Some(crate::SessionId::new());
                    assert!(scope.valid(), "{scenario}");
                    scope.worktree = None;
                    assert!(scope.valid(), "{scenario}");
                    assert!(
                        serde_json::from_value::<Scope>(json!({
                            "session": scope.session,
                            "provider": crate::ProviderId::new()
                        }))
                        .is_err(),
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

mod metric_width {
    use super::*;

    fn metric() -> Contribution {
        let mut entry = contribution(Slot::Footer, Kind::Metric);
        entry.handler = None;
        entry
    }

    #[test]
    fn bounds_widths_and_preserves_wire() {
        for (case, mut entry, widths, valid) in [
            (
                "metric",
                metric(),
                vec![None, Some(240), Some(320), Some(480)],
                true,
            ),
            (
                "metric overflow",
                metric(),
                vec![Some(0), Some(239), Some(481), Some(u16::MAX)],
                false,
            ),
            (
                "popover default",
                contribution(Slot::Status, Kind::Popover),
                vec![None],
                true,
            ),
            (
                "popover width",
                contribution(Slot::Status, Kind::Popover),
                vec![Some(320)],
                false,
            ),
        ] {
            for width in widths {
                entry.width = width;
                assert_eq!(entry.valid(), valid, "{case}: {width:?}");
            }
        }
        let mut entry = metric();
        assert!(
            serde_json::to_value(&entry).unwrap().get("width").is_none(),
            "default wire"
        );
        entry.width = Some(360);
        let encoded = serde_json::to_value(&entry).unwrap();
        assert_eq!(encoded["width"], 360, "explicit wire");
        assert_eq!(
            serde_json::from_value::<Contribution>(encoded).unwrap(),
            entry,
            "wire round trip"
        );
        assert!(
            serde_json::from_value::<Contribution>(json!({
                "id":"action","slot":"footer","kind":"metric","label":{"label":"Action"},"width":-1
            }))
            .is_err(),
            "negative wire width"
        );
    }
}
