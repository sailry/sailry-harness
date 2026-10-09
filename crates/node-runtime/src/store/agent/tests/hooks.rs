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

#[test]
fn keeps_uncertain_launch_receipts() {
    let mut fixture = fixture::Fixture::new();
    let session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    hooks::save(
        &fixture.database.connection,
        session.worktree,
        &enabled(),
        0,
    )
    .unwrap();
    let turn = fixture.queued();
    runs::start(&fixture.database.connection, turn).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    for phase in [Phase::BeforeTool, Phase::TurnEnd] {
        hooks::claim(&fixture.database.connection, turn, phase, 0).unwrap();
        assert_eq!(
            hooks::claim(&fixture.database.connection, turn, phase, 0)
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
    .unwrap();
    assert_eq!(
        hooks::claim(&fixture.database.connection, turn, Phase::BeforeTool, 0)
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    let path = fixture
        ._root
        .path()
        .canonicalize()
        .unwrap()
        .join("node.sqlite3");
    let node = fixture.database.node;
    fixture.database.close().unwrap();
    let database = Database::open(&path, node, None).unwrap();
    runs::recover(&database.connection).unwrap();
    assert!(hooks::claim(&database.connection, turn, Phase::TurnEnd, 0).is_err());
    let pending: i64 = database
        .connection
        .query_row(
            "SELECT count(*) FROM hook_executions WHERE turn=?1 AND phase='turn_end' AND result IS NULL",
            [turn.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending, 1);
}

#[test]
fn refuses_frozen_planning_admission() {
    let mut fixture = fixture::Fixture::new();
    let mut session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    hooks::save(
        &fixture.database.connection,
        session.worktree,
        &enabled(),
        0,
    )
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
    );
    let turn = fixture.queued();
    runs::start(&fixture.database.connection, turn).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    for phase in [Phase::BeforeTool, Phase::TurnEnd] {
        assert_eq!(
            hooks::claim(&fixture.database.connection, turn, phase, 0)
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
    assert_eq!(
        fixture
            .database
            .connection
            .query_row("SELECT count(*) FROM hook_executions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn rejects_duplicate_ids_without_changing_settings() {
    let fixture = fixture::Fixture::new();
    let session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    let initial = hooks::save(
        &fixture.database.connection,
        session.worktree,
        &enabled(),
        0,
    )
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
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        hooks::read(&fixture.database.connection, session.worktree).unwrap(),
        initial
    );
    invalid.additional[1].id = "before_tool".into();
    assert_eq!(
        hooks::save(&fixture.database.connection, session.worktree, &invalid, 1)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        hooks::read(&fixture.database.connection, session.worktree).unwrap(),
        initial
    );
}

#[test]
fn captures_additional_scripts_in_admission_order() {
    let mut fixture = fixture::Fixture::new();
    let session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
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
    let mut settings =
        hooks::save(&fixture.database.connection, session.worktree, &settings, 0).unwrap();
    let turn = fixture.queued();
    settings.additional.reverse();
    settings.additional[0].script.enabled = true;
    settings.additional[1].script.enabled = false;
    hooks::save(&fixture.database.connection, session.worktree, &settings, 1).unwrap();
    let captured = hooks::frozen(&fixture.database.connection, turn).unwrap();
    assert_eq!(
        captured
            .scripts(Phase::BeforeTool)
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        ["before_tool", "second"]
    );
    runs::start(&fixture.database.connection, turn).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    hooks::claim(&fixture.database.connection, turn, Phase::BeforeTool, 0).unwrap();
    assert_eq!(
        hooks::claim(&fixture.database.connection, turn, Phase::BeforeTool, 0)
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(
        fixture
            .database
            .connection
            .query_row(
                "SELECT count(*) FROM hook_executions WHERE turn=?1",
                [turn.to_string()],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}
