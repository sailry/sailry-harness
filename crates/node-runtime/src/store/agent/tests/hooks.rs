use super::*;
use crate::store::hooks;
use sailry_protocol::hooks::{Phase, Script, Settings};

fn enabled() -> Settings {
    Settings {
        enabled: true,
        before_tool: Some(Script {
            command: "printf before".into(),
            timeout_ms: 100,
        }),
        turn_end: Some(Script {
            command: "printf end".into(),
            timeout_ms: 100,
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
