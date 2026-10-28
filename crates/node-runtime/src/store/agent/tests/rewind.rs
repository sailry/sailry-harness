use super::{fixture::Fixture, *};
use crate::agent::canonical::{self, BlockKind};
use serde_json::json;

async fn rewind(
    fixture: &mut Fixture,
    through: Option<TurnId>,
    head: TurnId,
    revision: u64,
) -> Rewind {
    let Output::Rewound(result) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: fixture.session,
            through,
            expected_head: head,
            expected_revision: revision,
        },
    )
    .await
    else {
        panic!("rewind expected")
    };
    *result
}

#[tokio::test]
async fn keeps_canonical_records_without_backup() {
    let mut fixture = Fixture::new().await;
    let first = fixture.completed(&["Retained"]).await;
    let head = fixture.completed(&["Removed from view"]).await;
    let original = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let records = async |fixture: &Fixture| {
        let mut rows = fixture
            .database
            .connection
            .query(
                "SELECT r.body,t.plugins,s.config,e.body FROM turns t \
             JOIN requests r ON r.id=t.request AND r.caller=t.caller \
             JOIN session_revisions s ON s.session=t.session AND s.revision=t.revision \
             JOIN agent_events e ON e.turn=t.id WHERE t.session=?1 ORDER BY t.rowid,e.sequence",
                [fixture.session.to_string()],
            )
            .await
            .unwrap();
        let mut records = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            records.push((
                row.get::<Vec<u8>>(0).unwrap(),
                row.get::<Vec<u8>>(1).unwrap(),
                row.get::<Vec<u8>>(2).unwrap(),
                row.get::<Vec<u8>>(3).unwrap(),
            ));
        }
        records
    };
    let captured = records(&fixture).await;
    assert_eq!(captured.len(), 2);
    let retained = rewind(&mut fixture, Some(first), head, 1).await;
    assert!(retained.backup.is_none());
    assert_eq!(records(&fixture).await, captured);
    assert_eq!(fixture.read(None, 100).await.page.runs.len(), 1);
    let cleared = rewind(&mut fixture, None, first, 2).await;
    assert!(cleared.backup.is_none());
    assert_eq!(records(&fixture).await, captured);
    assert!(fixture.read(None, 100).await.page.runs.is_empty());
    let current = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(current.config, original.config);
    assert_eq!(current.revision, original.revision);
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM sessions")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn preserves_shared_history() {
    let mut fixture = Fixture::new().await;
    let source = fixture.session;
    let mut turns = Vec::new();
    for index in 1..=3 {
        let mut event = fixture::message(
            "assistant",
            canonical::Role::Assistant,
            vec![BlockKind::Text {
                text: format!("Message {index} 中文 🙂"),
            }],
        );
        event.metadata.state = Some(canonical::StateChanges {
            session: [("value".into(), json!(index))].into(),
            application: [("shared".into(), json!(index))].into(),
            user: [("shared".into(), json!(index))].into(),
        });
        turns.push(fixture.append([event]).await);
    }
    let before = fixture.read(None, 100).await.page;
    let original = crate::store::commands::session(&fixture.database.connection, source)
        .await
        .unwrap();
    let branch = fixture.fork(source, turns[2]).await;
    let result = rewind(&mut fixture, Some(turns[0]), turns[2], 1).await;
    assert!(result.backup.is_none());
    assert_eq!(result.revision, 2);
    assert_eq!(result.through, Some(turns[0]));
    assert_eq!(branch.config, original.config);
    assert_eq!(
        branch.fork,
        Some(Fork {
            session: source,
            through: turns[2]
        })
    );
    let mut latest = before.runs.last().unwrap().clone();
    assert!(latest.input.is_some());
    latest.input = None;
    assert_eq!(original.activity.run, Some(latest));
    let mut retained = before.runs[0].clone();
    retained.input = None;
    assert_eq!(
        crate::store::commands::session(&fixture.database.connection, source)
            .await
            .unwrap(),
        Session {
            activity: sailry_protocol::activity::Summary {
                run: Some(retained),
                ..original.activity.clone()
            },
            ..original
        }
    );
    let page = fixture.read(None, 100).await.page;
    assert_eq!(page.revision, 2);
    assert_eq!(page.entries, before.entries[..1]);
    assert_eq!(page.runs, before.runs[..1]);
    assert_eq!(page.next_before, None);
    let backup = history::pages::read(&fixture.database.connection, branch.id, None, 100)
        .await
        .unwrap()
        .page;
    assert_eq!(backup.entries, before.entries);
    assert_eq!(backup.revision, 1);
    assert_eq!(backup.runs.len(), 3);
    assert!(backup.runs.iter().all(|run| run.origin == Some(source)));
    let state = history::state(&fixture.database.connection, source)
        .await
        .unwrap();
    assert_eq!(state["value"], 1);
    assert_eq!(state["app:shared"], 3);
    assert_eq!(state["user:shared"], 3);
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(
        history::state(&fixture.database.connection, source)
            .await
            .unwrap(),
        state
    );
    assert_eq!(
        history::state(&fixture.database.connection, branch.id)
            .await
            .unwrap()["value"],
        3
    );
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM agent_events")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        history::pages::turn(&fixture.database.connection, source, turns[2], None, 100)
            .await
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
    assert_eq!(
        fixture.read(None, 1).await.page.entries,
        before.entries[..1]
    );
    assert_eq!(
        runs::stop(&fixture.database.connection, turns[2])
            .await
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
    let cleared = rewind(&mut fixture, None, turns[0], 2).await;
    assert!(cleared.backup.is_none());
    assert_eq!(cleared.revision, 3);
    assert!(fixture.read(None, 100).await.page.runs.is_empty());
    assert!(fixture.read(None, 100).await.page.entries.is_empty());
    let state = history::state(&fixture.database.connection, source)
        .await
        .unwrap();
    assert!(!state.contains_key("value"));
    assert_eq!(state["app:shared"], 3);
    let next = fixture.completed(&["New continuation"]).await;
    let resumed = fixture.read(None, 100).await.page;
    assert_eq!(resumed.revision, 3);
    assert_eq!(resumed.runs.len(), 1);
    assert_eq!(resumed.runs[0].turn, next);
    assert!(resumed.runs[0].sequence > before.runs[2].sequence);
    assert_eq!(
        history::pages::read(&fixture.database.connection, branch.id, None, 100)
            .await
            .unwrap()
            .page,
        backup
    );
}

#[tokio::test]
async fn rejects_conflicting_work() {
    let mut fixture = Fixture::new().await;
    let first = fixture.completed(&["First"]).await;
    let head = fixture.completed(&["Second"]).await;
    let attempt = async |fixture: &Fixture, through, expected_head, revision| {
        crate::store::sessions::rewind(
            &fixture.database.connection,
            fixture.session,
            through,
            expected_head,
            revision,
            false,
        )
        .await
        .unwrap_err()
        .code
    };
    assert_eq!(
        attempt(&fixture, Some(first), head, 2).await,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        attempt(&fixture, None, first, 1).await,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        attempt(&fixture, Some(head), head, 1).await,
        ErrorCode::Conflict
    );
    assert_eq!(
        attempt(&fixture, Some(TurnId::new()), head, 1).await,
        ErrorCode::NotFound
    );
    let queued = fixture.queued().await;
    assert_eq!(
        attempt(&fixture, Some(first), head, 1).await,
        ErrorCode::Busy
    );
    runs::start(&fixture.database.connection, queued)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        attempt(&fixture, Some(first), head, 1).await,
        ErrorCode::Busy
    );
    runs::stop(&fixture.database.connection, queued)
        .await
        .unwrap();
    assert_eq!(
        attempt(&fixture, Some(first), head, 1).await,
        ErrorCode::Busy
    );
    runs::finish(
        &mut fixture.database,
        queued,
        Status::Cancelled,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(
        attempt(&fixture, Some(first), head, 1).await,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM sessions")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        1
    );
    let before = fixture.read(None, 100).await.page;
    assert!(before.runs.iter().any(|run| run.turn == queued));
    let branch = fixture.fork(fixture.session, queued).await;
    let result = rewind(&mut fixture, Some(first), queued, 1).await;
    assert!(result.backup.is_none());
    assert_eq!(fixture.read(None, 100).await.page.runs.len(), 1);
    let backup = history::pages::read(&fixture.database.connection, branch.id, None, 100)
        .await
        .unwrap()
        .page;
    assert_eq!(backup.entries, before.entries);
    // Empty cancellations retain lifecycle metadata without counting as history messages.
    assert_eq!(backup.runs.len(), 3);
}

#[tokio::test]
async fn preserves_source_branch() {
    let mut fixture = Fixture::new().await;
    let first = fixture.completed(&["First"]).await;
    let last = fixture.completed(&["Last"]).await;
    let before = fixture.read(None, 100).await.page;
    let source = fixture.session;
    let Output::Session(branch) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: source,
            through: last,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("fork expected")
    };
    fixture.session = branch.id;
    assert_eq!(
        crate::store::sessions::rewind(
            &fixture.database.connection,
            branch.id,
            Some(TurnId::new()),
            last,
            1,
            false,
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::NotFound
    );
    let retained = fixture.fork(branch.id, last).await;
    let result = rewind(&mut fixture, Some(first), last, 1).await;
    assert!(result.backup.is_none());
    let page = fixture.read(None, 100).await.page;
    assert_eq!(page.runs[0].origin, Some(source));
    assert_eq!(page.entries, before.entries[..1]);
    let backup = history::pages::read(&fixture.database.connection, retained.id, None, 100)
        .await
        .unwrap()
        .page;
    assert_eq!(backup.entries, before.entries);
    assert!(backup.runs.iter().all(|run| run.origin == Some(source)));
    assert_eq!(
        history::pages::read(&fixture.database.connection, source, None, 100)
            .await
            .unwrap()
            .page,
        before
    );
}

#[tokio::test]
#[cfg(unix)]
async fn preserves_revoked_credentials() {
    let mut fixture = Fixture::new().await;
    let first = fixture.completed(&["First"]).await;
    let head = fixture.completed(&["Second"]).await;
    let mut config = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
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
    )
    .await;
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
    )
    .await;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RevokeCredential {
            id: credential,
            expected_revision: 1,
        },
    )
    .await;
    let branch = fixture.fork(fixture.session, head).await;
    let result = rewind(&mut fixture, Some(first), head, 1).await;
    assert!(result.backup.is_none());
    assert_eq!(branch.config, config);
    assert_eq!(branch.revision, 1);
    assert_eq!(
        crate::store::commands::session(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .revision,
        2
    );
    assert_eq!(
        crate::store::commands::validate_config(
            &fixture.database.connection,
            fixture.database.node,
            &branch.config
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        history::pages::read(&fixture.database.connection, branch.id, None, 100)
            .await
            .unwrap()
            .page
            .runs
            .len(),
        2
    );
}
