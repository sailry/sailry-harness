use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

fn admitted(fixture: &mut Fixture) -> (TurnId, Invocation) {
    let parent = fixture.queued();
    runs::start(&fixture.database.connection, parent).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    let request = adk_core::ToolConfirmationRequest {
        tool_name: crate::plugins::tools::alias("delegation", "spawn_agent"),
        function_call_id: Some("same-call".into()),
        args: serde_json::json!({"task":"Independent task"}),
    };
    let mut event = AdkEvent::new("parent-invocation");
    event.id = "same-event".into();
    event.author = "assistant".into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: request.function_call_id.clone(),
            name: request.tool_name.clone(),
            args: request.args.clone(),
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
    .unwrap();
    let child = delegation::admit(
        &mut fixture.database,
        parent,
        &request,
        request.args.clone(),
        None,
        &fixture.events,
    )
    .unwrap();
    runs::finish(
        &mut fixture.database,
        child.turn.id,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    (parent, child)
}

#[test]
fn scopes_repeated_event_ids() {
    let mut fixture = Fixture::new();
    crate::store::agent::tests::fixture::package(
        &fixture.database,
        fixture._root.path(),
        "delegation",
    );
    let first = fixture.session;
    let (parent, child) = admitted(&mut fixture);
    runs::finish(
        &mut fixture.database,
        parent,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    let original = crate::store::commands::session(&fixture.database.connection, first).unwrap();
    let Output::Session(other) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CreateSession {
            project: original.project,
            worktree: Some(original.worktree),
            config: Some(original.config),
        },
    ) else {
        panic!("session expected")
    };
    fixture.session = other.id;
    let (_, second) = admitted(&mut fixture);
    let children = fixture.read(None, 20).page.children;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].run.turn, second.turn.id);
    fixture.session = first;
    let children = fixture.read(None, 20).page.children;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].run.turn, child.turn.id);
}

#[test]
fn bounds_summary_history() {
    let mut fixture = Fixture::new();
    crate::store::agent::tests::fixture::package(
        &fixture.database,
        fixture._root.path(),
        "delegation",
    );
    let (parent, child) = admitted(&mut fixture);
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
        .unwrap();
    }
    runs::finish(
        &mut fixture.database,
        parent,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    let recent = fixture.read(None, 1);
    assert_eq!(recent.missing, [parent]);
    assert!(recent.page.children.is_empty());
    let chunk = history::pages::turn(
        &fixture.database.connection,
        fixture.session,
        parent,
        Some(recent.page.entries[0].sequence),
        100,
    )
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
