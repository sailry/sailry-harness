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
fn sums_disjoint_charges() {
    let total = priced().checked_add(&priced()).unwrap();
    assert_eq!(total.usd_micros, 20);
    assert_eq!(total.responses, 2);
    assert_eq!(
        total.breakdown,
        Some(CostBreakdown {
            input: 8,
            output: 6,
            cache_read: 4,
            cache_write: 2
        })
    );
}

#[test]
fn keeps_unsplit_totals_unknown() {
    let reported = Cost {
        usd_micros: 20,
        responses: 1,
        breakdown: None,
    };
    let total = priced().checked_add(&reported).unwrap();
    assert_eq!(total.usd_micros, 30);
    assert_eq!(total.breakdown, None);
    assert_eq!(reported.checked_add(&priced()), Some(total));
}

#[test]
fn rejects_overflow() {
    let cost = Cost {
        usd_micros: u64::MAX,
        ..priced()
    };
    assert!(cost.checked_add(&priced()).is_none());
}

#[test]
fn assistant_owner_round_trips() {
    let key = Key::Assistant {
        package: "notes".into(),
        id: "notes".into(),
    };
    let encoded = serde_json::to_value(&key).unwrap();
    assert_eq!(encoded["kind"], "assistant");
    assert_eq!(serde_json::from_value::<Key>(encoded).unwrap(), key);
}

#[test]
fn session_owner_round_trips() {
    let key = Key::Session(SessionId::new());
    let encoded = serde_json::to_value(&key).unwrap();
    assert_eq!(encoded["kind"], "session");
    assert_eq!(serde_json::from_value::<Key>(encoded).unwrap(), key);
}
