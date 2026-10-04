use super::*;
use core::prelude::v1::test;

#[test]
fn steps_exact_integers() {
    assert_eq!(
        stepped("9007199254740993", StepAction::Increment).as_deref(),
        Some("9007199254740994")
    );
    assert_eq!(
        stepped("9007199254740993", StepAction::Decrement).as_deref(),
        Some("9007199254740992")
    );
    assert_eq!(stepped("18446744073709551615", StepAction::Increment), None);
    assert_eq!(stepped("-9223372036854775808", StepAction::Decrement), None);
}

#[test]
fn steps_empty_and_fractional_values() {
    assert_eq!(stepped("", StepAction::Increment).as_deref(), Some("1"));
    assert_eq!(stepped(" ", StepAction::Decrement).as_deref(), Some("-1"));
    assert_eq!(
        stepped("1.5", StepAction::Increment).as_deref(),
        Some("2.5")
    );
    assert_eq!(stepped("invalid", StepAction::Increment), None);
    assert_eq!(stepped("true", StepAction::Increment), None);
    assert_eq!(stepped("1e999", StepAction::Increment), None);
}

#[gpui::test]
fn follows_model_capabilities(cx: &mut TestAppContext) {
    use sailry_protocol::{
        Effort,
        plugin::models::{Catalog, Kind as ModelKind, Model},
    };
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| crate::model_picker::Picker::new());
    visual.update(|window, cx| {
        let spec: Field = serde_json::from_value(serde_json::json!({"type":"string","x-sailry-model-effort":"model"})).unwrap();
        let state: State = serde_json::from_value(serde_json::json!({
            "package":{"name":"fixture","version":"1.0.0","digest":"a".repeat(64),"settings_revision":0},
            "values":{"effort":"xhigh"},"configured":[],"keepable":[],"ready":true
        })).unwrap();
        let catalog = Catalog { models: vec![
            Model { default:false, reasoning:true, kind:ModelKind::Provider, id:"first/luna".into(), provider:"Provider".into(), model:"luna".into(),
                efforts:vec![Effort::XHigh, Effort::Max], default_effort:Effort::XHigh },
            Model { default:false, reasoning:true, kind:ModelKind::Provider, id:"second/other".into(), provider:"Provider".into(), model:"other".into(),
                efforts:vec![Effort::High], default_effort:Effort::High },
        ] };
        let field = cx.new(|cx| Setting::new("effort".into(), spec, false, Data { state: &state, models: &catalog, labels: &Default::default() }, window, cx));
        field.update(cx, |field, cx| {
            field.sync_effort(Some("first/luna"), &catalog, window, cx);
            assert!(matches!(field.value(cx).unwrap(), Value::Public(Some(value)) if value == Json::String("xhigh".into())));
            field.sync_effort(Some("second/other"), &catalog, window, cx);
            assert!(matches!(field.value(cx).unwrap(), Value::Public(Some(value)) if value == Json::String(String::new())));
            let Control::Choice(choice) = &field.control else { unreachable!() };
            choice.update(cx, |choice, cx| choice.set_selected_value(&Json::String("high".into()), window, cx));
            field.sync_effort(Some("second/other"), &catalog, window, cx);
            assert!(matches!(field.value(cx).unwrap(), Value::Public(Some(value)) if value == Json::String("high".into())));
            field.sync_effort(None, &catalog, window, cx);
            assert!(matches!(field.value(cx).unwrap(), Value::Public(Some(value)) if value == Json::String(String::new())));
        });
    });
}

#[gpui::test]
fn preserves_saved_choices(cx: &mut TestAppContext) {
    use sailry_protocol::{
        Effort,
        plugin::models::{Catalog, Kind as ModelKind, Model},
    };
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| crate::model_picker::Picker::new());
    visual.update(|window, cx| {
        let spec: Field = serde_json::from_value(serde_json::json!({"type":"string","x-sailry-model-effort":"model","default":"none"})).unwrap();
        let catalog = Catalog { models: vec![Model { default:false, reasoning:true, kind:ModelKind::Provider, id:"provider/model".into(), provider:"Provider".into(), model:"model".into(), efforts:vec![Effort::High, Effort::Disabled], default_effort:Effort::High }] };
        for saved in ["none", "high", ""] {
            let state: State = serde_json::from_value(serde_json::json!({
                "package":{"name":"fixture","version":"1.0.0","digest":"a".repeat(64),"settings_revision":0},
                "values":{"effort":saved},"configured":[],"keepable":[],"ready":true
            })).unwrap();
            let field = cx.new(|cx| Setting::new("effort".into(), spec.clone(), false, Data { state: &state, models: &catalog, labels: &Default::default() }, window, cx));
            field.update(cx, |field, cx| {
                if saved == "none" { field.sync_effort(None, &catalog, window, cx); }
                field.sync_effort(Some("provider/model"), &catalog, window, cx);
                assert!(matches!(field.value(cx).unwrap(), Value::Public(Some(value)) if value == Json::String(saved.into())));
            });
        }
    });
}
