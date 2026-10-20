use super::*;

#[test]
fn identifies_language_package_versions() {
    let resource = Resource::Language {
        package: "rust-analyzer".into(),
        version: "1.0".into(),
    };
    let wire = serde_json::to_value(&resource).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"kind":"language","data":{"package":"rust-analyzer","version":"1.0"}})
    );
    assert_eq!(serde_json::from_value::<Resource>(wire).unwrap(), resource);
    assert!(serde_json::from_value::<Resource>(serde_json::json!({"kind":"language","data":{"package":"rust-analyzer","version":"1.0","origin":"node"}})).is_err());
}

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

#[test]
fn routes_only_to_execution_owners() {
    let ocr = ModelRef::Node {
        kind: Kind::Ocr,
        id: "a".repeat(64),
    };
    assert!(matches!(
        ocr.node_command().unwrap(),
        Command::InstallLocalModel { .. }
    ));
    let embedding = ModelRef::Node {
        kind: Kind::Embedding,
        id: "granite-multi".into(),
    };
    assert!(matches!(
        embedding.node_command().unwrap(),
        Command::InstallLocalModel { .. }
    ));
    let speech = ModelRef::Controller {
        kind: Kind::Speech,
        id: None,
    };
    speech.validate().unwrap();
    assert_eq!(
        speech.node_command().unwrap_err().code,
        ErrorCode::Unavailable
    );
    for invalid in [
        ModelRef::Node {
            kind: Kind::Speech,
            id: "sensevoice-int8".into(),
        },
        ModelRef::Controller {
            kind: Kind::Ocr,
            id: None,
        },
        ModelRef::Node {
            kind: Kind::Embedding,
            id: "../outside".into(),
        },
    ] {
        assert_eq!(
            invalid.validate().unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}

#[test]
fn bounds_scoped_observation() {
    let id = RequestId::new();
    validate_requests(&[id]).unwrap();
    assert!(validate_requests(&[]).is_err());
    assert!(validate_requests(&[id, id]).is_err());
    assert!(
        validate_requests(
            &(0..=MAX_REQUESTS)
                .map(|_| RequestId::new())
                .collect::<Vec<_>>()
        )
        .is_err()
    );
}
