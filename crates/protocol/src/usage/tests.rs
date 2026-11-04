use super::*;

fn priced() -> Cost {
    Cost {
        usd_micros: 10,
        responses: 1,
        breakdown: Some(CostBreakdown {
            input: 4,
            output: 3,
            cache_read: 2,
            cache_write: 1,
        }),
    }
}

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "adds_charges_without_inventing_breakdowns",
            (|scenario: &'static str| {
                let reported = Cost {
                    usd_micros: 20,
                    responses: 1,
                    breakdown: None,
                };
                for (case, left, right, expected) in [
                    (
                        "disjoint",
                        priced(),
                        priced(),
                        Some(Cost {
                            usd_micros: 20,
                            responses: 2,
                            breakdown: Some(CostBreakdown {
                                input: 8,
                                output: 6,
                                cache_read: 4,
                                cache_write: 2,
                            }),
                        }),
                    ),
                    (
                        "unsplit right",
                        priced(),
                        reported.clone(),
                        Some(Cost {
                            usd_micros: 30,
                            responses: 2,
                            breakdown: None,
                        }),
                    ),
                    (
                        "unsplit left",
                        reported,
                        priced(),
                        Some(Cost {
                            usd_micros: 30,
                            responses: 2,
                            breakdown: None,
                        }),
                    ),
                    (
                        "overflow",
                        Cost {
                            usd_micros: u64::MAX,
                            ..priced()
                        },
                        priced(),
                        None,
                    ),
                ] {
                    assert_eq!(left.checked_add(&right), expected, "{scenario}: {case}");
                }
            }) as fn(&'static str),
        ),
        (
            "round_trips_owner_kinds",
            (|scenario: &'static str| {
                for (case, key) in [
                    (
                        "assistant",
                        Key::Assistant {
                            package: "notes".into(),
                            id: "notes".into(),
                        },
                    ),
                    ("session", Key::Session(SessionId::new())),
                ] {
                    let encoded = serde_json::to_value(&key).unwrap();
                    assert_eq!(encoded["kind"], case, "{scenario}: {case}: wire kind");
                    assert_eq!(
                        serde_json::from_value::<Key>(encoded).unwrap(),
                        key,
                        "{scenario}: {case}: round trip"
                    );
                }
            }),
        ),
        (
            "query_offset_defaults_to_utc",
            (|scenario: &'static str| {
                let query: Query = serde_json::from_value(serde_json::json!({
                    "start_ms":0,"end_ms":1,"dimension":"project","projects":[],"worktrees":[],
                    "providers":[],"models":[],"before":null
                }))
                .unwrap();
                assert_eq!(query.utc_offset_minutes, 0, "{scenario}");
                assert_eq!(query.dimension, Dimension::Project, "{scenario}");
            }),
        ),
        (
            "optional_labels_default_to_absent",
            (|scenario: &'static str| {
                let group: Group = serde_json::from_value(serde_json::json!({
                    "key":{"kind":"provider","data":ProviderId::new()},
                    "metrics":{"responses":0,"tokens":null,"cost":null,"generation":null}
                }))
                .unwrap();
                assert_eq!(group.name, None, "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
