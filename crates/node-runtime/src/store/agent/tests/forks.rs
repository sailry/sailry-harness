use super::{fixture::Fixture, *};
use serde_json::json;
use std::collections::HashMap;

fn fork(fixture: &mut Fixture, through: TurnId, revision: u64) -> Session {
    let Output::Session(session) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through,
            expected_revision: revision,
        },
    ) else {
        panic!("forked session expected")
    };
    session
}

#[test]
fn preserves_frozen_roles() {
    let mut fixture = Fixture::new();
    let through = fixture.completed(&["Completed before configuring roles"]);
    let profile = role::Profile {
        appearance: None,
        id: RoleId::new(),
        revision: 0,
        key: "review".into(),
        name: "Review".into(),
        description: "Review changes".into(),
        model: None,
        max_turns: Some(8),
        skills: vec![],
        instructions: "Frozen instructions 中文 🙂".into(),
    };
    let Output::Role(mut role) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutRole {
            role: profile,
            expected_revision: 0,
        },
    ) else {
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
    ) else {
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
    );
    let fork = fork(&mut fixture, through, 2);
    assert_eq!(fork.roles, session.roles);
    assert_eq!(
        fork.roles.profiles[0].instructions,
        "Frozen instructions 中文 🙂"
    );
    assert_eq!(
        crate::store::sessions::roles::read(&fixture.database.connection, session.id, 1).unwrap(),
        role::Snapshot::default()
    );
    assert_eq!(
        crate::store::sessions::roles::read(&fixture.database.connection, fork.id, 1).unwrap(),
        session.roles
    );
}

#[test]
fn rebuilds_shared_history() {
    let mut fixture = Fixture::new();
    let original = fixture.session;
    let mut turns = Vec::new();
    for index in 1..=3 {
        let mut event = AdkEvent::new(format!("original-{index}"));
        event.author = "assistant".into();
        event.set_content(
            adk_core::Content::new("model").with_text(format!("Message {index} 中文 🙂")),
        );
        event.actions.state_delta.extend([
            ("value".into(), json!(index)),
            ("app:shared".into(), json!(index)),
            ("user:shared".into(), json!(index)),
            ("temp:ignored".into(), json!(index)),
        ]);
        turns.push(fixture.append([event]));
    }
    let before = fixture.read(None, 100).page;
    let mut previous =
        crate::store::commands::session(&fixture.database.connection, original).unwrap();
    previous.config.effort = Effort::Max;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: original,
            expected_revision: 1,
            config: previous.config.clone(),
        },
    );
    let branched = fork(&mut fixture, turns[1], 2);
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
        crate::store::commands::session(&fixture.database.connection, branched.id).unwrap(),
        branched
    );
    let page = history::pages::read(&fixture.database.connection, branched.id, None, 100)
        .unwrap()
        .page;
    assert_eq!(page.entries, before.entries[..2]);
    assert_eq!(page.runs.len(), 2);
    assert!(
        page.runs
            .iter()
            .all(|run| run.session == branched.id && run.origin == Some(original))
    );
    assert_eq!(fixture.read(None, 100).page, before);
    assert_eq!(
        fixture
            .database
            .connection
            .query_row("SELECT count(*) FROM agent_events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
    let expected = HashMap::from([
        ("value".to_owned(), json!(2)),
        ("app:shared".to_owned(), json!(3)),
        ("user:shared".to_owned(), json!(3)),
    ]);
    assert_eq!(
        history::state(&fixture.database.connection, branched.id).unwrap(),
        expected
    );
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(
        history::state(&fixture.database.connection, branched.id).unwrap(),
        expected
    );
    fixture.session = branched.id;
    let nested = fork(&mut fixture, turns[0], 1);
    let inherited = history::pages::read(&fixture.database.connection, nested.id, None, 100)
        .unwrap()
        .page;
    assert_eq!(inherited.entries, before.entries[..1]);
    assert_eq!(inherited.runs[0].origin, Some(original));
    let invocation = fixture.queued();
    let request = GetRequest {
        app_name: APP.into(),
        user_id: USER.into(),
        session_id: branched.id.to_string(),
        num_recent_events: None,
        after: None,
    };
    let native = sessions::get(&fixture.database.connection, invocation, request).unwrap();
    assert_eq!(native.events().len(), 2);
    assert_eq!(native.state().all(), expected);
    let other = fixture.completed(&["Independent branch content"]);
    assert!(
        history::pages::read(&fixture.database.connection, branched.id, None, 100)
            .unwrap()
            .page
            .runs
            .iter()
            .any(|run| run.turn == other && run.origin.is_none())
    );
    assert_eq!(
        history::pages::read(&fixture.database.connection, original, None, 100)
            .unwrap()
            .page,
        before
    );
    assert_eq!(
        history::pages::read(&fixture.database.connection, nested.id, None, 100)
            .unwrap()
            .page,
        inherited
    );
}

#[test]
fn validates_before_creation() {
    let mut fixture = Fixture::new();
    let first = fixture.completed(&["First"]);
    let queued = fixture.queued();
    let attempt = |fixture: &Fixture, source, through, revision| {
        crate::store::sessions::fork(&fixture.database.connection, source, through, revision)
            .unwrap_err()
            .code
    };
    assert_eq!(
        attempt(&fixture, fixture.session, first, 2),
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        attempt(&fixture, fixture.session, queued, 1),
        ErrorCode::Busy
    );
    assert_eq!(
        attempt(&fixture, fixture.session, TurnId::new(), 1),
        ErrorCode::NotFound
    );
    let branch = fork(&mut fixture, first, 1);
    assert_eq!(
        attempt(&fixture, branch.id, queued, 1),
        ErrorCode::WrongTarget
    );
    runs::start(&fixture.database.connection, queued).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(
        attempt(&fixture, fixture.session, queued, 1),
        ErrorCode::Busy
    );
    // A closed prefix can be forked while a later source turn is active.
    let other = fork(&mut fixture, first, 1);
    assert_eq!(
        history::pages::read(&fixture.database.connection, other.id, None, 100)
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
            .query_row("SELECT count(*) FROM sessions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn paginates_inherited_turns() {
    let mut fixture = Fixture::new();
    let original = fixture.session;
    let first = fixture.completed(&["Inherited 中文 🙂"; 105]);
    let before = fixture.read(None, 100);
    let branch = fork(&mut fixture, first, 1);
    fixture
        .database
        .connection
        .execute("DELETE FROM conversation_events", [])
        .unwrap();
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(
        fixture
            .database
            .connection
            .query_row("SELECT count(*) FROM conversation_events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        210
    );
    for sql in [history::EVENTS_QUERY, sessions::HISTORY_QUERY] {
        let mut query = fixture.database.connection.prepare(sql).unwrap();
        let target = branch.id.to_string();
        let values: Vec<&dyn rusqlite::ToSql> = if query.parameter_count() == 3 {
            vec![&target, &i64::MAX, &20]
        } else {
            vec![&target, &20]
        };
        let mut rows = query.query(values.as_slice()).unwrap();
        let mut count = 0;
        while rows.next().unwrap().is_some() {
            count += 1;
        }
        drop(rows);
        assert_eq!(count, 20);
        assert_eq!(
            query.get_status(rusqlite::StatementStatus::Sort),
            0,
            "recent history must not sort canonical bodies"
        );
        assert_eq!(
            query.get_status(rusqlite::StatementStatus::FullscanStep),
            0,
            "recent history must use the conversation index"
        );
    }
    let initial = history::pages::read(&fixture.database.connection, branch.id, None, 20).unwrap();
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
    .unwrap();
    assert_eq!(page.matches.len(), 5);
    fixture.session = branch.id;
    let current = fixture.queued();
    let mut event = AdkEvent::new("duplicate-fixture");
    event.id = remainder.entries[0].id.clone();
    event.set_content(
        adk_core::Content::new("model").with_text("Must not replace inherited content"),
    );
    assert!(
        history::append(
            &mut fixture.database,
            branch.id,
            current,
            event,
            &fixture.events
        )
        .is_err()
    );
    assert_eq!(
        fixture
            .database
            .connection
            .query_row("SELECT count(*) FROM agent_events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        105
    );
}
