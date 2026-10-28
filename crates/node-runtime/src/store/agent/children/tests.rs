use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

async fn admitted(fixture: &mut Fixture) -> (TurnId, Invocation) {
    let parent = fixture.queued().await;
    runs::start(&fixture.database.connection, parent)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    let request = crate::agent::canonical::ToolIdentity {
        name: crate::plugins::tools::alias("delegation", "spawn_agent"),
        id: Some("same-call".into()),
        arguments: serde_json::json!({"task":"Independent task"}),
    };
    let mut event = AdkEvent::new("parent-invocation");
    event.id = "same-event".into();
    event.author = "assistant".into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: request.id.clone(),
            name: request.name.clone(),
            args: request.arguments.clone(),
            thought_signature: None,
        }],
    });
    history::append(
        &mut fixture.database,
        fixture.session,
        parent,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    let child = delegation::admit(
        &mut fixture.database,
        parent,
        &request,
        request.arguments.clone(),
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(
        delegation::claim(&mut fixture.database, child.turn.id, &fixture.events)
            .await
            .unwrap(),
        Status::Running
    );
    runs::finish(
        &mut fixture.database,
        child.turn.id,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    let consumed = background::consume_child(
        &fixture.database.connection,
        parent,
        child.turn.session,
        child.turn.id,
    )
    .await
    .unwrap();
    assert_eq!(consumed.len(), 1);
    (parent, child)
}

#[tokio::test]
async fn scopes_repeated_event_ids() {
    let mut fixture = Fixture::new().await;
    crate::store::agent::tests::fixture::package(
        &fixture.database,
        fixture._root.path(),
        "delegation",
    )
    .await;
    let first = fixture.session;
    let (parent, child) = admitted(&mut fixture).await;
    runs::finish(
        &mut fixture.database,
        parent,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    let original = crate::store::commands::session(&fixture.database.connection, first)
        .await
        .unwrap();
    let Output::Session(other) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CreateSession {
            project: original.project,
            worktree: Some(original.worktree),
            config: Some(original.config),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    fixture.session = other.id;
    let (_, second) = admitted(&mut fixture).await;
    let children = fixture.read(None, 20).await.page.children;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].run.turn, second.turn.id);
    fixture.session = first;
    let children = fixture.read(None, 20).await.page.children;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].run.turn, child.turn.id);
}

#[tokio::test]
async fn bounds_summary_history() {
    let mut fixture = Fixture::new().await;
    crate::store::agent::tests::fixture::package(
        &fixture.database,
        fixture._root.path(),
        "delegation",
    )
    .await;
    let (parent, child) = admitted(&mut fixture).await;
    for _ in 0..105 {
        let mut event = AdkEvent::new("parent-invocation");
        event.author = "assistant".into();
        event.set_content(adk_core::Content::new("model").with_text("Later content"));
        history::append(
            &mut fixture.database,
            fixture.session,
            parent,
            event,
            &fixture.events,
        )
        .await
        .unwrap();
    }
    runs::finish(
        &mut fixture.database,
        parent,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    let recent = fixture.read(None, 1).await;
    assert_eq!(recent.missing, [parent]);
    assert!(recent.page.children.is_empty());
    let chunk = history::pages::turn(
        &fixture.database.connection,
        fixture.session,
        parent,
        Some(recent.page.entries[0].sequence),
        100,
    )
    .await
    .unwrap();
    assert_eq!(chunk.entries.len(), 6);
    assert_eq!(chunk.children.len(), 1);
    assert_eq!(chunk.children[0].run.turn, child.turn.id);
    assert_eq!(chunk.children[0].run.status, Status::Completed);
    assert_eq!(chunk.children[0].name, None);
    assert_eq!(chunk.next_before, None);
    let mut page = recent.page;
    page.entries = chunk.entries;
    page.children = chunk.children;
    page.entries.remove(0);
    history::pages::retain_metadata(&mut page);
    assert!(page.children.is_empty());
}
