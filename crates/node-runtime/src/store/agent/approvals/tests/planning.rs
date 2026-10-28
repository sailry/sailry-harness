use super::*;
use sailry_protocol::WorkMode;

#[tokio::test]
async fn rejects_mutating_approvals() {
    for permission in [Permission::Ask, Permission::Project, Permission::Full] {
        let mut fixture = Fixture::with_mode(permission, WorkMode::Plan).await;
        for tool in [
            "write_file",
            "plugin_fixture_command",
            "mcp_fixture",
            "unknown_tool",
        ] {
            let mut request = fixture.request.clone();
            request.name = tool.into();
            assert_eq!(
                crate::store::agent::permissions::source(
                    &fixture.database.connection,
                    fixture.turn,
                    &request,
                )
                .await
                .unwrap_err()
                .code,
                ErrorCode::PermissionDenied,
            );
        }
        assert_eq!(
            begin(
                &mut fixture.database,
                fixture.turn,
                &fixture.request,
                None,
                &fixture.events
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied,
        );
        assert!(
            list(&fixture.database.connection, fixture.session, &[])
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn preserves_admitted_authority() {
    for (admitted, current) in [
        (WorkMode::Plan, WorkMode::Code),
        (WorkMode::Code, WorkMode::Plan),
    ] {
        let mut fixture = Fixture::with_mode(Permission::Full, admitted).await;
        let body: Vec<u8> = fixture
            .database
            .connection
            .prepare("SELECT config FROM session_revisions WHERE session=?1 AND revision=1")
            .await
            .unwrap()
            .query_row([fixture.session.to_string()])
            .await
            .and_then(|row| row.get(0))
            .unwrap();
        let mut config: SessionConfig = serde_json::from_slice(&body).unwrap();
        config.mode = current;
        command(
            &mut fixture.database,
            &fixture.events,
            Command::SetSessionConfig {
                session: fixture.session,
                expected_revision: 1,
                config,
            },
        )
        .await;
        let result = begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            None,
            &fixture.events,
        )
        .await;
        if admitted == WorkMode::Plan {
            assert_eq!(result.unwrap_err().code, ErrorCode::PermissionDenied);
        } else {
            assert_eq!(result.unwrap().state, ApprovalState::Approved);
        }
    }
}

#[tokio::test]
async fn rejects_incompatible_modes() {
    for owner in ["defaults", "revision"] {
        assert_rejected(owner).await;
    }
}

#[tokio::test]
async fn rejects_incompatible_archives_during_upgrade() {
    for owner in ["event", "receipt"] {
        assert_rejected(owner).await;
    }
}

async fn assert_rejected(owner: &str) {
    let fixture = Fixture::with_mode(Permission::Full, WorkMode::Code).await;
    let db = &fixture.database.connection;
    let body: Vec<u8> = db
        .prepare("SELECT config FROM session_revisions WHERE session=?1 AND revision=1")
        .await
        .unwrap()
        .query_row([fixture.session.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    let mut config: serde_json::Value = serde_json::from_slice(&body).unwrap();
    config.as_object_mut().unwrap().remove("mode");
    let defaults = json!({"revision": 1, "config": config});
    match owner {
        "defaults" => {
            db.execute(
                "INSERT INTO defaults(singleton,revision,config) VALUES(1,1,?1)",
                [serde_json::to_vec(&config).unwrap()],
            )
            .await
            .unwrap();
        }
        "revision" => {
            db.execute(
                "UPDATE session_revisions SET config=?1 WHERE session=?2",
                params![
                    serde_json::to_vec(&config).unwrap(),
                    fixture.session.to_string()
                ],
            )
            .await
            .unwrap();
        }
        "event" => {
            let transaction = db.unchecked_transaction().await.unwrap();
            let sequence = crate::store::database::counters::next(
                &transaction,
                crate::store::database::counters::Namespace::Events,
            )
            .await
            .unwrap();
            transaction
                .execute(
                    "INSERT INTO events(cursor,body) VALUES(?1,?2)",
                    params![
                        sequence as i64,
                        serde_json::to_vec(&json!({"kind": "defaults_changed", "data": defaults}))
                            .unwrap(),
                    ],
                )
                .await
                .unwrap();
            transaction.commit().await.unwrap();
        }
        "receipt" => {
            db.execute(
            "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)",
            params![&fixture.database.node.0[..], RequestId::new().to_string(), b"fixture".as_slice(),
                json!({"Ok": {"kind": "defaults", "data": defaults}}).to_string()],
        ).await.unwrap();
        }
        _ => unreachable!(),
    }
    if matches!(owner, "event" | "receipt") {
        // An actual pending upgrade requires complete archive validation.
        db.execute_batch("DROP INDEX events_kind").await.unwrap();
    }
    let node = fixture.database.node;
    fixture.database.close().await.unwrap();
    let before = std::fs::read(&fixture.path).unwrap();
    assert!(Database::open(&fixture.path, node, None).await.is_err());
    assert_eq!(std::fs::read(&fixture.path).unwrap(), before);
}
