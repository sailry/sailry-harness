use super::*;

#[test]
fn keeps_measured_progress_distinct() {
    for progress in [
        Progress::Percent {
            percent: 40,
            bytes_per_second: Some(120),
        },
        Progress::Counts {
            completed: 3,
            total: 8,
            bytes_per_second: None,
        },
    ] {
        progress.validate().unwrap();
        assert_eq!(
            serde_json::from_value::<Progress>(serde_json::to_value(progress).unwrap()).unwrap(),
            progress
        );
    }
}

#[test]
fn rejects_invalid_totals() {
    for progress in [
        Progress::Percent {
            percent: 101,
            bytes_per_second: None,
        },
        Progress::Counts {
            completed: 0,
            total: 0,
            bytes_per_second: None,
        },
        Progress::Counts {
            completed: 9,
            total: 8,
            bytes_per_second: None,
        },
    ] {
        assert!(progress.validate().is_err());
    }
}

#[test]
fn keeps_future_models_in_the_common_contract() {
    let task = Task {
        request: RequestId::new(),
        revision: 1,
        resource: Resource::Model {
            kind: Kind::Embedding,
            id: "fixture-model".into(),
        },
        state: State::Queued,
    };
    assert_eq!(
        serde_json::from_value::<Task>(serde_json::to_value(&task).unwrap()).unwrap(),
        task
    );
}
