use super::*;
use fixture::Fixture;
use sailry_protocol::plugin::{self, storage::ConversationEntry, transaction::Operation};
use serde_json::{Value, json};

fn prepare(fixture: &Fixture) -> plugin::Context {
    let profile = fixture._root.path().join("profile");
    std::fs::create_dir(&profile).unwrap();
    fixture::package(&fixture.database, &profile, "goals");
    let package = crate::store::plugins::get(&fixture.database.connection, "goals")
        .unwrap()
        .unwrap();
    let session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    plugin::Context {
        package: package.summary.reference(),
        surface: Default::default(),
        worktree: Some(session.worktree),
        session: Some(session.id),
        turn: None,
        invocation: None,
    }
}

fn execute(fixture: &mut Fixture, request: Request) -> Result<Output, Fault> {
    let (reply, response) = oneshot::channel();
    assert!(
        fixture
            .database
            .dispatch(fixture.database.node, request, reply, &fixture.events)
            .is_none()
    );
    response
        .blocking_recv()
        .unwrap()?
        .completion
        .blocking_recv()
        .unwrap()
}

fn write(
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
    let Output::PluginConversationValue(entry) = execute(fixture, request).unwrap() else {
        panic!("entry expected")
    };
    entry
}

fn read(fixture: &Fixture, session: SessionId) -> ConversationEntry {
    crate::store::plugins::storage::conversation::read(
        &fixture.database.connection,
        "goals",
        session,
        "state",
    )
    .unwrap()
}

fn fork(fixture: &mut Fixture, source: SessionId, through: TurnId) -> SessionId {
    let Output::Session(session) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: source,
            through,
            expected_revision: 1,
        },
    ) else {
        panic!("session expected")
    };
    session.id
}

#[test]
fn branches_preserve_prefixes() {
    let mut fixture = Fixture::new();
    let context = prepare(&fixture);
    let source = fixture.session;
    let first = fixture.completed(&["First result"]);
    let initial = write(
        &mut fixture,
        &context,
        Some(json!({"phase":"initial","text":"Evidence 中文 🙂"})),
        0,
    );
    let head = fixture.completed(&["Second result"]);
    let latest = write(
        &mut fixture,
        &context,
        Some(json!({"phase":"final"})),
        initial.revision,
    );
    let prefix = fork(&mut fixture, source, first);
    let full = fork(&mut fixture, source, head);
    assert_eq!(read(&fixture, prefix).value, initial.value);
    assert!(read(&fixture, prefix).restored);
    assert_eq!(read(&fixture, full).value, latest.value);
    assert_eq!(read(&fixture, source), latest);
    assert_eq!(
        crate::store::plugins::storage::conversation::sequence(&fixture.database.connection)
            .unwrap(),
        2
    );

    let cleared = write(&mut fixture, &context, None, latest.revision);
    assert!(!cleared.present);
    let tombstone = fork(&mut fixture, source, head);
    assert!(!read(&fixture, tombstone).present);
    assert!(read(&fixture, tombstone).restored);
    let next = fixture.completed(&["New result"]);
    write(
        &mut fixture,
        &context,
        Some(json!({"phase":"replacement"})),
        cleared.revision,
    );
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: source,
            through: None,
            expected_head: next,
            expected_revision: 1,
        },
    );
    let nested = fork(&mut fixture, full, first);
    assert_eq!(read(&fixture, nested).value, initial.value);
    assert_eq!(read(&fixture, full).value, latest.value);
    assert_eq!(read(&fixture, prefix).value, initial.value);
    assert!(!read(&fixture, source).present);
}

#[test]
fn rewind_advances_revisions() {
    let mut fixture = Fixture::new();
    let context = prepare(&fixture);
    let first = fixture.completed(&["First result"]);
    let initial = write(&mut fixture, &context, Some(json!({"phase":"initial"})), 0);
    let head = fixture.completed(&["Second result"]);
    let latest = write(
        &mut fixture,
        &context,
        Some(json!({"phase":"final"})),
        initial.revision,
    );
    let request = Request::new(
        fixture.database.node,
        Command::RewindConversation {
            session: fixture.session,
            through: Some(first),
            expected_head: head,
            expected_revision: 1,
        },
    );
    let output = execute(&mut fixture, request.clone()).unwrap();
    let Output::Rewound(rewind) = &output else {
        panic!("rewind expected")
    };
    let restored = read(&fixture, fixture.session);
    assert_eq!(restored.value, initial.value);
    assert!(restored.restored);
    assert_eq!(restored.revision, latest.revision + 1);
    assert_eq!(read(&fixture, rewind.backup.id).value, latest.value);
    assert!(read(&fixture, rewind.backup.id).restored);
    assert_eq!(execute(&mut fixture, request).unwrap(), output);
    assert_eq!(read(&fixture, fixture.session), restored);
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
        execute(&mut fixture, stale).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let Output::Rewound(empty) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: fixture.session,
            through: None,
            expected_head: first,
            expected_revision: rewind.revision,
        },
    ) else {
        panic!("rewind expected")
    };
    let absent = read(&fixture, fixture.session);
    assert!(!absent.present);
    assert!(absent.restored);
    assert_eq!(absent.revision, restored.revision + 1);
    assert_eq!(read(&fixture, empty.backup.id).value, initial.value);
}

#[test]
fn backup_keeps_clear() {
    let mut fixture = Fixture::new();
    let context = prepare(&fixture);
    let first = fixture.completed(&["First result"]);
    let initial = write(&mut fixture, &context, Some(json!(true)), 0);
    let head = fixture.completed(&["Second result"]);
    write(&mut fixture, &context, None, initial.revision);
    let Output::Rewound(rewind) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: fixture.session,
            through: Some(first),
            expected_head: head,
            expected_revision: 1,
        },
    ) else {
        panic!("rewind expected")
    };
    assert_eq!(read(&fixture, fixture.session).value, initial.value);
    let backup = read(&fixture, rewind.backup.id);
    assert!(!backup.present);
    assert!(backup.restored);
    assert_eq!(backup.value, Value::Null);
}

#[test]
fn empty_rewind_clears_unanchored_values() {
    let mut fixture = Fixture::new();
    let context = prepare(&fixture);
    let initial = write(&mut fixture, &context, Some(json!("before input")), 0);
    let head = fixture.completed(&["First result"]);
    let Output::Rewound(rewind) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: fixture.session,
            through: None,
            expected_head: head,
            expected_revision: 1,
        },
    ) else {
        panic!("rewind expected")
    };
    let absent = read(&fixture, fixture.session);
    assert!(!absent.present);
    assert!(absent.restored);
    assert_eq!(absent.revision, initial.revision + 1);
    assert_eq!(read(&fixture, rewind.backup.id).value, initial.value);
    let references: i64 = fixture
        .database
        .connection
        .query_row(
            "SELECT count(*) FROM plugin_conversation_history WHERE session=?1",
            [fixture.session.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(references, 0);
}

#[test]
fn admission_anchors_state() {
    for existing in [false, true] {
        let mut fixture = Fixture::new();
        let context = prepare(&fixture);
        let initial = if existing {
            let turn = fixture.completed(&["Previous result"]);
            write(&mut fixture, &context, Some(json!("previous")), 0);
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
        let output = execute(&mut fixture, request.clone()).unwrap();
        let Output::PluginTransaction(outputs) = &output else {
            panic!("transaction expected")
        };
        let Output::QueuedTurn(turn) = &outputs[1] else {
            panic!("turn expected")
        };
        let anchor: String = fixture.database.connection.query_row(
            "SELECT turn FROM plugin_conversation_history WHERE session=?1 ORDER BY record DESC LIMIT 1",
            [fixture.session.to_string()], |row| row.get(0),
        ).unwrap();
        assert_eq!(anchor, turn.id.to_string());
        assert_eq!(execute(&mut fixture, request).unwrap(), output);
        if let Some(initial) = initial {
            let source = fixture.session;
            let branch = fork(&mut fixture, source, initial);
            assert_eq!(read(&fixture, branch).value, json!("previous"));
        }
        let invocation = runs::claim(&mut fixture.database, &fixture.events)
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
        .unwrap();
        let source = fixture.session;
        let full = fork(&mut fixture, source, turn.id);
        assert_eq!(read(&fixture, full).value, json!("new input"));
        command(
            &mut fixture.database,
            &fixture.events,
            Command::RewindConversation {
                session: fixture.session,
                through: None,
                expected_head: turn.id,
                expected_revision: 1,
            },
        );
        assert!(!read(&fixture, fixture.session).present);
    }
}
