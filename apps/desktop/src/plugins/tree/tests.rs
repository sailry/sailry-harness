use super::*;
use core::prelude::v1::test;

#[test]
fn validates_nested_items() {
    for items in [
        "{]",
        "{}",
        r#"[{"id":"leaf","label":"Leaf","unknown":true}]"#,
        r#"[{"id":"folder","label":"Folder","children":[{"id":"leaf","label":"Leaf","unknown":true}]}]"#,
        r#"[{"id":"leaf","label":"Leaf","menu":[{"label":"Open","enabled":"true"}]}]"#,
        r#"[{"id":"leaf","label":"Leaf","decoration":{"check":{"id":"check","label":"Stage","checked":"false"}}}]"#,
    ] {
        assert!(serde_json::from_value::<Props>(json!({"items":items})).is_err());
    }
}

#[test]
fn leaves_guides_opt_in() {
    let defaults: Props = serde_json::from_value(json!({"items": "[]"})).unwrap();
    assert!(!defaults.guides);
    let enabled: Props = serde_json::from_value(json!({"items": "[]", "guides": true})).unwrap();
    assert!(enabled.guides);
}
