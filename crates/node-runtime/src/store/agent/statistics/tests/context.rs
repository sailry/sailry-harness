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
fn excludes_cached_subsets_from_context() {
    let mut fixture = Fixture::new();
    let mut response = event(200_000, 12_000);
    let usage = response.llm_response.usage_metadata.as_mut().unwrap();
    usage.cache_read_input_token_count = Some(190_000);
    usage.thinking_token_count = Some(10_000);
    fixture.append([event(300_000, 20_000), response]);
    assert_context(&fixture, fixture.session, 212_000, &[]);
    let statistics = read(&fixture.database.connection, fixture.session).unwrap();
    let usage = statistics.usage.unwrap();
    assert_eq!(usage.input, 500_000);
    assert_eq!(usage.output, 32_000);
    assert_eq!(usage.cached_input, 190_002);
    assert_eq!(usage.reasoning, 10_001);
}

#[test]
fn follows_fork_and_rewind_membership() {
    let mut fixture = Fixture::new();
    let source = fixture.session;
    let inherited = user("Inherited pending input");
    let first = fixture.append([timed(100, 20, 125_000), inherited.clone()]);
    let source_result = tool("Source-only evidence");
    let head = fixture.append([timed(200, 30, 250_000), source_result.clone()]);

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
    assert_eq!(
        read(&fixture.database.connection, fork.id)
            .unwrap()
            .first_token_us,
        Some(125_000)
    );
    assert_context(&fixture, source, 230, &[&source_result]);
    assert_eq!(
        read(&fixture.database.connection, source)
            .unwrap()
            .first_token_us,
        Some(250_000)
    );

    let retained = fixture.fork(source, head);
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
    assert_eq!(
        read(&fixture.database.connection, source)
            .unwrap()
            .first_token_us,
        Some(125_000)
    );
    assert!(rewind.backup.is_none());
    assert_context(&fixture, retained.id, 230, &[&source_result]);
    assert_context(&fixture, fork.id, 120, &[&inherited, &fork_input]);

    super::super::super::history::rebuild(&mut fixture.database.connection).unwrap();
    assert_context(&fixture, source, 120, &[&inherited]);
    assert_context(&fixture, retained.id, 230, &[&source_result]);
    assert_context(&fixture, fork.id, 120, &[&inherited, &fork_input]);
}

#[test]
fn freezes_observed_model_limits() {
    let mut fixture = Fixture::new();
    let config = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .unwrap()
        .config;
    let provider: sailry_protocol::conversation::Provider = serde_json::from_value(serde_json::json!({
        "id":config.provider,"revision":0,"name":"Context fixture","api":"chat_completions","authentication":"api_key",
        "endpoint":"http://127.0.0.1:1","enabled":true,"default_model":"fixture","credential":null,
        "models":[{"id":"fixture","context":1_000,"output":64,"reasoning":true,"efforts":["low"],"default_effort":"low"}]
    })).unwrap();
    let Output::Provider(mut provider) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutProvider {
            provider,
            expected_revision: 0,
        },
    ) else {
        panic!("provider expected")
    };
    fixture.append([event(100, 20)]);
    let observed = read(&fixture.database.connection, fixture.session).unwrap();
    assert_eq!(
        (observed.context_tokens, observed.context_limit),
        (Some(120), Some(1_000))
    );
    provider.models[0].context = 2_000;
    let expected_revision = provider.revision;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::PutProvider {
            provider,
            expected_revision,
        },
    );
    let retained = read(&fixture.database.connection, fixture.session).unwrap();
    assert_eq!(
        (retained.context_tokens, retained.context_limit),
        (Some(120), Some(1_000))
    );
    let turn = fixture.append([event(200, 30)]);
    let latest = read(&fixture.database.connection, fixture.session).unwrap();
    assert_eq!(
        (latest.context_tokens, latest.context_limit),
        (Some(230), Some(2_000))
    );
    fixture
        .database
        .connection
        .execute(
            "UPDATE agent_runs SET provider=NULL WHERE turn=?1",
            [turn.to_string()],
        )
        .unwrap();
    let historical = read(&fixture.database.connection, fixture.session).unwrap();
    assert_eq!(
        (historical.context_tokens, historical.context_limit),
        (Some(230), None)
    );
}

#[test]
fn estimates_only_after_summary_reset() {
    let mut fixture = Fixture::new();
    let anchor = event(14_000, 10);
    fixture.append([user("Preserved requirement"), anchor.clone()]);
    let mut summary = AdkEvent::new("summary-fixture");
    summary.author = "system".into();
    summary
        .provider_metadata
        .insert(super::super::super::compaction::END_EVENT.into(), anchor.id);
    summary.actions.compaction = Some(adk_core::EventCompaction {
        start_timestamp: anchor.timestamp,
        end_timestamp: anchor.timestamp,
        compacted_content: Content::new("model").with_text("Verified handoff"),
    });
    fixture.append([summary]);
    let growth = tool(&"Unobserved pending evidence ".repeat(3000));
    fixture.append([growth.clone()]);
    let usage = context_usage(&fixture.database.connection, fixture.session)
        .unwrap()
        .unwrap();
    assert!(!usage.observed);
    assert_eq!(usage.tokens, 0);
    assert!(
        usage.pending[0].content().unwrap().parts[0]
            .text()
            .unwrap()
            .contains("Verified handoff")
    );
    assert_eq!(usage.pending.last().unwrap().id, growth.id);
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .unwrap()
            .context_tokens,
        None
    );
    let mut unknown = event(10, 2);
    unknown.llm_response.usage_metadata = None;
    fixture.append([unknown]);
    assert!(
        context_usage(&fixture.database.connection, fixture.session)
            .unwrap()
            .is_none()
    );
    fixture.append([event(40, 8)]);
    assert!(
        context_usage(&fixture.database.connection, fixture.session)
            .unwrap()
            .unwrap()
            .observed
    );
}
