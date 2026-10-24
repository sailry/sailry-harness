use super::*;
use sailry_protocol::{connection::Sharing, database, tool::Operation};

async fn fixture() -> (Fixture, Binding, database::Profile) {
    let mut fixture =
        Fixture::with_package(Permission::Ask, WorkMode::Code, Some("databases")).await;
    let db = &fixture.database.connection;
    let profile = database::Profile {
        sharing: Some(Sharing::Global),
        id: DatabaseId::new(),
        revision: 1,
        name: "Approved target".into(),
        connection: database::Connection::Sqlite {
            path: fixture
                ._directory
                .path()
                .join("approved.sqlite3")
                .to_str()
                .unwrap()
                .into(),
        },
        read_only: false,
    };
    db.execute(
        "INSERT INTO database_profiles(id,body,password) VALUES(?1,?2,'')",
        params![profile.id.to_string(), encode(&profile).unwrap()],
    )
    .await
    .unwrap();
    let owner = super::super::super::plugins::read(db, fixture.turn)
        .await
        .unwrap()
        .into_iter()
        .find(|package| package.name == "databases")
        .unwrap();
    fixture.request = ToolConfirmationRequest {
        acting: None,
        origin: None,
        tool_name: crate::plugins::tools::alias("databases", "database_execute"),
        function_call_id: Some("execute-approved-target".into()),
        args: json!({"connection":profile.id,"sql":"DELETE FROM items","row_limit":2,"timeout_ms":1000}),
    };
    let mut event = AdkEvent::new("target");
    event.author = "assistant".into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: fixture.request.function_call_id.clone(),
            name: fixture.request.tool_name.clone(),
            args: fixture.request.args.clone(),
            thought_signature: None,
        }],
    });
    history::append(
        &mut fixture.database,
        fixture.session,
        fixture.turn,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    let binding = Binding {
        owner,
        tool: fixture.request.tool_name.clone(),
        operation: Some(Operation::ExecuteDatabase),
    };
    (fixture, binding, profile)
}

fn command(profile: &database::Profile) -> Command {
    Command::QueryDatabase {
        read_only: false,
        profile: profile.id,
        expected_revision: profile.revision,
        database: None,
        sql: "DELETE FROM items".into(),
        row_limit: 2,
        timeout_ms: 1000,
    }
}

#[tokio::test]
async fn freezes_connection_before_user_decision() {
    let (mut fixture, binding, profile) = fixture().await;
    let approval = begin_for(
        &mut fixture.database,
        fixture.turn,
        &fixture.request,
        Some(&binding),
        Capture {
            prepared: Some(&command(&profile)),
            ..Default::default()
        },
        &fixture.events,
    )
    .await
    .unwrap();
    let saved: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT prepared FROM agent_approvals WHERE id=?1")
        .await
        .unwrap()
        .query_row([approval.id.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    let captured: Option<Prepared> = serde_json::from_slice(&saved).unwrap();
    assert!(captured.unwrap().matches(&command(&profile)).unwrap());
    let mut changed = profile.clone();
    changed.revision += 1;
    changed.connection = database::Connection::Sqlite {
        path: fixture
            ._directory
            .path()
            .join("unapproved.sqlite3")
            .to_str()
            .unwrap()
            .into(),
    };
    fixture
        .database
        .connection
        .execute(
            "UPDATE database_profiles SET body=?1",
            [encode(&changed).unwrap()],
        )
        .await
        .unwrap();
    fixture.approve(approval.id).await;
    let error = authorize(
        &mut fixture.database.connection,
        fixture.turn,
        &fixture.request,
        Some(&command(&changed)),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    let claimed: bool = fixture
        .database
        .connection
        .prepare("SELECT claimed FROM agent_approvals WHERE id=?1")
        .await
        .unwrap()
        .query_row([approval.id.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert!(!claimed);
    // The original frozen command can be claimed, but the service rejects its stale revision.
    authorize(
        &mut fixture.database.connection,
        fixture.turn,
        &fixture.request,
        Some(&command(&profile)),
    )
    .await
    .unwrap();
    assert_eq!(
        crate::store::databases::prepare(&fixture.database, &command(&profile))
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::RevisionConflict
    );
}

#[tokio::test]
async fn accepts_unchanged_target_and_rejects_unbound_history() {
    for legacy in [false, true] {
        let (mut fixture, binding, profile) = fixture().await;
        let approval = begin_for(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&binding),
            Capture {
                prepared: Some(&command(&profile)),
                ..Default::default()
            },
            &fixture.events,
        )
        .await
        .unwrap();
        if legacy {
            fixture
                .database
                .connection
                .execute(
                    "UPDATE agent_approvals SET prepared=?1",
                    [b"null".as_slice()],
                )
                .await
                .unwrap();
        }
        fixture.approve(approval.id).await;
        let result = authorize(
            &mut fixture.database.connection,
            fixture.turn,
            &fixture.request,
            Some(&command(&profile)),
        )
        .await;
        if legacy {
            assert_eq!(result.unwrap_err().code, ErrorCode::Conflict);
        } else {
            result.unwrap();
        }
    }
}

#[tokio::test]
async fn retains_revision_fence_across_workflow_steps() {
    let (mut fixture, binding, profile) = fixture().await;
    let approval = begin(
        &mut fixture.database,
        fixture.turn,
        &fixture.request,
        Some(&binding),
        &fixture.events,
    )
    .await
    .unwrap();
    fixture.approve(approval.id).await;
    authorize(
        &mut fixture.database.connection,
        fixture.turn,
        &fixture.request,
        Some(&command(&profile)),
    )
    .await
    .unwrap();
    let mut second = command(&profile);
    if let Command::QueryDatabase { sql, .. } = &mut second {
        *sql = "INSERT INTO items VALUES (2)".into();
    }
    connection_access(
        &fixture.database.connection,
        fixture.turn,
        &fixture.request,
        &second,
    )
    .await
    .unwrap();
    if let Command::QueryDatabase {
        expected_revision, ..
    } = &mut second
    {
        *expected_revision += 1;
    }
    assert_eq!(
        connection_access(
            &fixture.database.connection,
            fixture.turn,
            &fixture.request,
            &second
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    let mut changed = fixture.request.clone();
    changed.args["sql"] = json!("DROP TABLE items");
    assert!(
        connection_access(
            &fixture.database.connection,
            fixture.turn,
            &changed,
            &command(&profile)
        )
        .await
        .is_err()
    );
}
