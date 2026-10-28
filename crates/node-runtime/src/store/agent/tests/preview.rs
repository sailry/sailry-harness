use super::{fixture::Fixture, *};
use crate::agent::canonical::{self, BlockKind};

#[tokio::test]
async fn snapshot_does_not_read_preview_bodies() {
    let mut fixture = Fixture::new().await;
    fixture.completed(&["Public board content"]).await;
    fixture
        .database
        .connection
        .execute_batch("BEGIN; ALTER TABLE agent_events RENAME TO hidden_events; COMMIT;")
        .await
        .unwrap();
    let node = fixture.database.node;
    let (output, _) = crate::store::commands::execute(
        &fixture.database.connection,
        node,
        node,
        None,
        &Request::new(node, Command::Snapshot),
    )
    .await
    .unwrap();
    assert!(matches!(output, Output::Snapshot(_)));
    assert!(
        activity::previews(&fixture.database.connection, &[fixture.session])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn validates_batch_and_session() {
    let fixture = Fixture::new().await;
    let db = &fixture.database.connection;
    assert!(activity::previews(db, &[]).await.unwrap().is_empty());
    assert_eq!(
        activity::previews(db, &vec![fixture.session; 33])
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        activity::previews(db, &[SessionId::new()])
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert!(
        !Command::ReadActivity {
            sessions: vec![fixture.session]
        }
        .durable()
    );
}

#[tokio::test]
async fn follows_current_run_and_preserves_source() {
    let mut fixture = Fixture::new().await;
    assert_eq!(preview(&fixture).await, None);
    let text = "进度".repeat(400);
    fixture.completed(&["Earlier output", &text]).await;
    let summary = preview(&fixture).await;
    assert_eq!(summary, Some(text.chars().take(512).collect()));
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(preview(&fixture).await, summary);
    let next = fixture.queued().await;
    runs::start(&fixture.database.connection, next)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(preview(&fixture).await, None);
}

#[tokio::test]
async fn excludes_private_thoughts_and_user_text() {
    let mut fixture = Fixture::new().await;
    let public = message("assistant", "Checking the current workspace", false);
    let thought = message("assistant", "Private reasoning", true);
    let user = message("user", "User message", false);
    fixture.append([public, thought, user]).await;
    assert_eq!(
        preview(&fixture).await.as_deref(),
        Some("Checking the current workspace")
    );
}

async fn preview(fixture: &Fixture) -> Option<String> {
    activity::previews(&fixture.database.connection, &[fixture.session])
        .await
        .unwrap()
        .remove(0)
        .text
}

fn message(author: &str, text: &str, thought: bool) -> canonical::Record {
    fixture::message(
        author,
        if author == "user" {
            canonical::Role::User
        } else {
            canonical::Role::Assistant
        },
        vec![if thought {
            BlockKind::Thinking {
                text: text.into(),
                signature: None,
            }
        } else {
            BlockKind::Text { text: text.into() }
        }],
    )
}
