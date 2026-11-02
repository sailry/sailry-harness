use super::*;

fn parse_args(values: &[String]) -> Result<Option<Options>, Box<dyn std::error::Error>> {
    parse(values.iter().map(OsString::from))
}

#[test]
fn preserves_input_and_explicit_identity() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("profile not started");
    let path = directory.path().join("任务 input.txt");
    let text = "Keep `literal` $(shell) and 中文 🙂\nSecond line";
    std::fs::write(&path, text).unwrap();
    let request = RequestId::new();
    let session = SessionId::new();
    let options = parse_args(&[
        "submit".into(),
        "--session".into(),
        session.to_string(),
        "--request-id".into(),
        request.to_string(),
        "--expected-revision".into(),
        "7".into(),
        "--prompt-file".into(),
        path.to_str().unwrap().into(),
        "--data-dir".into(),
        profile.to_str().unwrap().into(),
        "--no-wait".into(),
        "--timeout".into(),
        "3".into(),
    ])
    .unwrap()
    .unwrap();
    assert_eq!(options.profile, profile);
    assert!(!profile.exists());
    assert!(!options.wait);
    let Task::Submit {
        request: id,
        target,
        message,
        title,
        wait,
        timeout,
        ..
    } = options.task
    else {
        panic!("submission expected")
    };
    let Target::Session {
        session: target,
        expected_revision,
    } = *target
    else {
        panic!("session submission expected")
    };
    assert_eq!(id, request);
    assert_eq!(target, session);
    assert_eq!(expected_revision, Some(7));
    assert_eq!(message.text, text);
    assert_eq!(title, "");
    assert!(!wait);
    assert_eq!(timeout, 3);
}

#[test]
fn rejects_conflicting_or_repeated_options() {
    let base = vec![
        "submit".into(),
        "--session".into(),
        SessionId::new().to_string(),
        "--prompt".into(),
        "task".into(),
    ];
    for extra in [
        vec!["--worktree".into(), WorktreeId::new().to_string()],
        vec!["--session".into(), SessionId::new().to_string()],
        vec!["--prompt".into(), "second".into()],
        vec!["--stdin".into()],
        vec!["--config".into(), "/does/not/exist".into()],
        vec!["--title".into(), "New title".into()],
        vec!["--execution".into(), "model".into()],
        vec!["--team".into(), TeamId::new().to_string()],
        vec!["--team-revision".into(), "1".into()],
        vec!["--association".into(), "/does/not/exist".into()],
        vec!["--timeout".into(), "0".into()],
        vec!["--timeout".into(), "86401".into()],
        vec!["--data-dir".into(), "relative".into()],
        vec!["--unknown".into(), "value".into()],
    ] {
        let mut arguments = base.clone();
        arguments.extend(extra);
        assert!(parse_args(&arguments).is_err(), "{arguments:?}");
    }
    assert!(
        parse_args(&[
            "submit".into(),
            "--worktree".into(),
            WorktreeId::new().to_string(),
            "--prompt".into(),
            "task".into(),
            "--expected-revision".into(),
            "1".into()
        ])
        .is_err()
    );
    assert!(
        parse_args(&[
            "wait".into(),
            "--session".into(),
            SessionId::new().to_string(),
            "--turn".into(),
            TurnId::new().to_string(),
            "--no-wait".into()
        ])
        .is_err()
    );
}

#[test]
fn reads_only_bounded_utf8_input() {
    assert_eq!(read("任务 🙂".as_bytes()).unwrap(), "任务 🙂");
    assert!(read([255].as_slice()).is_err());
    assert!(read(vec![b'x'; super::super::MAX_REQUEST + 1].as_slice()).is_err());
    assert!(
        parse_args(&[
            "submit".into(),
            "--session".into(),
            SessionId::new().to_string(),
            "--prompt".into(),
            " \n\t".into()
        ])
        .is_err()
    );
}

#[test]
fn loads_only_explicit_new_task_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session config.json");
    let config = SessionConfig {
        features: Default::default(),
        assistant: None,
        resource: None,
        provider: ProviderId::new(),
        model: "fixture".into(),
        effort: Effort::Default,
        mode: WorkMode::Code,
        options: Default::default(),
        permission: Permission::Ask,
        credential: None,
    };
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let options = parse_args(&[
        "submit".into(),
        "--worktree".into(),
        WorktreeId::new().to_string(),
        "--prompt".into(),
        "First line\nSecond line".into(),
        "--config".into(),
        path.to_str().unwrap().into(),
    ])
    .unwrap()
    .unwrap();
    let Task::Submit { target, title, .. } = options.task else {
        panic!("submission expected")
    };
    let Target::Worktree {
        config: Some(selected),
        ..
    } = *target
    else {
        panic!("worktree submission expected")
    };
    assert_eq!(*selected, config);
    assert_eq!(title, "First line");
}

#[test]
fn selects_execution_and_association() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("association.json");
    let association = conversation::Association {
        namespace: "automation".into(),
        kind: "job".into(),
        id: "daily".into(),
    };
    std::fs::write(&path, serde_json::to_vec(&association).unwrap()).unwrap();
    let team = TeamId::new();
    let base = vec![
        "submit".into(),
        "--worktree".into(),
        WorktreeId::new().to_string(),
        "--prompt".into(),
        "Run".into(),
    ];
    for (choice, selected) in [("model", None), ("team", Some(team))] {
        let mut arguments = base.clone();
        arguments.extend([
            "--execution".into(),
            choice.into(),
            "--association".into(),
            path.to_str().unwrap().into(),
        ]);
        if let Some(team) = selected {
            arguments.extend([
                "--team".into(),
                team.to_string(),
                "--team-revision".into(),
                "9007199254740993".into(),
            ]);
        }
        let Task::Submit { target, .. } = parse_args(&arguments).unwrap().unwrap().task else {
            panic!("submission expected")
        };
        let Target::Worktree {
            execution,
            team,
            association: actual,
            ..
        } = *target
        else {
            panic!("worktree task expected")
        };
        assert_eq!(
            execution,
            if selected.is_some() {
                conversation::Execution::Team
            } else {
                conversation::Execution::Model
            }
        );
        assert_eq!(
            team,
            selected.map(|id| team::Selection {
                id,
                revision: Some(9_007_199_254_740_993)
            })
        );
        assert_eq!(actual, Some(association.clone()));
    }
    let mut latest = base.clone();
    latest.extend(["--team".into(), team.to_string()]);
    let Task::Submit { target, .. } = parse_args(&latest).unwrap().unwrap().task else {
        panic!("submission expected")
    };
    let Target::Worktree {
        execution,
        team: Some(selection),
        ..
    } = *target
    else {
        panic!("team selection expected")
    };
    assert_eq!(execution, conversation::Execution::Team);
    assert_eq!(selection.revision, None);
    for extra in [
        vec!["--team-revision".into(), "1".into()],
        vec![
            "--execution".into(),
            "model".into(),
            "--team".into(),
            team.to_string(),
        ],
        vec!["--execution".into(), "unknown".into()],
        vec![
            "--execution".into(),
            "model".into(),
            "--execution".into(),
            "team".into(),
        ],
        vec![
            "--team".into(),
            team.to_string(),
            "--team-revision".into(),
            "01".into(),
        ],
    ] {
        let mut arguments = base.clone();
        arguments.extend(extra);
        assert!(parse_args(&arguments).is_err(), "{arguments:?}");
    }
}
