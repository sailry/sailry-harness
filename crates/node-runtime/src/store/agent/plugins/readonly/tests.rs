use super::*;
use crate::store::agent::tests::fixture::Fixture;
use serde_json::json;

async fn fixture() -> (Fixture, TurnId, WorktreeId) {
    let mut fixture = Fixture::new().await;
    let mut session =
        crate::store::commands::session(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
    let worktree = session.worktree;
    session.config.mode = WorkMode::Plan;
    crate::store::commands::revise_session(&fixture.database.connection, session)
        .await
        .unwrap();
    let turn = fixture.queued().await;
    (fixture, turn, worktree)
}

#[tokio::test]
async fn rejects_actual_mutations_despite_read_labels() {
    let (fixture, turn, worktree) = fixture().await;
    let db = &fixture.database.connection;
    let query = |read_only| Command::QueryDatabase {
        read_only,
        database: None,
        profile: sailry_protocol::DatabaseId::new(),
        expected_revision: 1,
        sql: "SELECT 1".into(),
        row_limit: 1,
        timeout_ms: 1000,
    };
    check_action(db, turn, &query(true), Some(&Action::ReadDatabases))
        .await
        .unwrap();
    assert_eq!(
        check_action(db, turn, &query(false), Some(&Action::ReadDatabases))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    for (action, read_only) in [
        (sailry_protocol::browser::Action::Read { tab: None }, true),
        (
            sailry_protocol::browser::Action::Navigate {
                url: "https://example.com".into(),
                tab: None,
                background: false,
            },
            true,
        ),
        (
            sailry_protocol::browser::Action::Screenshot { tab: 0 },
            false,
        ),
        (
            sailry_protocol::browser::Action::Click {
                tab: 0,
                snapshot: "fixture".into(),
                element: 1,
            },
            false,
        ),
    ] {
        let command = Command::UseBrowser {
            session: fixture.session,
            worktree,
            action,
        };
        assert_eq!(
            check_action(db, turn, &command, Some(&Action::ReadBrowser))
                .await
                .is_ok(),
            read_only
        );
    }
}

#[tokio::test]
async fn permits_reads_and_rejects_resource_writes() {
    let (mut fixture, turn, worktree) = fixture().await;
    let mut session =
        crate::store::commands::session(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
    session.config.mode = WorkMode::Code;
    crate::store::commands::revise_session(&fixture.database.connection, session)
        .await
        .unwrap();
    let coding = fixture.queued().await;
    let db = &fixture.database.connection;
    for (command, action) in [
        (
            Command::ReadPluginValue {
                key: "state".into(),
            },
            Action::ReadStorage,
        ),
        (Command::InspectGit { worktree }, Action::ReadGit),
        (
            Command::ReadFile {
                worktree,
                path: "source.txt".into(),
            },
            Action::ReadFiles,
        ),
    ] {
        check_action(db, turn, &command, Some(&action))
            .await
            .unwrap();
    }
    for (command, action) in [
        (
            Command::WriteFile {
                worktree,
                path: "source.txt".into(),
                text: "Changed".into(),
                expected_revision: None,
            },
            Action::WriteFiles,
        ),
        (
            Command::WritePluginValue {
                key: "state".into(),
                value: json!(true),
                expected_revision: 0,
            },
            Action::WriteStorage,
        ),
        (
            Command::RunCommand {
                turn,
                command: "touch changed.txt".into(),
                description: None,
                access: sailry_protocol::process::Access::Workspace,
                cwd: String::new(),
                timeout_ms: 1000,
                pty: false,
                background: false,
                attachments: vec![],
            },
            Action::ControlCommands,
        ),
    ] {
        assert_eq!(
            check_action(db, turn, &command, Some(&action))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        check_action(db, coding, &command, Some(&action))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn permits_only_the_captured_artifact_effect() {
    let (fixture, turn, _) = fixture().await;
    let command = Command::WriteArtifact(sailry_protocol::artifact::Write {
        kind: sailry_protocol::artifact::Kind::Document,
        id: None,
        expected_revision: None,
        title: "Options".into(),
        summary: String::new(),
        html: "<h1>Options</h1>".into(),
    });
    let db = &fixture.database.connection;
    check_action(db, turn, &command, Some(&Action::WriteArtifacts))
        .await
        .unwrap();
    for action in [None, Some(Action::WriteStorage), Some(Action::WriteFiles)] {
        assert_eq!(
            check_action(db, turn, &command, action.as_ref())
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
}

#[tokio::test]
async fn permits_observation_without_model_mutation() {
    let (mut fixture, turn, _) = fixture().await;
    let mut session =
        crate::store::commands::session(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
    session.config.mode = WorkMode::Code;
    crate::store::commands::revise_session(&fixture.database.connection, session)
        .await
        .unwrap();
    let coding = fixture.queued().await;
    let db = &fixture.database.connection;
    let install = Command::InstallLocalModel {
        id: "fixture".into(),
    };
    assert_eq!(
        check_action(db, turn, &install, Some(&Action::InstallModels))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    check_action(db, coding, &install, Some(&Action::InstallModels))
        .await
        .unwrap();
    for command in [
        Command::ReadInstallations {
            requests: vec![sailry_protocol::RequestId::new()],
        },
        Command::DismissInstallation {
            request: sailry_protocol::RequestId::new(),
        },
    ] {
        check_action(db, turn, &command, Some(&Action::InstallModels))
            .await
            .unwrap();
    }
}
