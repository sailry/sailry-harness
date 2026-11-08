use super::*;
use core::prelude::v1::test;

fn reference(provider: ProviderId) -> catalog::ModelRef {
    catalog::ModelRef {
        provider,
        model: "shared-model".into(),
    }
}

fn response(revision: u64, reference: &catalog::ModelRef) -> catalog::Scores {
    catalog::Scores {
        revision,
        batch: Some("batch".into()),
        published_at: Some("2026-11-07".into()),
        scores: vec![catalog::ModelScore {
            provider: reference.provider,
            model: reference.model.clone(),
            efforts: vec![catalog::EffortScore {
                effort: Effort::High,
                overall: 81.5,
                axes: [80; 5],
                elapsed_ms: None,
                cost_usd: None,
            }],
        }],
    }
}

#[test]
fn revision_invalidates_rows_and_misses() {
    let mut cache = Cache::default();
    let matched = reference(ProviderId::new());
    let missing = reference(ProviderId::new());
    cache.accept(catalog::ScoreStatus {
        revision: 4,
        ..Default::default()
    });
    cache.merge(&[matched.clone(), missing.clone()], response(4, &matched));
    assert!(cache.loaded.contains(&key(&missing)));
    assert_eq!(cache.rows.len(), 1);
    assert_eq!(cache.rows[&key(&matched)][0].overall, 81.5);
    cache.accept(catalog::ScoreStatus {
        revision: 4,
        checked_at_ms: Some(50),
        ..Default::default()
    });
    assert_eq!(cache.rows.len(), 1);
    cache.accept(catalog::ScoreStatus {
        revision: 5,
        ..Default::default()
    });
    assert!(cache.loaded.is_empty());
    assert!(cache.rows.is_empty());
    cache.merge(std::slice::from_ref(&matched), response(4, &matched));
    assert!(cache.rows.is_empty());
}

#[test]
fn stale_uses_check_time() {
    let mut status = catalog::ScoreStatus::default();
    assert!(stale(&status, 5));
    status.checked_at_ms = Some(10);
    assert!(!stale(&status, 5));
    assert!(!stale(&status, 86_400_009));
    assert!(stale(&status, 86_400_010));
    assert!(stale(&status, 172_800_000));
}

#[test]
fn older_checks_cannot_replace_current_failures() {
    let mut cache = Cache::default();
    let current = catalog::ScoreStatus {
        revision: 4,
        checked_at_ms: Some(100),
        fault: Some(sailry_protocol::Fault::new(
            sailry_protocol::ErrorCode::Unavailable,
            "Source unavailable",
        )),
        ..Default::default()
    };
    cache.accept(current.clone());
    cache.accept(catalog::ScoreStatus::default());
    assert_eq!(cache.status, current);
    cache.accept(catalog::ScoreStatus {
        revision: 4,
        checked_at_ms: Some(50),
        ..Default::default()
    });
    assert_eq!(cache.status, current);
    cache.accept(catalog::ScoreStatus {
        revision: 4,
        checked_at_ms: Some(110),
        ..Default::default()
    });
    assert!(cache.status.fault.is_none());
    assert_eq!(cache.status.checked_at_ms, Some(110));
}

#[test]
fn providers_keep_identical_model_names_separate() {
    let mut cache = Cache::default();
    let first = reference(ProviderId::new());
    let second = reference(ProviderId::new());
    cache.merge(std::slice::from_ref(&first), response(0, &first));
    assert!(cache.loaded.contains(&key(&first)));
    assert!(!cache.loaded.contains(&key(&second)));
    assert!(!cache.rows.contains_key(&key(&second)));
}
