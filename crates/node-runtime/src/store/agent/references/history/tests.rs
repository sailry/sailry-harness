use super::*;
use crate::agent::canonical::{self, BlockKind};
use crate::store::agent::tests::{
    command,
    fixture::{self, Fixture},
};
use serde_json::json;

async fn copy_session(
    fixture: &Fixture,
    project: Option<ProjectId>,
    worktree: WorktreeId,
) -> Session {
    let transaction = fixture
        .database
        .connection
        .unchecked_transaction()
        .await
        .unwrap();
    let mut session = crate::store::commands::session(&transaction, fixture.session)
        .await
        .unwrap();
    session.id = SessionId::new();
    session.project = project;
    session.worktree = worktree;
    session.creation_order = crate::store::sessions::insert(&transaction, &session)
        .await
        .unwrap();
    crate::store::media::freeze(&transaction, session.id, Some(fixture.session))
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    session
}

async fn worktree(
    fixture: &Fixture,
    project: Option<ProjectId>,
    path: &std::path::Path,
) -> WorktreeId {
    let worktree = Worktree {
        id: WorktreeId::new(),
        project,
        path: path.to_str().unwrap().into(),
        main: false,
    };
    crate::store::worktrees::register(&fixture.database.connection, &worktree)
        .await
        .unwrap();
    worktree.id
}

async fn start(fixture: &mut Fixture) -> Source {
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    Source {
        caller: invocation.caller,
        session: invocation.turn.session,
        turn,
    }
}

fn read_query(session: SessionId) -> Query {
    Query::Read(Read {
        session,
        before: None,
        turn: None,
        revision: None,
        before_entry: None,
        limit: None,
    })
}

fn search_query(session: SessionId) -> Query {
    Query::Search(Search {
        session,
        text: "decision".into(),
        case_sensitive: false,
        before: None,
        revision: None,
        limit: None,
    })
}

#[tokio::test]
async fn shares_project_history_across_worktrees() {
    let mut fixture = Fixture::new().await;
    let original = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let sibling = worktree(&fixture, original.project, directory.path()).await;
    let target = copy_session(&fixture, original.project, sibling).await;
    fixture.session = target.id;
    fixture.completed(&["A shared decision 中文 🙂"]).await;
    fixture.session = original.id;
    let source = start(&mut fixture).await;
    let listed = execute(&fixture.database, source, Query::List(List::default()))
        .await
        .unwrap();
    assert_eq!(listed["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(listed["sessions"][0]["session"], target.id.to_string());
    for excluded in ["config", "profile", "roles", "caller"] {
        assert!(listed["sessions"][0].get(excluded).is_none());
    }
    assert!(
        execute(&fixture.database, source, read_query(target.id))
            .await
            .unwrap()
            .to_string()
            .contains("shared decision")
    );
    assert_eq!(
        execute(&fixture.database, source, search_query(target.id))
            .await
            .unwrap()["matches"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    super::super::validate(
        &fixture.database.connection,
        &original,
        &Input {
            text: "Continue the referenced task".into(),
            attachments: vec![],
            references: vec![reference::Reference {
                target: reference::Target::Session(target.id),
                label: "Earlier decision".into(),
            }],
        },
        None,
    )
    .await
    .unwrap();
    drop(directory);
    assert!(
        execute(&fixture.database, source, read_query(target.id))
            .await
            .is_ok()
    );
    assert_eq!(
        crate::store::commands::session(&fixture.database.connection, original.id)
            .await
            .unwrap()
            .worktree,
        original.worktree
    );
}

#[tokio::test]
async fn separates_unassigned_history_from_file_access() {
    let mut fixture = Fixture::new().await;
    let first_directory = tempfile::tempdir().unwrap();
    let second_directory = tempfile::tempdir().unwrap();
    let first_tree = worktree(&fixture, None, first_directory.path()).await;
    let second_tree = worktree(&fixture, None, second_directory.path()).await;
    let first = copy_session(&fixture, None, first_tree).await;
    let second = copy_session(&fixture, None, second_tree).await;
    fixture.session = second.id;
    fixture.completed(&["The prior decision is retained"]).await;
    fixture.session = first.id;
    let source = start(&mut fixture).await;
    assert!(
        execute(&fixture.database, source, read_query(second.id))
            .await
            .is_ok()
    );
    assert!(
        execute(&fixture.database, source, search_query(second.id))
            .await
            .is_ok()
    );
    let listed = execute(&fixture.database, source, Query::List(List::default()))
        .await
        .unwrap();
    assert_eq!(listed["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(listed["sessions"][0]["session"], second.id.to_string());
    let trees = crate::store::agent::worktrees::list(&fixture.database, source.turn)
        .await
        .unwrap();
    assert_eq!(
        trees.iter().map(|tree| tree.id).collect::<Vec<_>>(),
        [first_tree]
    );
    assert!(!trees.iter().any(|tree| tree.id == second_tree));
}

#[tokio::test]
async fn rechecks_target_and_invocation_ownership() {
    let mut fixture = Fixture::new().await;
    let original = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let target = copy_session(&fixture, original.project, original.worktree).await;
    let source = start(&mut fixture).await;
    assert!(
        execute(&fixture.database, source, read_query(target.id))
            .await
            .is_ok()
    );
    let spoofed = Source {
        caller: NodeId([9; 32]),
        ..source
    };
    assert_eq!(
        execute(&fixture.database, spoofed, read_query(target.id))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let spoofed = Source {
        session: target.id,
        ..source
    };
    assert_eq!(
        execute(&fixture.database, spoofed, read_query(target.id))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RemoveSession {
            session: target.id,
            expected_revision: target.revision,
        },
    )
    .await;
    for query in [
        read_query(target.id),
        search_query(target.id),
        read_query(SessionId::new()),
    ] {
        assert_eq!(
            execute(&fixture.database, source, query)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
    assert!(
        execute(&fixture.database, source, Query::List(List::default()))
            .await
            .unwrap()["sessions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    runs::stop(&fixture.database.connection, source.turn)
        .await
        .unwrap();
    assert_eq!(
        execute(&fixture.database, source, Query::List(List::default()))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn rejects_foreign_projects_and_changed_history() {
    let mut fixture = Fixture::new().await;
    let original = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let target = copy_session(&fixture, original.project, original.worktree).await;
    let directory = tempfile::tempdir().unwrap();
    let Output::Project(foreign) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RegisterProject {
            name: "Other project".into(),
            path: directory.path().to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected")
    };
    let foreign_tree = crate::store::worktrees::list(&fixture.database.connection)
        .await
        .unwrap()
        .into_iter()
        .find(|worktree| worktree.project == Some(foreign.id) && worktree.main)
        .unwrap()
        .id;
    let foreign_session = copy_session(&fixture, Some(foreign.id), foreign_tree).await;
    let source = start(&mut fixture).await;
    for query in [
        read_query(foreign_session.id),
        search_query(foreign_session.id),
    ] {
        assert_eq!(
            execute(&fixture.database, source, query)
                .await
                .unwrap_err()
                .code,
            ErrorCode::WrongTarget
        );
    }
    assert_eq!(
        visible(
            &fixture.database.connection,
            original.project,
            foreign_session.id
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::WrongTarget
    );
    let page = execute(&fixture.database, source, read_query(target.id))
        .await
        .unwrap();
    let revision = page["page"]["revision"].as_u64().unwrap();
    fixture
        .database
        .connection
        .execute(
            "UPDATE sessions SET history_revision=history_revision+1 WHERE id=?1",
            [target.id.to_string()],
        )
        .await
        .unwrap();
    let mut read: Read = serde_json::from_value(json!({"session":target.id})).unwrap();
    read.revision = Some(revision);
    assert_eq!(
        execute(&fixture.database, source, Query::Read(read))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let mut search: Search =
        serde_json::from_value(json!({"session":target.id,"text":"decision"})).unwrap();
    search.revision = Some(revision);
    assert_eq!(
        execute(&fixture.database, source, Query::Search(search))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}

#[tokio::test]
async fn bounds_directory_pages_and_preserves_unknown_time() {
    let mut fixture = Fixture::new().await;
    let session = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    for _ in 0..260 {
        copy_session(&fixture, session.project, session.worktree).await;
    }
    let source = start(&mut fixture).await;
    let page = execute(
        &fixture.database,
        source,
        Query::List(List {
            limit: Some(1),
            ..List::default()
        }),
    )
    .await
    .unwrap();
    assert_eq!(page["sessions"].as_array().unwrap().len(), 1);
    assert!(page["sessions"][0]["updated_ms"].is_null());
    let next = page["next_before"].as_u64().unwrap();
    let following = execute(
        &fixture.database,
        source,
        Query::List(List {
            limit: Some(1),
            before: Some(next),
            ..List::default()
        }),
    )
    .await
    .unwrap();
    assert_ne!(
        page["sessions"][0]["session"],
        following["sessions"][0]["session"]
    );
    let mut event = fixture::message(
        "assistant",
        canonical::Role::Assistant,
        vec![BlockKind::Text {
            text: "Observed directory results".into(),
        }],
    );
    event.turn = source.turn;
    crate::store::agent::history::append(
        &mut fixture.database,
        source.session,
        source.turn,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    assert!(
        execute(
            &fixture.database,
            source,
            Query::List(List {
                before: Some(next),
                revision: Some(page["revision"].as_str().unwrap().into()),
                limit: Some(1),
                ..List::default()
            })
        )
        .await
        .is_ok()
    );
    let filtered = execute(
        &fixture.database,
        source,
        Query::List(List {
            since_ms: Some(0),
            ..List::default()
        }),
    )
    .await
    .unwrap();
    assert!(filtered["sessions"].as_array().unwrap().is_empty());
    assert!(filtered["next_before"].is_number());
    let exhausted = execute(
        &fixture.database,
        source,
        Query::List(List {
            since_ms: Some(0),
            before: filtered["next_before"].as_u64(),
            ..List::default()
        }),
    )
    .await
    .unwrap();
    assert!(exhausted["next_before"].is_null());
    assert!(exhausted["sessions"].as_array().unwrap().is_empty());
    copy_session(&fixture, session.project, session.worktree).await;
    assert_eq!(
        execute(
            &fixture.database,
            source,
            Query::List(List {
                revision: Some(page["revision"].as_str().unwrap().into()),
                ..List::default()
            })
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
}

#[tokio::test]
async fn bounds_public_content_without_claiming_completeness() {
    let mut fixture = Fixture::new().await;
    let text = "Unicode decision 中文 🙂 ".repeat(5000);
    let turn = fixture.completed(&[text.as_str(); 40]).await;
    let source = start(&mut fixture).await;
    let page = execute(&fixture.database, source, read_query(source.session))
        .await
        .unwrap();
    assert!(serde_json::to_vec(&page).unwrap().len() <= 128 * 1024);
    assert_eq!(page["truncated"], true);
    assert!(!page["truncated_entries"].as_array().unwrap().is_empty());
    assert!(page["missing"].as_array().unwrap().contains(&json!(turn)));
    let first = execute(
        &fixture.database,
        source,
        Query::Read(
            serde_json::from_value(json!({
                "session":source.session,"turn":turn,"revision":page["page"]["revision"],"limit":1
            }))
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(first["entries"].as_array().unwrap().len(), 1);
    assert!(first["next_before"].is_number());
    assert_eq!(first["truncated"], true);
    let older = execute(
        &fixture.database,
        source,
        Query::Read(
            serde_json::from_value(json!({
                "session":source.session,"turn":turn,"revision":first["revision"],
                "before_entry":first["next_before"],"limit":1
            }))
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    assert!(
        older["entries"][0]["sequence"].as_u64().unwrap()
            < first["entries"][0]["sequence"].as_u64().unwrap()
    );
    let found = execute(&fixture.database, source, search_query(source.session))
        .await
        .unwrap();
    for matched in found["matches"].as_array().unwrap() {
        assert!(
            matched["snippet"].as_str().unwrap().len() <= conversation::search::MAX_SNIPPET_BYTES
        );
    }
}

#[tokio::test]
async fn omits_private_event_state() {
    let mut fixture = Fixture::new().await;
    let mut event = fixture::message(
        "assistant",
        canonical::Role::Assistant,
        vec![BlockKind::Text {
            text: "A public decision".into(),
        }],
    );
    event.metadata.state = Some(canonical::StateChanges {
        session: [("private".into(), json!("private-state-marker"))].into(),
        ..Default::default()
    });
    fixture.append([event]).await;
    let source = start(&mut fixture).await;
    let read = execute(&fixture.database, source, read_query(source.session))
        .await
        .unwrap();
    assert!(read.to_string().contains("public decision"));
    assert!(!read.to_string().contains("private-state-marker"));
}
