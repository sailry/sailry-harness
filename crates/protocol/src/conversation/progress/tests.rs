use super::*;
use serde_json::json;

#[test]
fn bounds_complete_snapshots() {
    let mut progress = Progress {
        title: Some("Task 中文 🙂".into()),
        steps: vec![Step {
            description: "Inspect sources".into(),
            state: State::InProgress,
        }],
    };
    progress.validate().unwrap();
    for text in [
        "".to_owned(),
        " \n\t".into(),
        "x".repeat(MAX_DESCRIPTION_BYTES + 1),
    ] {
        progress.steps[0].description = text;
        assert!(progress.validate().is_err());
    }
    progress.title = None;
    progress.steps = vec![
        Step {
            description: "中".repeat(2730),
            state: State::Pending
        };
        8
    ];
    progress.validate().unwrap();
    progress.steps.push(progress.steps[0].clone());
    assert!(progress.validate().is_err());
    progress.steps = vec![
        Step {
            description: "Step".into(),
            state: State::Skipped
        };
        MAX_STEPS
    ];
    progress.validate().unwrap();
    progress.steps.push(progress.steps[0].clone());
    assert!(progress.validate().is_err());
    progress.steps.clear();
    assert!(progress.validate().is_err());
}

#[test]
fn validates_states() {
    let value = json!({"title": null, "steps": [
        {"description": "Inspect", "state": "completed"},
        {"description": "Build", "state": "in_progress"},
        {"description": "Verify", "state": "pending"},
        {"description": "Publish", "state": "skipped"}
    ]});
    let progress: Progress = serde_json::from_value(value.clone()).unwrap();
    progress.validate().unwrap();
    assert_eq!(serde_json::to_value(progress).unwrap(), value);
    for invalid in [
        json!({"title": null, "steps": [], "status": "active"}),
        json!({"title": null, "steps": [{"description": "Inspect", "state": "done"}]}),
        json!({"title": null, "steps": [{"description": "Inspect", "state": "pending", "extra": true}]}),
    ] {
        assert!(serde_json::from_value::<Progress>(invalid).is_err());
    }
}
