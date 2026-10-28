use super::*;
use crate::agent::canonical::{self, BlockKind};
use crate::store::agent::tests::{
    command,
    fixture::{self, Fixture},
};
use serde_json::json;

fn options(text: &str) -> Query {
    Query {
        text: text.into(),
        case_sensitive: false,
        before: None,
        limit: 100,
    }
}

async fn read(fixture: &Fixture, query: &Query) -> search::Page {
    search(&fixture.database.connection, fixture.session, query)
        .await
        .unwrap()
}

#[tokio::test]
async fn matches_literal_unicode() {
    let mut fixture = Fixture::new().await;
    let first = fixture
        .completed(&["before ÄBC 中文 🙂 after", "literal .* and [text]"])
        .await;
    let second = fixture.completed(&["More äbc text"]).await;
    let found = read(&fixture, &options("äbc")).await;
    assert_eq!(
        found
            .matches
            .iter()
            .map(|found| found.turn)
            .collect::<Vec<_>>(),
        [second, first]
    );
    assert_eq!(
        &found.matches[1].snippet[found.matches[1].highlight.clone()],
        "ÄBC"
    );
    let full = fixture.read(None, 100).await.page;
    for found in &found.matches {
        let entry = full
            .entries
            .iter()
            .find(|entry| entry.id == found.entry)
            .unwrap();
        assert_eq!(entry.sequence, found.sequence);
        assert_eq!(entry.turn, found.turn);
        assert_eq!(entry.author, found.author);
        assert_eq!(
            full.runs
                .iter()
                .find(|run| run.turn == found.turn)
                .unwrap()
                .sequence,
            found.turn_sequence
        );
        assert!(
            matches!(&entry.parts[found.part], Part::Text(text) if text.contains(&found.snippet))
        );
    }
    let exact = read(
        &fixture,
        &Query {
            case_sensitive: true,
            ..options("äbc")
        },
    )
    .await;
    assert_eq!(exact.matches.len(), 1);
    assert_eq!(exact.matches[0].turn, second);
    assert_eq!(read(&fixture, &options(".*")).await.matches.len(), 1);
    assert_eq!(read(&fixture, &options("中文 🙂")).await.matches.len(), 1);
    assert!(read(&fixture, &options("missing")).await.matches.is_empty());

    let mut event = fixture::message(
        "assistant",
        canonical::Role::Assistant,
        vec![
            BlockKind::ToolRequest(fixture::request(
                Some("search-call"),
                "hidden-marker",
                json!({"text":"hidden-marker"}),
            )),
            BlockKind::Text {
                text: "first visible marker".into(),
            },
            BlockKind::Text {
                text: "second visible marker".into(),
            },
        ],
    );
    event.metadata.state = Some(canonical::StateChanges {
        session: [("value".into(), json!("hidden-marker"))].into(),
        ..Default::default()
    });
    fixture.append([event]).await;
    assert!(
        read(&fixture, &options("hidden-marker"))
            .await
            .matches
            .is_empty()
    );
    let visible = read(&fixture, &options("visible marker")).await;
    assert_eq!(visible.matches.len(), 1);
    assert_eq!(visible.matches[0].part, 1);
}

#[tokio::test]
async fn bounds_sparse_scans() {
    let mut fixture = Fixture::new().await;
    let first = fixture.completed(&["old needle 中文 🙂"]).await;
    fixture
        .completed(&vec!["unrelated entry"; MAX_EVENTS + 10])
        .await;
    let page = read(&fixture, &options("needle")).await;
    assert!(page.matches.is_empty());
    let next = read(
        &fixture,
        &Query {
            before: page.next_before,
            ..options("needle")
        },
    )
    .await;
    assert_eq!(next.matches.len(), 1);
    assert_eq!(next.matches[0].turn, first);
    assert_eq!(next.next_before, None);
    let large = format!(
        "{}needle 中文 🙂{}",
        "首".repeat(250_000),
        "尾".repeat(250_000)
    );
    fixture.completed(&[large.as_str(); 5]).await;
    let bounded = read(&fixture, &options("needle 中文 🙂")).await;
    assert_eq!(bounded.matches.len(), 3);
    assert!(bounded.next_before.is_some());
    for found in bounded.matches {
        assert!(found.snippet.len() <= search::MAX_SNIPPET_BYTES);
        assert_eq!(&found.snippet[found.highlight], "needle 中文 🙂");
    }
    let single = read(
        &fixture,
        &Query {
            limit: 1,
            ..options("needle")
        },
    )
    .await;
    let later = read(
        &fixture,
        &Query {
            limit: 1,
            before: single.next_before,
            ..options("needle")
        },
    )
    .await;
    assert!(single.matches[0].sequence > later.matches[0].sequence);
    assert_eq!(single.next_before, Some(single.matches[0].sequence));
}

#[tokio::test]
async fn validates_scope_without_persistence() {
    let mut fixture = Fixture::new().await;
    fixture.completed(&["search fixture"]).await;
    let before = fixture.read(None, 1).await.page.entries[0].sequence;
    let count = fixture
        .database
        .connection
        .prepare("SELECT COUNT(*) FROM requests")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get::<i64>(0)
        .unwrap();
    let cursor = fixture
        .database
        .connection
        .prepare("SELECT MAX(cursor) FROM events")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get::<i64>(0)
        .unwrap();
    let result = command(
        &mut fixture.database,
        &fixture.events,
        Command::SearchConversation {
            session: fixture.session,
            query: options("fixture"),
        },
    )
    .await;
    assert!(matches!(result, Output::ConversationMatches(_)));
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT COUNT(*) FROM requests")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        count
    );
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT MAX(cursor) FROM events")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        cursor
    );
    for invalid_query in [
        options("   "),
        options(&"x".repeat(search::MAX_QUERY_BYTES + 1)),
        Query {
            limit: 0,
            ..options("fixture")
        },
        Query {
            limit: 101,
            ..options("fixture")
        },
        Query {
            before: Some(0),
            ..options("fixture")
        },
        Query {
            before: Some(u64::MAX),
            ..options("fixture")
        },
    ] {
        assert_eq!(
            search(
                &fixture.database.connection,
                fixture.session,
                &invalid_query
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
    }
    assert_eq!(
        search(
            &fixture.database.connection,
            SessionId::new(),
            &options("fixture")
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::NotFound
    );
    let Output::Snapshot(snapshot) =
        command(&mut fixture.database, &fixture.events, Command::Snapshot).await
    else {
        panic!("snapshot expected")
    };
    let Output::Session(other) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CreateSession {
            project: Some(snapshot.projects[0].id),
            worktree: None,
            config: Some(snapshot.sessions[0].config.clone()),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    assert_eq!(
        search(
            &fixture.database.connection,
            other.id,
            &Query {
                before: Some(before),
                ..options("fixture")
            }
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::WrongTarget
    );
}
