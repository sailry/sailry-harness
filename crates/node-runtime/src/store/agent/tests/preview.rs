use super::{fixture::Fixture, *};

#[test]
fn snapshot_does_not_read_preview_bodies() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let mut fixture = Fixture::new();
    fixture.completed(&["Public board content"]);
    fixture
        .database
        .connection
        .authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::Read {
                table_name: "agent_events",
                ..
            } => Authorization::Deny,
            _ => Authorization::Allow,
        }))
        .unwrap();
    let node = fixture.database.node;
    let (output, _) = crate::store::commands::execute(
        &fixture.database.connection,
        node,
        node,
        None,
        &Request::new(node, Command::Snapshot),
    )
    .unwrap();
    assert!(matches!(output, Output::Snapshot(_)));
    assert!(activity::previews(&fixture.database.connection, &[fixture.session]).is_err());
}

#[test]
fn validates_batch_and_session() {
    let fixture = Fixture::new();
    let db = &fixture.database.connection;
    assert!(activity::previews(db, &[]).unwrap().is_empty());
    assert_eq!(
        activity::previews(db, &vec![fixture.session; 33])
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        activity::previews(db, &[SessionId::new()])
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

#[test]
fn follows_current_run_and_preserves_source() {
    let mut fixture = Fixture::new();
    assert_eq!(preview(&fixture), None);
    let text = "进度".repeat(400);
    fixture.completed(&["Earlier output", &text]);
    let summary = preview(&fixture);
    assert_eq!(summary, Some(text.chars().take(512).collect()));
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(preview(&fixture), summary);
    let next = fixture.queued();
    runs::start(&fixture.database.connection, next).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(preview(&fixture), None);
}

#[test]
fn excludes_private_thoughts_and_user_text() {
    let mut fixture = Fixture::new();
    let public = message("assistant", "Checking the current workspace", false);
    let thought = message("assistant", "Private reasoning", true);
    let user = message("user", "User message", false);
    fixture.append([public, thought, user]);
    assert_eq!(
        preview(&fixture).as_deref(),
        Some("Checking the current workspace")
    );
}

fn preview(fixture: &Fixture) -> Option<String> {
    activity::previews(&fixture.database.connection, &[fixture.session])
        .unwrap()
        .remove(0)
        .text
}

fn message(author: &str, text: &str, thought: bool) -> AdkEvent {
    let mut event = AdkEvent::new("preview-fixture");
    event.author = author.into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![if thought {
            adk_core::Part::Thinking {
                thinking: text.into(),
                signature: None,
            }
        } else {
            adk_core::Part::Text { text: text.into() }
        }],
    });
    event
}
