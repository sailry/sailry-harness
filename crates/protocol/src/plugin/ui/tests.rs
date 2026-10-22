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
fn popovers_keep_boolean_state_and_supported_slots() {
    let mut entry = contribution(Slot::Context, Kind::Popover);
    assert!(entry.valid());
    for value in [json!(true), json!(false), Value::Null] {
        let state: State = serde_json::from_value(json!({"id":"action","value":value})).unwrap();
        assert!(state.valid(&entry));
    }
    let state: State = serde_json::from_value(json!({"id":"action","value":"open"})).unwrap();
    assert!(!state.valid(&entry));
    entry.slot = Slot::Composer;
    assert!(entry.valid());
    entry.slot = Slot::Status;
    assert!(entry.valid());
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
    assert!(status.valid(&entry));
    entry.slot = Slot::Project;
    assert!(!entry.valid());
    entry.slot = Slot::Context;
    let mut other = entry.clone();
    other.id = "other".into();
    assert!(!valid(&[entry, other]));
}

#[test]
fn commands_share_control_state_and_validate_routes() {
    let mut entry = contribution(Slot::Commands, Kind::Button);
    assert!(!entry.valid());
    entry.command = Some(Command {
        name: "task".into(),
        kind: CommandKind::Message,
    });
    assert!(entry.valid());
    for name in ["/task", "Task", "task name", "", "plan", "model"] {
        entry.command.as_mut().unwrap().name = name.into();
        assert!(!entry.valid());
    }
    entry.command.as_mut().unwrap().name = "task".into();
    entry.slot = Slot::Composer;
    assert!(!entry.valid());
    entry.command.as_mut().unwrap().kind = CommandKind::Invoke;
    assert!(entry.valid());
    entry.kind = Kind::Picker;
    assert!(entry.valid());
    entry.slot = Slot::Project;
    assert!(!entry.valid());
}

#[test]
fn accepts_package_defined_intent_keys() {
    let mut entry = contribution(Slot::Context, Kind::Button);
    entry.intent = Some("example.details".into());
    assert!(entry.valid());
    assert_eq!(
        serde_json::to_value(&entry).unwrap()["intent"],
        "example.details"
    );
    for key in ["", "show details", "/details"] {
        entry.intent = Some(key.into());
        assert!(!entry.valid());
    }
}

#[test]
fn choices_default_to_enabled() {
    let choice: Choice = serde_json::from_value(json!({
        "id": "available", "label": { "label": "Available" }
    }))
    .unwrap();
    assert!(choice.enabled);
    let choice: Choice = serde_json::from_value(json!({
        "id": "unavailable", "label": { "label": "Unavailable" }, "enabled": false
    }))
    .unwrap();
    assert!(!choice.enabled);
    assert_eq!(serde_json::to_value(choice).unwrap()["enabled"], false);
}

#[test]
fn project_menu_accepts_native_actions() {
    for kind in [Kind::Button, Kind::Menu] {
        assert!(contribution(Slot::ProjectMenu, kind).valid());
    }
    for kind in [Kind::Picker, Kind::Select, Kind::Toggle, Kind::Metric] {
        assert!(!contribution(Slot::ProjectMenu, kind).valid());
    }
    assert!(contribution(Slot::Project, Kind::Picker).valid());
}

#[test]
fn indicators_bound_progress_and_optional_interaction() {
    let mut entry = contribution(Slot::Composer, Kind::Indicator);
    assert!(entry.valid());
    entry.handler = None;
    assert!(entry.valid());
    let mut state = State::pending(&entry);
    for percent in [serde_json::Value::Null, json!(0), json!(120)] {
        state.value = json!({"percent":percent,"loading":true,"tone":"warning","hint":"Context"});
        assert!(state.valid(&entry));
    }
    state.value["open"] = true.into();
    assert!(!state.valid(&entry));
    entry.handler = Some("context".into());
    assert!(state.valid(&entry));
    let other = contribution(Slot::Context, Kind::Popover);
    assert!(!valid(&[entry.clone(), other]));
    state.value["percent"] = json!(-1);
    assert!(!state.valid(&entry));
    state.value = json!({"percent":0,"tone":"danger","hint":"Context","command":"run"});
    assert!(!state.valid(&entry));
    entry.slot = Slot::Statistics;
    assert!(!entry.valid());
}

#[test]
fn model_controls_share_labeled_segments() {
    let mut entry = contribution(Slot::Model, Kind::Toggle);
    assert!(entry.valid());
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
    assert!(state.valid(&entry));
    entry.slot = Slot::Context;
    assert!(!state.valid(&entry));
    assert_eq!(serde_json::to_value(Slot::Model).unwrap(), "model");
}

#[test]
fn metrics_use_passive_footer_slots() {
    let mut entry = contribution(Slot::Footer, Kind::Metric);
    assert!(!entry.valid());
    entry.handler = None;
    for slot in [Slot::Footer, Slot::Statistics] {
        entry.slot = slot;
        assert!(entry.valid());
    }
    entry.slot = Slot::Composer;
    assert!(!entry.valid());
    entry.slot = Slot::Footer;
    entry.kind = Kind::Button;
    entry.handler = Some("action".into());
    assert!(!entry.valid());
    assert_eq!(serde_json::to_value(Slot::Footer).unwrap(), "footer");
}

#[test]
fn statistics_allow_one_plugin_detail_surface() {
    let entry = contribution(Slot::Statistics, Kind::Metric);
    assert!(entry.valid());
    assert!(super::valid(&[entry.clone()]));
    let mut duplicate = entry.clone();
    duplicate.id = "other".into();
    assert!(!super::valid(&[entry, duplicate]));
    for slot in [Slot::Footer, Slot::Provider] {
        assert!(!contribution(slot, Kind::Metric).valid());
    }
}

#[test]
fn details_accept_bounded_optional_progress() {
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
    assert!(state.valid(&entry));
    let encoded = serde_json::to_value(&state.details).unwrap();
    assert!(encoded[0].get("progress").is_none());
    assert_eq!(encoded[1]["progress"], 50.5);
    for value in [json!(0), json!(12.5), json!(100)] {
        state.details[1].progress = Some(serde_json::from_value(value).unwrap());
        assert!(state.valid(&entry));
    }
    for value in [json!(-1), json!(100.01)] {
        state.details[1].progress = Some(serde_json::from_value(value).unwrap());
        assert!(!state.valid(&entry));
    }
    for value in [json!("50"), json!(true)] {
        assert!(
            serde_json::from_value::<Detail>(
                json!({"label":{"label":"Window"},"value":"50%","progress":value})
            )
            .is_err()
        );
    }
    entry.kind = Kind::Button;
    assert!(!state.valid(&entry));
}

mod metric_width {
    use super::*;

    fn metric() -> Contribution {
        let mut entry = contribution(Slot::Footer, Kind::Metric);
        entry.handler = None;
        entry
    }

    #[test]
    fn accepts_bounded_metric_widths() {
        let mut entry = metric();
        for width in [None, Some(240), Some(320), Some(480)] {
            entry.width = width;
            assert!(entry.valid());
        }
        for width in [0, 239, 481, u16::MAX] {
            entry.width = Some(width);
            assert!(!entry.valid());
        }
    }

    #[test]
    fn rejects_width_on_other_kinds() {
        let mut entry = contribution(Slot::Status, Kind::Popover);
        assert!(entry.valid());
        entry.width = Some(320);
        assert!(!entry.valid());
    }

    #[test]
    fn round_trips_on_the_wire() {
        let mut entry = metric();
        assert!(serde_json::to_value(&entry).unwrap().get("width").is_none());
        entry.width = Some(360);
        let encoded = serde_json::to_value(&entry).unwrap();
        assert_eq!(encoded["width"], 360);
        assert_eq!(
            serde_json::from_value::<Contribution>(encoded).unwrap(),
            entry
        );
        assert!(
            serde_json::from_value::<Contribution>(json!({
                "id": "action",
                "slot": "footer",
                "kind": "metric",
                "label": { "label": "Action" },
                "width": -1
            }))
            .is_err()
        );
    }
}

#[test]
fn provider_metrics_require_scoped_public_identity() {
    let mut entry = contribution(Slot::Provider, Kind::Metric);
    entry.handler = None;
    assert!(entry.valid());
    let mut state = State::pending(&entry);
    assert!(!state.valid(&entry));
    state.scope = Some(ProviderScope {
        provider: crate::ProviderId::new(),
        revision: "9007199254740993:7".into(),
        credential: crate::CredentialRef {
            node: crate::NodeId([1; 32]),
            id: crate::CredentialId::new(),
        },
    });
    state.value = json!("25%");
    assert!(state.valid(&entry));
    for revision in ["1", "1:", ":2", "1:2:3", "+1:2", "1:-2", "01:2"] {
        state.scope.as_mut().unwrap().revision = revision.into();
        assert!(!state.valid(&entry));
    }
    state.scope.as_mut().unwrap().revision = "9007199254740993:7".into();
    entry.slot = Slot::Footer;
    assert!(!state.valid(&entry));
    entry.slot = Slot::Provider;
    entry.kind = Kind::Button;
    entry.handler = Some("run".into());
    assert!(!entry.valid());
}

#[test]
fn header_supports_scoped_hover_panels() {
    let mut entry = contribution(Slot::Header, Kind::Popover);
    entry.width = Some(480);
    entry.hover = Some(Hover {
        open_ms: 120,
        close_ms: 230,
    });
    assert!(entry.valid());
    entry.slot = Slot::Composer;
    assert!(!entry.valid());
    entry.slot = Slot::Header;
    entry.kind = Kind::Button;
    assert!(!entry.valid());
    entry.kind = Kind::Popover;
    entry.hover.as_mut().unwrap().close_ms = 2001;
    assert!(!entry.valid());
}
