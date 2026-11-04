use super::*;

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "identifies_language_package_versions",
            (|scenario: &'static str| {
                let resource = Resource::Language {
                    package: "rust-analyzer".into(),
                    version: "1.0".into(),
                };
                let wire = serde_json::to_value(&resource).unwrap();
                assert_eq!(
                    wire,
                    serde_json::json!({"kind":"language","data":{"package":"rust-analyzer","version":"1.0"}}),
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<Resource>(wire).unwrap(),
                    resource,
                    "{scenario}"
                );
                assert!(serde_json::from_value::<Resource>(serde_json::json!({"kind":"language","data":{"package":"rust-analyzer","version":"1.0","origin":"node"}})).is_err(), "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "validates_measured_progress",
            (|scenario: &'static str| {
                for (case, progress, valid) in [
                    (
                        "percent",
                        Progress::Percent {
                            percent: 40,
                            bytes_per_second: Some(120),
                        },
                        true,
                    ),
                    (
                        "counts",
                        Progress::Counts {
                            completed: 3,
                            total: 8,
                            bytes_per_second: None,
                        },
                        true,
                    ),
                    (
                        "percent overflow",
                        Progress::Percent {
                            percent: 101,
                            bytes_per_second: None,
                        },
                        false,
                    ),
                    (
                        "empty total",
                        Progress::Counts {
                            completed: 0,
                            total: 0,
                            bytes_per_second: None,
                        },
                        false,
                    ),
                    (
                        "count overflow",
                        Progress::Counts {
                            completed: 9,
                            total: 8,
                            bytes_per_second: None,
                        },
                        false,
                    ),
                ] {
                    assert_eq!(
                        progress.validate().is_ok(),
                        valid,
                        "{scenario}: {case}: validation"
                    );
                    if valid {
                        assert_eq!(
                            serde_json::from_value::<Progress>(
                                serde_json::to_value(progress).unwrap()
                            )
                            .unwrap(),
                            progress,
                            "{scenario}: {case}: wire"
                        );
                    }
                }
            }),
        ),
        (
            "keeps_future_models_in_the_common_contract",
            (|scenario: &'static str| {
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
                    task,
                    "{scenario}"
                );
            }),
        ),
        (
            "routes_only_to_execution_owners",
            (|scenario: &'static str| {
                let ocr = ModelRef::Node {
                    kind: Kind::Ocr,
                    id: "a".repeat(64),
                };
                assert!(
                    matches!(
                        ocr.node_command().unwrap(),
                        Command::InstallLocalModel { .. }
                    ),
                    "{scenario}"
                );
                let embedding = ModelRef::Node {
                    kind: Kind::Embedding,
                    id: "granite-multi".into(),
                };
                assert!(
                    matches!(
                        embedding.node_command().unwrap(),
                        Command::InstallLocalModel { .. }
                    ),
                    "{scenario}"
                );
                let speech = ModelRef::Controller {
                    kind: Kind::Speech,
                    id: None,
                };
                speech.validate().unwrap();
                assert_eq!(
                    speech.node_command().unwrap_err().code,
                    ErrorCode::Unavailable,
                    "{scenario}"
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
                        ErrorCode::InvalidRequest,
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "bounds_scoped_observation",
            (|scenario: &'static str| {
                let id = RequestId::new();
                validate_requests(&[id]).unwrap();
                assert!(validate_requests(&[]).is_err(), "{scenario}");
                assert!(validate_requests(&[id, id]).is_err(), "{scenario}");
                assert!(
                    validate_requests(
                        &(0..=MAX_REQUESTS)
                            .map(|_| RequestId::new())
                            .collect::<Vec<_>>()
                    )
                    .is_err(),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
