use super::*;
use core::prelude::v1::test;

fn items() -> Vec<Item> {
    serde_json::from_value(json!([{
        "id": "root", "label": "Root", "expanded": true, "children": [
            {"id": "first", "label": "First", "expanded": true, "children": [
                {"id": "nested", "label": "Nested", "expanded": true, "children": [
                    {"id": "leaf-first", "label": "First leaf"},
                    {"id": "leaf-last", "label": "Last leaf"}
                ]},
                {"id": "tail", "label": "Tail"}
            ]},
            {"id": "last", "label": "Last", "expanded": true, "children": [
                {"id": "last-leaf", "label": "Last leaf"}
            ]}
        ]
    }]))
    .unwrap()
}

#[test]
fn retains_full_sibling_paths() {
    let items = items();
    let paths = paths(&items);
    assert_eq!(paths["root"], [false]);
    assert_eq!(paths["leaf-first"], [false, true, true, true]);
    assert_eq!(paths["leaf-last"], [false, true, true, false]);
    assert_eq!(paths["last-leaf"], [false, false, false]);
    let mut collapsed = items.clone();
    collapsed[0].expanded = false;
    assert_eq!(super::paths(&collapsed), paths);
}

#[test]
fn continues_ancestors_and_stops_last_siblings() {
    assert_eq!(
        columns(&[false, true, true, false], 0),
        [
            Column {
                index: 0,
                half: false,
                arm: false
            },
            Column {
                index: 1,
                half: false,
                arm: false
            },
            Column {
                index: 2,
                half: true,
                arm: true
            },
        ]
    );
    assert_eq!(
        columns(&[false, false, false], 0),
        [Column {
            index: 1,
            half: true,
            arm: true
        }]
    );
    assert_eq!(
        columns(&[false, true, true, true], 0).last(),
        Some(&Column {
            index: 2,
            half: false,
            arm: true
        })
    );
}

#[test]
fn skips_hidden_groups_and_flat_roots() {
    assert_eq!(
        columns(&[false, true, true, false], 1),
        [
            Column {
                index: 0,
                half: false,
                arm: false
            },
            Column {
                index: 1,
                half: true,
                arm: true
            },
        ]
    );
    assert!(columns(&[true], 0).is_empty());
    assert!(columns(&[false, false], 1).is_empty());
    assert!(columns(&[false, true], 4).is_empty());
}
