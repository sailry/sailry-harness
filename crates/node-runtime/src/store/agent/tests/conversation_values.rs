use super::*;
use fixture::Fixture;
use sailry_protocol::plugin::{self, storage::ConversationEntry, transaction::Operation};
use serde_json::{Value, json};

async fn prepare(fixture: &Fixture) -> plugin::Context {
    let profile = fixture._root.path().join("profile");
    std::fs::create_dir(&profile).unwrap();
    fixture::package(&fixture.database, &profile, "goals").await;
    let package = crate::store::plugins::get(&fixture.database.connection, "goals")
        .await
        .unwrap()
        .unwrap();
    let session = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    plugin::Context {
        package: package.summary.reference(),
        surface: Default::default(),
        worktree: Some(session.worktree),
        session: Some(session.id),
        turn: None,
        invocation: None,
    }
}

async fn execute(fixture: &mut Fixture, request: Request) -> Result<Output, Fault> {
    let (reply, response) = oneshot::channel();
    assert!(
        fixture
            .database
            .dispatch(fixture.database.node, request, reply, &fixture.events)
            .await
            .is_none()
    );
    response.await.unwrap()?.completion.await.unwrap()
}

async fn write(
    fixture: &mut Fixture,
    context: &plugin::Context,
    value: Option<Value>,
    revision: u64,
) -> ConversationEntry {
    let command = match value {
        Some(value) => Command::WritePluginConversationValue {
            key: "state".into(),
            value,
            expected_revision: revision,
        },
        None => Command::RemovePluginConversationValue {
            key: "state".into(),
            expected_revision: revision,
        },
    };
    let request = Request::new(fixture.database.node, command).with_plugin(context.clone());
    let Output::PluginConversationValue(entry) = execute(fixture, request).await.unwrap() else {
        panic!("entry expected")
    };
    entry
}

async fn read(fixture: &Fixture, session: SessionId) -> ConversationEntry {
    crate::store::plugins::storage::conversation::read(
        &fixture.database.connection,
        "goals",
        session,
        "state",
    )
    .await
    .unwrap()
}

async fn fork(fixture: &mut Fixture, source: SessionId, through: TurnId) -> SessionId {
    let Output::Session(session) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: source,
            through,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    session.id
}

#[tokio::test]
async fn branches_preserve_prefixes() {
    let mut fixture = Fixture::new().await;
    let context = prepare(&fixture).await;
    let source = fixture.session;
    let first = fixture.completed(&["First result"]).await;
    let initial = write(
        &mut fixture,
        &context,
        Some(json!({"phase":"initial","text":"Evidence 中文 🙂"})),
        0,
    )
    .await;
    let head = fixture.completed(&["Second result"]).await;
    let latest = write(
        &mut fixture,
        &context,
        Some(json!({"phase":"final"})),
        initial.revision,
    )
    .await;
    let prefix = fork(&mut fixture, source, first).await;
    let full = fork(&mut fixture, source, head).await;
    assert_eq!(read(&fixture, prefix).await.value, initial.value);
    assert!(read(&fixture, prefix).await.restored);
    assert_eq!(read(&fixture, full).await.value, latest.value);
    assert_eq!(read(&fixture, source).await, latest);
    assert_eq!(
        crate::store::plugins::storage::conversation::sequence(&fixture.database.connection)
            .await
            .unwrap(),
        2
    );

    let cleared = write(&mut fixture, &context, None, latest.revision).await;
    assert!(!cleared.present);
    let tombstone = fork(&mut fixture, source, head).await;
    assert!(!read(&fixture, tombstone).await.present);
    assert!(read(&fixture, tombstone).await.restored);
    let next = fixture.completed(&["New result"]).await;
    write(
        &mut fixture,
        &context,
        Some(json!({"phase":"replacement"})),
        cleared.revision,
    )
    .await;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: source,
            through: None,
            expected_head: next,
            expected_revision: 1,
        },
    )
    .await;
    let nested = fork(&mut fixture, full, first).await;
    assert_eq!(read(&fixture, nested).await.value, initial.value);
    assert_eq!(read(&fixture, full).await.value, latest.value);
    assert_eq!(read(&fixture, prefix).await.value, initial.value);
    assert!(!read(&fixture, source).await.present);
}

#[tokio::test]
async fn rewind_advances_revisions() {
    let mut fixture = Fixture::new().await;
    let context = prepare(&fixture).await;
    let first = fixture.completed(&["First result"]).await;
    let initial = write(&mut fixture, &context, Some(json!({"phase":"initial"})), 0).await;
    let head = fixture.completed(&["Second result"]).await;
    let latest = write(
        &mut fixture,
        &context,
        Some(json!({"phase":"final"})),
        initial.revision,
    )
    .await;
    let request = Request::new(
        fixture.database.node,
        Command::RewindConversation {
            session: fixture.session,
            through: Some(first),
            expected_head: head,
            expected_revision: 1,
        },
    );
    let source = fixture.session;
    let branch = fork(&mut fixture, source, head).await;
    let output = execute(&mut fixture, request.clone()).await.unwrap();
    let Output::Rewound(rewind) = &output else {
        panic!("rewind expected")
    };
    let restored = read(&fixture, fixture.session).await;
    assert!(rewind.backup.is_none());
    assert_eq!(restored.value, initial.value);
    assert!(restored.restored);
    assert_eq!(restored.revision, latest.revision + 1);
    assert_eq!(read(&fixture, branch).await.value, latest.value);
    assert!(read(&fixture, branch).await.restored);
    assert_eq!(execute(&mut fixture, request).await.unwrap(), output);
    assert_eq!(read(&fixture, fixture.session).await, restored);
    let stale = Request::new(
        fixture.database.node,
        Command::WritePluginConversationValue {
            key: "state".into(),
            value: json!({"phase":"stale"}),
            expected_revision: latest.revision,
        },
    )
    .with_plugin(context);
    assert_eq!(
        execute(&mut fixture, stale).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let prefix = fork(&mut fixture, source, first).await;
    let Output::Rewound(empty) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: fixture.session,
            through: None,
            expected_head: first,
            expected_revision: rewind.revision,
        },
    )
    .await
    else {
        panic!("rewind expected")
    };
    let absent = read(&fixture, fixture.session).await;
    assert!(empty.backup.is_none());
    assert!(!absent.present);
    assert!(absent.restored);
    assert_eq!(absent.revision, restored.revision + 1);
    assert_eq!(read(&fixture, prefix).await.value, initial.value);
}

#[tokio::test]
async fn backup_keeps_clear() {
    let mut fixture = Fixture::new().await;
    let context = prepare(&fixture).await;
    let first = fixture.completed(&["First result"]).await;
    let initial = write(&mut fixture, &context, Some(json!(true)), 0).await;
    let head = fixture.completed(&["Second result"]).await;
    write(&mut fixture, &context, None, initial.revision).await;
    let source = fixture.session;
    let branch = fork(&mut fixture, source, head).await;
    let Output::Rewound(rewind) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: fixture.session,
            through: Some(first),
            expected_head: head,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("rewind expected")
    };
    assert_eq!(read(&fixture, fixture.session).await.value, initial.value);
    assert!(rewind.backup.is_none());
    let backup = read(&fixture, branch).await;
    assert!(!backup.present);
    assert!(backup.restored);
    assert_eq!(backup.value, Value::Null);
}

#[tokio::test]
async fn empty_rewind_clears_unanchored_values() {
    let mut fixture = Fixture::new().await;
    let context = prepare(&fixture).await;
    let initial = write(&mut fixture, &context, Some(json!("before input")), 0).await;
    let head = fixture.completed(&["First result"]).await;
    let source = fixture.session;
    let branch = fork(&mut fixture, source, head).await;
    let Output::Rewound(rewind) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: fixture.session,
            through: None,
            expected_head: head,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("rewind expected")
    };
    let absent = read(&fixture, fixture.session).await;
    assert!(rewind.backup.is_none());
    assert!(!absent.present);
    assert!(absent.restored);
    assert_eq!(absent.revision, initial.revision + 1);
    assert_eq!(read(&fixture, branch).await.value, initial.value);
    let references: i64 = fixture
        .database
        .connection
        .prepare("SELECT count(*) FROM plugin_conversation_history WHERE session=?1")
        .await
        .unwrap()
        .query_row([fixture.session.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(references, 0);
}

#[tokio::test]
async fn admission_anchors_state() {
    for existing in [false, true] {
        let mut fixture = Fixture::new().await;
        let context = prepare(&fixture).await;
        let initial = if existing {
            let turn = fixture.completed(&["Previous result"]).await;
            write(&mut fixture, &context, Some(json!("previous")), 0).await;
            Some(turn)
        } else {
            None
        };
        let request = Request::new(
            fixture.database.node,
            Command::PluginTransaction {
                operations: vec![
                    Operation::ConversationWrite {
                        key: "state".into(),
                        value: json!("new input"),
                        expected_revision: u64::from(existing),
                    },
                    Operation::Submit {
                        session: fixture.session,
                        expected_revision: 1,
                        message: "New input".into(),
                    },
                ],
            },
        )
        .with_plugin(context);
        let output = execute(&mut fixture, request.clone()).await.unwrap();
        let Output::PluginTransaction(outputs) = &output else {
            panic!("transaction expected")
        };
        let Output::QueuedTurn(turn) = &outputs[1] else {
            panic!("turn expected")
        };
        let anchor: String = fixture.database.connection.prepare("SELECT turn FROM plugin_conversation_history WHERE session=?1 ORDER BY record DESC LIMIT 1").await.unwrap().query_row([fixture.session.to_string()]).await.and_then(|row| row.get(0)).unwrap();
        assert_eq!(anchor, turn.id.to_string());
        assert_eq!(execute(&mut fixture, request).await.unwrap(), output);
        if let Some(initial) = initial {
            let source = fixture.session;
            let branch = fork(&mut fixture, source, initial).await;
            assert_eq!(read(&fixture, branch).await.value, json!("previous"));
        }
        let invocation = runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(invocation.turn.id, turn.id);
        runs::finish(
            &mut fixture.database,
            turn.id,
            Status::Completed,
            None,
            &fixture.events,
        )
        .await
        .unwrap();
        let source = fixture.session;
        let full = fork(&mut fixture, source, turn.id).await;
        assert_eq!(read(&fixture, full).await.value, json!("new input"));
        command(
            &mut fixture.database,
            &fixture.events,
            Command::RewindConversation {
                session: fixture.session,
                through: None,
                expected_head: turn.id,
                expected_revision: 1,
            },
        )
        .await;
        assert!(!read(&fixture, fixture.session).await.present);
    }
}
