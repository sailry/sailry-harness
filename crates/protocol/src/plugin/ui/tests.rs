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
fn indicators_accept_only_finite_passive_composer_state() {
    let mut entry = contribution(Slot::Composer, Kind::Indicator);
    assert!(!entry.valid());
    entry.handler = None;
    assert!(entry.valid());
    let mut state = State::pending(&entry);
    for percent in [serde_json::Value::Null, json!(0), json!(120)] {
        state.value = json!({"percent":percent,"loading":true,"tone":"warning","hint":"Context"});
        assert!(state.valid(&entry));
    }
    state.value["percent"] = json!(-1);
    assert!(!state.valid(&entry));
    state.value = json!({"percent":0,"tone":"danger","hint":"Context","command":"run"});
    assert!(!state.valid(&entry));
    entry.slot = Slot::Statistics;
    assert!(!entry.valid());
}
