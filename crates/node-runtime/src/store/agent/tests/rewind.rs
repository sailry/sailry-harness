use super::{fixture::Fixture, *};
use serde_json::json;

fn rewind(fixture: &mut Fixture, through: Option<TurnId>, head: TurnId, revision: u64) -> Rewind {
    let Output::Rewound(result) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: fixture.session,
            through,
            expected_head: head,
            expected_revision: revision,
        },
    ) else {
        panic!("rewind expected")
    };
    *result
}

#[test]
fn preserves_shared_history() {
    let mut fixture = Fixture::new();
    let source = fixture.session;
    let mut turns = Vec::new();
    for index in 1..=3 {
        let mut event = AdkEvent::new(format!("rewind-{index}"));
        event.author = "assistant".into();
        event.set_content(
            adk_core::Content::new("model").with_text(format!("Message {index} 中文 🙂")),
        );
        event.actions.state_delta.extend([
            ("value".into(), json!(index)),
            ("app:shared".into(), json!(index)),
            ("user:shared".into(), json!(index)),
        ]);
        turns.push(fixture.append([event]));
    }
    let before = fixture.read(None, 100).page;
    let original = crate::store::commands::session(&fixture.database.connection, source).unwrap();
    let result = rewind(&mut fixture, Some(turns[0]), turns[2], 1);
    assert_eq!(result.revision, 2);
    assert_eq!(result.through, Some(turns[0]));
    assert_eq!(result.backup.config, original.config);
    assert_eq!(
        result.backup.fork,
        Some(Fork {
            session: source,
            through: turns[2]
        })
    );
    assert_eq!(original.activity.run.as_ref(), before.runs.last());
    assert_eq!(
        crate::store::commands::session(&fixture.database.connection, source).unwrap(),
        Session {
            activity: sailry_protocol::activity::Summary {
                run: Some(before.runs[0].clone()),
                ..original.activity.clone()
            },
            ..original
        }
    );
    let page = fixture.read(None, 100).page;
    assert_eq!(page.revision, 2);
    assert_eq!(page.entries, before.entries[..1]);
    assert_eq!(page.runs, before.runs[..1]);
    assert_eq!(page.next_before, None);
    let backup = history::pages::read(&fixture.database.connection, result.backup.id, None, 100)
        .unwrap()
        .page;
    assert_eq!(backup.entries, before.entries);
    assert_eq!(backup.revision, 1);
    assert_eq!(backup.runs.len(), 3);
    assert!(backup.runs.iter().all(|run| run.origin == Some(source)));
    let state = history::state(&fixture.database.connection, source).unwrap();
    assert_eq!(state["value"], 1);
    assert_eq!(state["app:shared"], 3);
    assert_eq!(state["user:shared"], 3);
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(
        history::state(&fixture.database.connection, source).unwrap(),
        state
    );
    assert_eq!(
        history::state(&fixture.database.connection, result.backup.id).unwrap()["value"],
        3
    );
    assert_eq!(
        fixture
            .database
            .connection
            .query_row("SELECT count(*) FROM agent_events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        history::pages::turn(&fixture.database.connection, source, turns[2], None, 100)
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
    assert_eq!(fixture.read(None, 1).page.entries, before.entries[..1]);
    assert_eq!(
        runs::stop(&fixture.database.connection, turns[2])
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
    let cleared = rewind(&mut fixture, None, turns[0], 2);
    assert_eq!(cleared.revision, 3);
    assert!(fixture.read(None, 100).page.runs.is_empty());
    assert!(fixture.read(None, 100).page.entries.is_empty());
    let state = history::state(&fixture.database.connection, source).unwrap();
    assert!(!state.contains_key("value"));
    assert_eq!(state["app:shared"], 3);
    let next = fixture.completed(&["New continuation"]);
    let resumed = fixture.read(None, 100).page;
    assert_eq!(resumed.revision, 3);
    assert_eq!(resumed.runs.len(), 1);
    assert_eq!(resumed.runs[0].turn, next);
    assert!(resumed.runs[0].sequence > before.runs[2].sequence);
    assert_eq!(
        history::pages::read(&fixture.database.connection, result.backup.id, None, 100)
            .unwrap()
            .page,
        backup
    );
}

#[test]
fn rejects_conflicting_work() {
    let mut fixture = Fixture::new();
    let first = fixture.completed(&["First"]);
    let head = fixture.completed(&["Second"]);
    let attempt = |fixture: &Fixture, through, expected_head, revision| {
        crate::store::sessions::rewind(
            &fixture.database.connection,
            fixture.session,
            through,
            expected_head,
            revision,
        )
        .unwrap_err()
        .code
    };
    assert_eq!(
        attempt(&fixture, Some(first), head, 2),
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        attempt(&fixture, None, first, 1),
        ErrorCode::RevisionConflict
    );
    assert_eq!(attempt(&fixture, Some(head), head, 1), ErrorCode::Conflict);
    assert_eq!(
        attempt(&fixture, Some(TurnId::new()), head, 1),
        ErrorCode::NotFound
    );
    let queued = fixture.queued();
    assert_eq!(attempt(&fixture, Some(first), head, 1), ErrorCode::Busy);
    runs::start(&fixture.database.connection, queued).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(attempt(&fixture, Some(first), head, 1), ErrorCode::Busy);
    runs::stop(&fixture.database.connection, queued).unwrap();
    assert_eq!(attempt(&fixture, Some(first), head, 1), ErrorCode::Busy);
    runs::finish(
        &mut fixture.database,
        queued,
        Status::Cancelled,
        None,
        &fixture.events,
    )
    .unwrap();
    assert_eq!(
        attempt(&fixture, Some(first), head, 1),
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        fixture
            .database
            .connection
            .query_row("SELECT count(*) FROM sessions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let before = fixture.read(None, 100).page;
    assert!(before.runs.iter().any(|run| run.turn == queued));
    let result = rewind(&mut fixture, Some(first), queued, 1);
    assert_eq!(fixture.read(None, 100).page.runs.len(), 1);
    let backup = history::pages::read(&fixture.database.connection, result.backup.id, None, 100)
        .unwrap()
        .page;
    assert_eq!(backup.entries, before.entries);
    // Empty cancellations retain lifecycle metadata without counting as history messages.
    assert_eq!(backup.runs.len(), 3);
}

#[test]
fn preserves_source_branch() {
    let mut fixture = Fixture::new();
    let first = fixture.completed(&["First"]);
    let last = fixture.completed(&["Last"]);
    let before = fixture.read(None, 100).page;
    let source = fixture.session;
    let Output::Session(branch) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: source,
            through: last,
            expected_revision: 1,
        },
    ) else {
        panic!("fork expected")
    };
    fixture.session = branch.id;
    assert_eq!(
        crate::store::sessions::rewind(
            &fixture.database.connection,
            branch.id,
            Some(TurnId::new()),
            last,
            1
        )
        .unwrap_err()
        .code,
        ErrorCode::NotFound
    );
    let result = rewind(&mut fixture, Some(first), last, 1);
    let page = fixture.read(None, 100).page;
    assert_eq!(page.runs[0].origin, Some(source));
    assert_eq!(page.entries, before.entries[..1]);
    let backup = history::pages::read(&fixture.database.connection, result.backup.id, None, 100)
        .unwrap()
        .page;
    assert_eq!(backup.entries, before.entries);
    assert!(backup.runs.iter().all(|run| run.origin == Some(source)));
    assert_eq!(
        history::pages::read(&fixture.database.connection, source, None, 100)
            .unwrap()
            .page,
        before
    );
}

#[test]
#[cfg(unix)]
fn preserves_revoked_credentials() {
    let mut fixture = Fixture::new();
    let first = fixture.completed(&["First"]);
    let head = fixture.completed(&["Second"]);
    let mut config = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .unwrap()
        .config;
    let credential = CredentialId::new();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::PutCredential {
            id: credential,
            provider: config.provider,
            expected_revision: 0,
            secret: Secret::new("isolated-rewind-fixture".into()),
            expires_at_ms: None,
        },
    );
    config.credential = Some(CredentialRef {
        node: fixture.database.node,
        id: credential,
    });
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: fixture.session,
            expected_revision: 1,
            config: config.clone(),
        },
    );
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RevokeCredential {
            id: credential,
            expected_revision: 1,
        },
    );
    let result = rewind(&mut fixture, Some(first), head, 1);
    assert_eq!(result.backup.config, config);
    assert_eq!(result.backup.revision, 1);
    assert_eq!(
        crate::store::commands::session(&fixture.database.connection, fixture.session)
            .unwrap()
            .revision,
        2
    );
    assert_eq!(
        crate::store::commands::validate_config(
            &fixture.database.connection,
            fixture.database.node,
            &result.backup.config
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        history::pages::read(&fixture.database.connection, result.backup.id, None, 100)
            .unwrap()
            .page
            .runs
            .len(),
        2
    );
}
