use super::*;
use adk_core::{Content, FunctionResponseData, Part as AdkPart};

fn user(text: &str) -> AdkEvent {
    let mut event = AdkEvent::new("context-fixture");
    event.author = "user".into();
    event.set_content(Content::new("user").with_text(text));
    event
}

fn tool(text: &str) -> AdkEvent {
    let mut event = AdkEvent::new("context-fixture");
    event.author = "assistant".into();
    event.set_content(Content {
        role: "user".into(),
        parts: vec![AdkPart::FunctionResponse {
            id: Some("read".into()),
            function_response: FunctionResponseData::new(
                "read_file",
                serde_json::json!({"content": text}),
            ),
            annotations: None,
        }],
    });
    event
}

fn assert_context(fixture: &Fixture, session: SessionId, tokens: u64, pending: &[&AdkEvent]) {
    let usage = context_usage(&fixture.database.connection, session)
        .unwrap()
        .unwrap();
    assert_eq!(usage.tokens, tokens);
    assert_eq!(
        usage
            .pending
            .iter()
            .map(|event| &event.id)
            .collect::<Vec<_>>(),
        pending.iter().map(|event| &event.id).collect::<Vec<_>>()
    );
    for (actual, expected) in usage.pending.iter().zip(pending) {
        let actual = actual.content().unwrap();
        let expected = expected.content().unwrap();
        assert_eq!(actual.role, expected.role);
        assert_eq!(actual.parts, expected.parts);
    }
    assert_eq!(
        read(&fixture.database.connection, session)
            .unwrap()
            .context_tokens,
        Some(tokens)
    );
}

#[test]
fn counts_new_content() {
    let mut fixture = Fixture::new();
    let anchor = event(100, 20);
    fixture.append([user("Already observed"), anchor.clone()]);
    assert_context(&fixture, fixture.session, 120, &[]);

    let mut result = tool("New tool result 中文 🙂");
    result.timestamp = anchor.timestamp - chrono::Duration::seconds(60);
    let followup = user("Use the new evidence");
    let mut media = event(500, 40);
    media
        .provider_metadata
        .insert("sailry_media".into(), "image".into());
    fixture.append([result.clone(), media, followup.clone()]);
    assert_context(&fixture, fixture.session, 120, &[&result, &followup]);
    let statistics = read(&fixture.database.connection, fixture.session).unwrap();
    assert_eq!(statistics.responses, 2);
    assert_eq!(statistics.usage.as_ref().unwrap().input, 600);
    assert_eq!(statistics.usage.as_ref().unwrap().output, 60);

    let latest = user("After the next observation");
    fixture.append([event(200, 30), latest.clone()]);
    assert_context(&fixture, fixture.session, 230, &[&latest]);
}

#[test]
fn follows_fork_and_rewind_membership() {
    let mut fixture = Fixture::new();
    let source = fixture.session;
    let inherited = user("Inherited pending input");
    let first = fixture.append([event(100, 20), inherited.clone()]);
    let source_result = tool("Source-only evidence");
    let head = fixture.append([event(200, 30), source_result.clone()]);

    let Output::Session(fork) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: source,
            through: first,
            expected_revision: 1,
        },
    ) else {
        panic!("fork expected");
    };
    fixture.session = fork.id;
    let fork_input = user("Fork-only input");
    fixture.append([fork_input.clone()]);
    assert_context(&fixture, fork.id, 120, &[&inherited, &fork_input]);
    assert_context(&fixture, source, 230, &[&source_result]);

    let Output::Rewound(rewind) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: source,
            through: Some(first),
            expected_head: head,
            expected_revision: 1,
        },
    ) else {
        panic!("rewind expected");
    };
    assert_context(&fixture, source, 120, &[&inherited]);
    assert_context(&fixture, rewind.backup.id, 230, &[&source_result]);
    assert_context(&fixture, fork.id, 120, &[&inherited, &fork_input]);

    super::super::super::history::rebuild(&mut fixture.database.connection).unwrap();
    assert_context(&fixture, source, 120, &[&inherited]);
    assert_context(&fixture, rewind.backup.id, 230, &[&source_result]);
    assert_context(&fixture, fork.id, 120, &[&inherited, &fork_input]);
}
