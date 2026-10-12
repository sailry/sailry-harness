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

#[test]
fn routes_only_to_execution_owners() {
    let ocr = ModelRef::Ocr {
        target: Target::Node,
        id: "a".repeat(64),
    };
    assert!(matches!(
        ocr.node_command().unwrap(),
        Command::InstallOcrModel { .. }
    ));
    let embedding = ModelRef::Embedding {
        target: Target::Node,
        model: knowledge::Model::GraniteMulti,
    };
    assert!(matches!(
        embedding.node_command().unwrap(),
        Command::InstallKnowledgeModel { .. }
    ));
    let speech = ModelRef::Speech {
        target: Target::Controller,
        id: None,
    };
    speech.validate().unwrap();
    assert_eq!(
        speech.node_command().unwrap_err().code,
        ErrorCode::Unavailable
    );
    for invalid in [
        ModelRef::Ocr {
            target: Target::Controller,
            id: "a".repeat(64),
        },
        ModelRef::Speech {
            target: Target::Node,
            id: None,
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
