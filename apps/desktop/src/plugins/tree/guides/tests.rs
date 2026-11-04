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
fn ancestry_and_spacing() {
    for (scenario, check) in [
        (
            "retains_full_sibling_paths",
            (|_scenario: &str| {
                let items = items();
                let paths = paths(&items);
                assert_eq!(paths["root"], [false], "retains full sibling paths");
                assert_eq!(
                    paths["leaf-first"],
                    [false, true, true, true],
                    "retains full sibling paths"
                );
                assert_eq!(
                    paths["leaf-last"],
                    [false, true, true, false],
                    "retains full sibling paths"
                );
                assert_eq!(
                    paths["last-leaf"],
                    [false, false, false],
                    "retains full sibling paths"
                );
                let mut collapsed = items.clone();
                collapsed[0].expanded = false;
                assert_eq!(
                    super::paths(&collapsed),
                    paths,
                    "retains full sibling paths"
                );
            }) as fn(&str),
        ),
        (
            "continues_ancestors_and_stops_last_siblings",
            (|_scenario: &str| {
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
                    ],
                    "continues ancestors and stops last siblings"
                );
                assert_eq!(
                    columns(&[false, false, false], 0),
                    [Column {
                        index: 1,
                        half: true,
                        arm: true
                    }],
                    "continues ancestors and stops last siblings"
                );
                assert_eq!(
                    columns(&[false, true, true, true], 0).last(),
                    Some(&Column {
                        index: 2,
                        half: false,
                        arm: true
                    }),
                    "continues ancestors and stops last siblings"
                );
            }),
        ),
        (
            "skips_hidden_groups_and_flat_roots",
            (|_scenario: &str| {
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
                    ],
                    "skips hidden groups and flat roots"
                );
                assert!(
                    columns(&[true], 0).is_empty(),
                    "skips hidden groups and flat roots"
                );
                assert!(
                    columns(&[false, false], 1).is_empty(),
                    "skips hidden groups and flat roots"
                );
                assert!(
                    columns(&[false, true], 4).is_empty(),
                    "skips hidden groups and flat roots"
                );
            }),
        ),
        (
            "uses_fixed_spacing_within_rows",
            (|_scenario: &str| {
                assert_eq!(
                    pattern(px(28.)),
                    Some([px(2.), px(2.)]),
                    "uses fixed spacing within rows"
                );
                assert_eq!(
                    pattern(px(56.)),
                    Some([px(4.), px(4.)]),
                    "uses fixed spacing within rows"
                );
                assert_eq!(pattern(px(0.)), None, "uses fixed spacing within rows");
                for height in [px(28.), px(32.), px(35.), px(56.)] {
                    let [dash, gap] = pattern(height).unwrap();
                    assert_eq!(dash, gap, "uses fixed spacing within rows");
                    assert!(
                        (height / (dash + gap) - 7.).abs() < 0.00001,
                        "uses fixed spacing within rows"
                    );
                }
            }),
        ),
        (
            "joins_half_rows_on_dash_ends",
            (|_scenario: &str| {
                for height in [px(28.), px(32.), px(35.), px(56.)] {
                    let [dash, gap] = pattern(height).unwrap();
                    let middle = height / 2.;
                    let last_dash = (dash + gap) * 3.;
                    assert!(
                        (f32::from(middle - last_dash - dash)).abs() < 0.00001,
                        "joins half rows on dash ends"
                    );
                    assert!(
                        (f32::from((dash + gap) * 7. - height)).abs() < 0.00001,
                        "joins half rows on dash ends"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
