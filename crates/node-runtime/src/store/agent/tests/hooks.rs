use super::*;
use crate::store::hooks;
use sailry_protocol::hooks::{Phase, Script, Settings};

fn enabled() -> Settings {
    Settings {
        enabled: true,
        before_tool: Some(Script {
            command: "printf before".into(),
            timeout_ms: 100,
            ..Default::default()
        }),
        turn_end: Some(Script {
            command: "printf end".into(),
            timeout_ms: 100,
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn retains_guidance_capture() {
    for change in 0..3 {
        let mut fixture = fixture::Fixture::new().await;
        let session =
            crate::store::commands::session(&fixture.database.connection, fixture.session)
                .await
                .unwrap();
        let mut initial = if change == 0 {
            Settings::default()
        } else {
            enabled()
        };
        if change == 2 {
            initial.additional = ["first", "second"]
                .into_iter()
                .map(|id| sailry_protocol::hooks::Entry {
                    id: id.into(),
                    phase: Phase::TurnEnd,
                    script: Script {
                        command: format!("printf {id}"),
                        ..Default::default()
                    },
                })
                .collect();
        }
        let initial = hooks::save(&fixture.database.connection, session.worktree, &initial, 0)
            .await
            .unwrap();
        let active = fixture.queued().await;
        runs::start(&fixture.database.connection, active)
            .await
            .unwrap();
        let recipient = runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap();
        let mut changed = match change {
            0 => Settings {
                revision: initial.revision,
                ..enabled()
            },
            1 => Settings {
                enabled: false,
                ..initial.clone()
            },
            _ => initial.clone(),
        };
        if change == 2 {
            changed.additional.reverse();
            changed.additional[0].script.enabled = false;
            changed.turn_end.as_mut().unwrap().command = "printf changed".into();
        }
        let changed = hooks::save(
            &fixture.database.connection,
            session.worktree,
            &changed,
            initial.revision,
        )
        .await
        .unwrap();
        let source = fixture.queued().await;
        let input: Input = "Revised guidance".into();
        command(
            &mut fixture.database,
            &fixture.events,
            Command::EditQueuedTurn {
                turn: source,
                expected_revision: 1,
                message: input.clone(),
            },
        )
        .await;
        let Output::Queue(selected) = command(
            &mut fixture.database,
            &fixture.events,
            Command::GuideQueuedTurn {
                turn: source,
                recipient: active,
                expected_revision: 2,
            },
        )
        .await
        else {
            panic!("queue expected")
        };
        let selected = selected.queue.items[0].turn;
        assert_ne!(selected, source);
        assert_eq!(
            runs::get(&fixture.database.connection, source)
                .await
                .unwrap()
                .status,
            Status::Cancelled
        );
        assert_eq!(
            hooks::frozen(&fixture.database.connection, source)
                .await
                .unwrap(),
            changed
        );
        assert_eq!(
            hooks::frozen(&fixture.database.connection, active)
                .await
                .unwrap(),
            initial
        );
        assert_eq!(
            queue::message(&fixture.database.connection, selected)
                .await
                .unwrap()
                .message,
            input
        );
        assert!(
            queue::guidance::yield_run(&mut fixture.database, active, &fixture.events)
                .await
                .unwrap()
        );
        runs::finish(
            &mut fixture.database,
            active,
            Status::Continued,
            None,
            &fixture.events,
        )
        .await
        .unwrap();
        let continuation = runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(continuation.turn.id, selected);
        assert_eq!(continuation.hooks, recipient.hooks);
        assert_eq!(continuation.message, input);
        let launch = hooks::claim(&fixture.database.connection, selected, Phase::TurnEnd, 0).await;
        if initial.enabled {
            launch.unwrap();
        } else {
            assert_eq!(launch.unwrap_err().code, ErrorCode::PermissionDenied);
        }
    }
}

#[tokio::test]
async fn keeps_uncertain_launch_receipts() {
    let mut fixture = fixture::Fixture::new().await;
    let session = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    hooks::save(
        &fixture.database.connection,
        session.worktree,
        &enabled(),
        0,
    )
    .await
    .unwrap();
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    for phase in [Phase::BeforeTool, Phase::TurnEnd] {
        hooks::claim(&fixture.database.connection, turn, phase, 0)
            .await
            .unwrap();
        assert_eq!(
            hooks::claim(&fixture.database.connection, turn, phase, 0)
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
    }
    hooks::finish(
        &fixture.database.connection,
        turn,
        Phase::BeforeTool,
        0,
        &Ok(process::Outcome::Exited(0)),
    )
    .await
    .unwrap();
    assert_eq!(
        hooks::claim(&fixture.database.connection, turn, Phase::BeforeTool, 0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    let path = fixture
        ._root
        .path()
        .canonicalize()
        .unwrap()
        .join("node.turso");
    let node = fixture.database.node;
    fixture.database.close().await.unwrap();
    let database = Database::open(&path, node, None).await.unwrap();
    {
        let transaction = database.connection.unchecked_transaction().await.unwrap();
        runs::recover(&transaction).await.unwrap();
        transaction.commit().await.unwrap();
    }
    assert!(
        hooks::claim(&database.connection, turn, Phase::TurnEnd, 0)
            .await
            .is_err()
    );
    let pending: i64 = database.connection.prepare("SELECT count(*) FROM hook_executions WHERE turn=?1 AND phase='turn_end' AND result IS NULL").await.unwrap().query_row([turn.to_string()]).await.and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(pending, 1);
}

#[tokio::test]
async fn refuses_frozen_planning_admission() {
    let mut fixture = fixture::Fixture::new().await;
    let mut session =
        crate::store::commands::session(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
    hooks::save(
        &fixture.database.connection,
        session.worktree,
        &enabled(),
        0,
    )
    .await
    .unwrap();
    session.config.mode = WorkMode::Plan;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: session.id,
            expected_revision: session.revision,
            config: session.config,
        },
    )
    .await;
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    for phase in [Phase::BeforeTool, Phase::TurnEnd] {
        assert_eq!(
            hooks::claim(&fixture.database.connection, turn, phase, 0)
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM hook_executions")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn rejects_duplicate_ids_without_changing_settings() {
    let fixture = fixture::Fixture::new().await;
    let session = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let initial = hooks::save(
        &fixture.database.connection,
        session.worktree,
        &enabled(),
        0,
    )
    .await
    .unwrap();
    let mut invalid = initial.clone();
    let entry = sailry_protocol::hooks::Entry {
        id: "same".into(),
        phase: Phase::BeforeTool,
        script: Script {
            command: "printf second".into(),
            ..Default::default()
        },
    };
    invalid.additional = vec![entry.clone(), entry];
    assert_eq!(
        hooks::save(&fixture.database.connection, session.worktree, &invalid, 1)
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        hooks::read(&fixture.database.connection, session.worktree)
            .await
            .unwrap(),
        initial
    );
    invalid.additional[1].id = "before_tool".into();
    assert_eq!(
        hooks::save(&fixture.database.connection, session.worktree, &invalid, 1)
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        hooks::read(&fixture.database.connection, session.worktree)
            .await
            .unwrap(),
        initial
    );
}

#[tokio::test]
async fn captures_additional_scripts_in_admission_order() {
    let mut fixture = fixture::Fixture::new().await;
    let session = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let mut settings = enabled();
    settings.additional = vec![
        sailry_protocol::hooks::Entry {
            id: "second".into(),
            phase: Phase::BeforeTool,
            script: Script {
                command: "printf second".into(),
                ..Default::default()
            },
        },
        sailry_protocol::hooks::Entry {
            id: "disabled".into(),
            phase: Phase::BeforeTool,
            script: Script {
                command: "printf disabled".into(),
                enabled: false,
                ..Default::default()
            },
        },
    ];
    let mut settings = hooks::save(&fixture.database.connection, session.worktree, &settings, 0)
        .await
        .unwrap();
    let turn = fixture.queued().await;
    settings.additional.reverse();
    settings.additional[0].script.enabled = true;
    settings.additional[1].script.enabled = false;
    hooks::save(&fixture.database.connection, session.worktree, &settings, 1)
        .await
        .unwrap();
    let captured = hooks::frozen(&fixture.database.connection, turn)
        .await
        .unwrap();
    assert_eq!(
        captured
            .scripts(Phase::BeforeTool)
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        ["before_tool", "second"]
    );
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    hooks::claim(&fixture.database.connection, turn, Phase::BeforeTool, 0)
        .await
        .unwrap();
    assert_eq!(
        hooks::claim(&fixture.database.connection, turn, Phase::BeforeTool, 0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM hook_executions WHERE turn=?1")
            .await
            .unwrap()
            .query_row([turn.to_string()])
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        1
    );
}
