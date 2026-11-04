use super::*;
use core::prelude::v1::test;

#[test]
fn bounded_items() {
    for (scenario, check) in [
        (
            "validates_nested_items",
            (|_scenario: &str| {
                for items in [
                    "{]",
                    "{}",
                    r#"[{"id":"leaf","label":"Leaf","unknown":true}]"#,
                    r#"[{"id":"folder","label":"Folder","children":[{"id":"leaf","label":"Leaf","unknown":true}]}]"#,
                    r#"[{"id":"leaf","label":"Leaf","menu":[{"label":"Open","enabled":"true"}]}]"#,
                    r#"[{"id":"leaf","label":"Leaf","decoration":{"check":{"id":"check","label":"Stage","checked":"false"}}}]"#,
                ] {
                    assert!(
                        serde_json::from_value::<Props>(json!({"items":items})).is_err(),
                        "validates nested items"
                    );
                }
            }) as fn(&str),
        ),
        (
            "leaves_guides_opt_in",
            (|_scenario: &str| {
                let defaults: Props = serde_json::from_value(json!({"items": "[]"})).unwrap();
                assert!(!defaults.guides, "leaves guides opt in");
                let enabled: Props =
                    serde_json::from_value(json!({"items": "[]", "guides": true})).unwrap();
                assert!(enabled.guides, "leaves guides opt in");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
