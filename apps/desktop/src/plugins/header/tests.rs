use super::*;
use core::prelude::v1::test;
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
