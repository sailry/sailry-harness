use super::{fixture::Fixture, *};
use crate::agent::canonical::{self, BlockKind};
use serde_json::json;
use std::collections::HashMap;

async fn fork(fixture: &mut Fixture, through: TurnId, revision: u64) -> Session {
    let Output::Session(session) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through,
            expected_revision: revision,
        },
    )
    .await
    else {
        panic!("forked session expected")
    };
    session
}

#[tokio::test]
async fn preserves_frozen_roles() {
    let mut fixture = Fixture::new().await;
    let original_roles =
        crate::store::sessions::roles::read(&fixture.database.connection, fixture.session, 1)
            .await
            .unwrap();
    assert_eq!(original_roles.profiles.len(), 2);
    assert_eq!(original_roles.profiles[0].key, "explore");
    assert!(original_roles.profiles[0].read_only);
    assert_eq!(original_roles.profiles[1].key, "research");
    assert!(original_roles.profiles[1].read_only);
    let through = fixture
        .completed(&["Completed before configuring roles"])
        .await;
    let profile = role::Profile {
        read_only: false,
        appearance: None,
        id: RoleId::new(),
        revision: 0,
        key: "review".into(),
        name: "Review".into(),
        description: "Review changes".into(),
        model: None,
        max_turns: Some(8),
        skills: vec![],
        tools: None,
        instructions: "Frozen instructions 中文 🙂".into(),
    };
    let Output::Role(mut role) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutRole {
            role: profile,
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("role expected")
    };
    let Output::Session(session) = command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionRoles {
            session: fixture.session,
            expected_revision: 1,
            roles: vec![role.reference()],
        },
    )
    .await
    else {
        panic!("session expected")
    };
    role.instructions = "Changed defaults".into();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::PutRole {
            role,
            expected_revision: 1,
        },
    )
    .await;
    let fork = fork(&mut fixture, through, 2).await;
    assert_eq!(fork.roles, session.roles);
    assert_eq!(
        fork.roles.profiles[0].instructions,
        "Frozen instructions 中文 🙂"
    );
    assert_eq!(
        crate::store::sessions::roles::read(&fixture.database.connection, session.id, 1)
            .await
            .unwrap(),
        original_roles
    );
    assert_eq!(
        crate::store::sessions::roles::read(&fixture.database.connection, fork.id, 1)
            .await
            .unwrap(),
        session.roles
    );
}

#[tokio::test]
async fn rebuilds_shared_history() {
    let mut fixture = Fixture::new().await;
    let original = fixture.session;
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
    let mut previous = crate::store::commands::session(&fixture.database.connection, original)
        .await
        .unwrap();
    previous.config.effort = Effort::Max;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: original,
            expected_revision: 1,
            config: previous.config.clone(),
        },
    )
    .await;
    let branched = fork(&mut fixture, turns[1], 2).await;
    assert_eq!(branched.config, previous.config);
    assert_eq!(branched.revision, 1);
    assert_eq!(
        branched.fork,
        Some(Fork {
            session: original,
            through: turns[1]
        })
    );
    assert_eq!(
        crate::store::commands::session(&fixture.database.connection, branched.id)
            .await
            .unwrap(),
        branched
    );
    let page = history::pages::read(&fixture.database.connection, branched.id, None, 100)
        .await
        .unwrap()
        .page;
    assert_eq!(page.entries, before.entries[..2]);
    assert_eq!(page.runs.len(), 2);
    assert!(
        page.runs
            .iter()
            .all(|run| run.session == branched.id && run.origin == Some(original))
    );
    assert_eq!(fixture.read(None, 100).await.page, before);
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
    let expected = HashMap::from([
        ("value".to_owned(), json!(2)),
        ("app:shared".to_owned(), json!(3)),
        ("user:shared".to_owned(), json!(3)),
    ]);
    assert_eq!(
        history::state(&fixture.database.connection, branched.id)
            .await
            .unwrap(),
        expected
    );
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(
        history::state(&fixture.database.connection, branched.id)
            .await
            .unwrap(),
        expected
    );
    fixture.session = branched.id;
    let nested = fork(&mut fixture, turns[0], 1).await;
    let inherited = history::pages::read(&fixture.database.connection, nested.id, None, 100)
        .await
        .unwrap()
        .page;
    assert_eq!(inherited.entries, before.entries[..1]);
    assert_eq!(inherited.runs[0].origin, Some(original));
    let invocation = fixture.queued().await;
    let request = sessions::Query {
        session: branched.id,
        recent: None,
        after: None,
    };
    let native = sessions::get(&fixture.database.connection, invocation, request)
        .await
        .unwrap();
    assert_eq!(native.records.len(), 2);
    assert_eq!(native.state, expected);
    let other = fixture.completed(&["Independent branch content"]).await;
    assert!(
        history::pages::read(&fixture.database.connection, branched.id, None, 100)
            .await
            .unwrap()
            .page
            .runs
            .iter()
            .any(|run| run.turn == other && run.origin.is_none())
    );
    assert_eq!(
        history::pages::read(&fixture.database.connection, original, None, 100)
            .await
            .unwrap()
            .page,
        before
    );
    assert_eq!(
        history::pages::read(&fixture.database.connection, nested.id, None, 100)
            .await
            .unwrap()
            .page,
        inherited
    );
}

#[tokio::test]
async fn validates_before_creation() {
    let mut fixture = Fixture::new().await;
    let first = fixture.completed(&["First"]).await;
    let queued = fixture.queued().await;
    let attempt = async |fixture: &Fixture, source, through, revision| {
        crate::store::sessions::fork(&fixture.database.connection, source, through, revision)
            .await
            .unwrap_err()
            .code
    };
    assert_eq!(
        attempt(&fixture, fixture.session, first, 2).await,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        attempt(&fixture, fixture.session, queued, 1).await,
        ErrorCode::Busy
    );
    assert_eq!(
        attempt(&fixture, fixture.session, TurnId::new(), 1).await,
        ErrorCode::NotFound
    );
    let branch = fork(&mut fixture, first, 1).await;
    assert_eq!(
        attempt(&fixture, branch.id, queued, 1).await,
        ErrorCode::WrongTarget
    );
    runs::start(&fixture.database.connection, queued)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        attempt(&fixture, fixture.session, queued, 1).await,
        ErrorCode::Busy
    );
    // A closed prefix can be forked while a later source turn is active.
    let other = fork(&mut fixture, first, 1).await;
    assert_eq!(
        history::pages::read(&fixture.database.connection, other.id, None, 100)
            .await
            .unwrap()
            .page
            .runs
            .len(),
        1
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
        3
    );
}

#[tokio::test]
async fn paginates_inherited_turns() {
    let mut fixture = Fixture::new().await;
    let original = fixture.session;
    let first = fixture.completed(&["Inherited 中文 🙂"; 105]).await;
    let before = fixture.read(None, 100).await;
    let branch = fork(&mut fixture, first, 1).await;
    fixture
        .database
        .connection
        .execute("DELETE FROM conversation_events", ())
        .await
        .unwrap();
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM conversation_events")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        210
    );
    for (sql, values) in [(
        history::EVENTS_QUERY.to_owned(),
        vec![
            turso::Value::Text(branch.id.to_string()),
            turso::Value::Integer(i64::MAX),
            turso::Value::Integer(20),
        ],
    )] {
        let mut query = fixture.database.connection.prepare(&sql).await.unwrap();
        let mut rows = query.query(values.clone()).await.unwrap();
        let mut count = 0;
        while rows.next().await.unwrap().is_some() {
            count += 1;
        }
        drop(rows);
        assert_eq!(count, 20);
        drop(query);
        let mut rows = fixture
            .database
            .connection
            .query(format!("EXPLAIN QUERY PLAN {sql}"), values)
            .await
            .unwrap();
        let mut plan = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            plan.push(row.get::<String>(3).unwrap());
        }
        // The native SDK exposes plans, but not executed sort or full-scan counters.
        assert!(
            !plan.iter().any(|step| step.contains("TEMP B-TREE")),
            "recent history must not sort canonical bodies: {plan:?}"
        );
        assert!(
            plan.iter().any(|step| step.contains("SEARCH h")
                && step.contains("INDEX")
                && step.contains("session=?"))
                && plan.iter().any(|step| step.contains("SEARCH e")
                    && (step.contains("PRIMARY KEY") || step.contains("ROWID")))
                && !plan.iter().any(|step| ["SCAN h", "SCAN e", "SCAN t"]
                    .iter()
                    .any(|scan| step.contains(scan))),
            "recent history must use the conversation index: {plan:?}"
        );
    }
    let initial = history::pages::read(&fixture.database.connection, branch.id, None, 20)
        .await
        .unwrap();
    assert_eq!(initial.page.entries.len(), 100);
    assert_eq!(initial.missing, vec![first]);
    assert_eq!(initial.page.entries, before.page.entries);
    let remainder = history::pages::turn(
        &fixture.database.connection,
        branch.id,
        first,
        Some(initial.page.entries[0].sequence),
        100,
    )
    .await
    .unwrap();
    assert_eq!(remainder.entries.len(), 5);
    assert!(remainder.next_before.is_none());
    assert_eq!(remainder.run.origin, Some(original));
    let page = search::search(
        &fixture.database.connection,
        branch.id,
        &sailry_protocol::conversation::search::Query {
            text: "Inherited".into(),
            case_sensitive: false,
            before: Some(initial.page.entries[0].sequence),
            limit: 7,
        },
    )
    .await
    .unwrap();
    assert_eq!(page.matches.len(), 5);
    fixture.session = branch.id;
    let current = fixture.queued().await;
    let mut event = fixture::message(
        "assistant",
        canonical::Role::Assistant,
        vec![BlockKind::Text {
            text: "Must not replace inherited content".into(),
        }],
    );
    event.id = remainder.entries[0].id.clone();
    event.turn = current;
    assert!(
        history::append(
            &mut fixture.database,
            branch.id,
            current,
            event,
            &fixture.events
        )
        .await
        .is_err()
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
        105
    );
}
