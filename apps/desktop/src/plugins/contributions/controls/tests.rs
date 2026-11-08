use super::*;
use core::prelude::v1::test;
use serde_json::json;

#[test]
fn menus_preserve_published_titles() {
    let mut entry = Entry {
        key: Key {
            node: NodeId([1; 32]),
            worktree: None,
            session: None,
            package: Reference {
                name: "fixture".into(),
                digest: "captured".into(),
                settings_revision: 1,
            },
            id: "location".into(),
        },
        declaration: serde_json::from_value(json!({
            "id": "location", "slot": "composer", "kind": "menu",
            "label": {"label": "Location"}
        }))
        .unwrap(),
        state: serde_json::from_value(json!({
            "id": "location", "label": {"label": "Project"},
            "value": "main", "choices": [
                {"id": "main", "label": {"label": "Main worktree (main)"}}
            ]
        }))
        .unwrap(),
        retry: false,
    };
    assert_eq!(entry.text(), "Project");
    entry.declaration.kind = Kind::Select;
    assert_eq!(entry.text(), "Main worktree (main)");
    entry.declaration.kind = Kind::Menu;
    entry.state.label = None;
    assert_eq!(entry.text(), "Main worktree (main)");
    entry.state.value = serde_json::Value::Null;
    assert_eq!(entry.text(), "Location");
}
