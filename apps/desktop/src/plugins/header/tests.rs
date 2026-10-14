use super::*;
use crate::plugins::tests::{click, init};
use core::prelude::v1::test;
use gpui_kit::component::Root;
use sailry_link::CancellationToken;
use serde_json::json;

#[test]
fn retains_flat_schema_and_validates_menus() {
    let flat: Action = serde_json::from_value(json!({
        "id":"flat", "label":"Flat", "icon":"icons/ellipsis.svg"
    }))
    .unwrap();
    assert!(!flat.disabled);
    assert!(flat.menu.is_none());
    assert!(
        serde_json::from_value::<Action>(json!({
            "id":"more", "label":"More", "icon":"icons/ellipsis.svg",
            "menu":[{"id":"html", "label":"HTML", "unknown":true}]
        }))
        .is_err()
    );
    for (items, expected) in [
        (json!([{"id":"html", "label":"HTML", "enabled":true}]), true),
        (json!([{"id":"html", "label":" "}]), false),
        (json!([{"id":"", "label":"HTML"}]), false),
        (json!([{"id":"x".repeat(257), "label":"HTML"}]), false),
        (json!(vec![json!({"separator":true}); 257]), false),
    ] {
        let action: Action = serde_json::from_value(json!({
            "id":"more", "label":"More", "icon":"icons/ellipsis.svg", "menu":items
        }))
        .unwrap();
        assert_eq!(
            super::super::menu::valid(action.menu.as_ref().unwrap()),
            expected
        );
    }
}

#[gpui::test]
fn routes_flat_and_dropdown_selections(cx: &mut TestAppContext) {
    init(cx);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
    let content: Content = serde_json::from_value(json!({"actions":[
        {"id":"flat", "label":"Flat", "icon":"icons/ellipsis.svg"},
        {"id":"more", "label":"More", "icon":"icons/ellipsis-vertical.svg", "menu":[
            {"id":"html", "label":"HTML", "enabled":true, "checked":true, "value":{"ignored":true}},
            {"id":"blocked-leaf", "label":"Unavailable", "enabled":false},
            {"id":"image", "label":"Image", "enabled":true}
        ]},
        {"id":"blocked", "label":"Unavailable", "icon":"icons/ellipsis.svg", "disabled":true,
         "menu":[{"id":"forbidden", "label":"Forbidden", "enabled":true}]}
    ]}))
    .unwrap();
    let (_, visual) = cx.add_window_view(|window, cx| {
        let header = cx.new(|_| Header {
            content,
            trailing: false,
            filters: Default::default(),
            tabs: super::super::controls::tabs::Controls::new(CancellationToken::new()),
            actions: sender,
        });
        Root::new(header, window, cx)
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    click(visual, "flat");
    assert_eq!(receiver.try_recv().unwrap(), "flat");
    assert!(visual.debug_bounds("popup-menu").is_none());
    click(visual, "more");
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("popup-menu").is_some());
    assert!(
        receiver.try_recv().is_err(),
        "opening a menu must not emit its trigger ID"
    );
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    assert!(receiver.try_recv().is_err());
    assert!(visual.debug_bounds("popup-menu").is_none());
    click(visual, "more");
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.simulate_keystrokes("down down enter");
    visual.run_until_parked();
    assert_eq!(
        receiver.try_recv().unwrap(),
        "image",
        "disabled leaves must be skipped"
    );
    assert!(
        receiver.try_recv().is_err(),
        "selection must publish only one ID"
    );
    click(visual, "more");
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.simulate_keystrokes("down enter");
    visual.run_until_parked();
    assert_eq!(receiver.try_recv().unwrap(), "html");
    assert!(
        receiver.try_recv().is_err(),
        "leaf values must not enter header_action"
    );
    click(visual, "blocked");
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("popup-menu").is_none());
    assert!(receiver.try_recv().is_err());
    visual.update(|window, _| window.remove_window());
}

#[gpui::test]
fn segmented_filters_emit_choices(cx: &mut TestAppContext) {
    init(cx);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
    let content: Content = serde_json::from_value(json!({"filters":[{
        "id":"range", "label":"Range", "selected":"30", "primary":true, "segmented":true,
        "items":[{"id":"7", "label":"7 days"}, {"id":"30", "label":"30 days"}]
    }]}))
    .unwrap();
    let (_, visual) = cx.add_window_view(|window, cx| {
        let header = cx.new(|cx| {
            let mut header = Header {
                content,
                trailing: false,
                filters: Default::default(),
                tabs: super::super::controls::tabs::Controls::new(CancellationToken::new()),
                actions: sender,
            };
            header.sync_filters(window, cx);
            header
        });
        Root::new(header, window, cx)
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("range").is_some());
    assert!(visual.debug_bounds("range-item-30").is_some());
    click(visual, "range-item-7");
    let value: serde_json::Value = serde_json::from_str(&receiver.try_recv().unwrap()).unwrap();
    assert_eq!(value, json!({"id":"range", "value":"7"}));
    assert!(receiver.try_recv().is_err());
    visual.update(|window, _| window.remove_window());
}
