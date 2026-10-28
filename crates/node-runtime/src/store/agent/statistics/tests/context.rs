use super::*;

#[tokio::test]
async fn keeps_helper_usage_separate_from_context() {
    let mut fixture = Fixture::new().await;
    fixture.append([event(100, 20)]).await;
    let helper = |input, output| {
        let mut event = event(input, output);
        event.provenance.author = "system".into();
        event.message_mut().unwrap().content.clear();
        event
            .metadata
            .execution
            .get_or_insert_with(Default::default)
            .purpose = Some(canonical::Purpose::Compaction);
        event
    };
    let maintenance = helper(800, 40);
    let projected =
        crate::agent::engine::history::project(&fixture.database.connection, &maintenance)
            .await
            .unwrap()
            .unwrap();
    assert!(projected.metadata.usage.as_ref().unwrap().is_compaction);
    fixture.append([maintenance]).await;
    let statistics = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(statistics.context_tokens, Some(120));
    assert_eq!(statistics.usage.as_ref().unwrap().input, 900);
    let mut projection = record(
        "system",
        canonical::Kind::Message(canonical::Message::default()),
    );
    projection
        .metadata
        .execution
        .get_or_insert_with(Default::default)
        .context_tokens = Some(60);
    fixture.append([projection]).await;
    fixture.append([helper(2000, 10)]).await;
    let statistics = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(statistics.context_tokens, None);
    assert_eq!(statistics.context_estimate, Some(60));
    assert_eq!(statistics.usage.as_ref().unwrap().input, 2900);
    assert_eq!(statistics.usage.as_ref().unwrap().output, 70);
    fixture.append([event(30, 6)]).await;
    let statistics = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(statistics.context_tokens, Some(36));
    assert_eq!(statistics.context_estimate, None);
}
fn user(text: &str) -> canonical::Record {
    message(
        "user",
        canonical::Role::User,
        vec![canonical::BlockKind::Text { text: text.into() }],
    )
}

fn tool(text: &str) -> canonical::Record {
    message(
        "assistant",
        canonical::Role::User,
        vec![canonical::BlockKind::ToolResponse(
            crate::store::agent::tests::fixture::response(
                Some("read"),
                "read_file",
                serde_json::json!({"content":text}),
            ),
        )],
    )
}

async fn assert_context(
    fixture: &Fixture,
    session: SessionId,
    tokens: u64,
    pending: &[&canonical::Record],
) {
    let usage = context_usage(&fixture.database.connection, session)
        .await
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
        let actual = actual.message().unwrap();
        let expected = expected.message().unwrap();
        assert_eq!(actual.role, expected.role);
        assert_eq!(
            serde_json::to_value(&actual.content).unwrap(),
            serde_json::to_value(&expected.content).unwrap()
        );
    }
    assert_eq!(
        read(&fixture.database.connection, session)
            .await
            .unwrap()
            .context_tokens,
        Some(tokens)
    );
}

#[tokio::test]
async fn counts_new_content() {
    let mut fixture = Fixture::new().await;
    let anchor = event(100, 20);
    fixture
        .append([user("Already observed"), anchor.clone()])
        .await;
    assert_context(&fixture, fixture.session, 120, &[]).await;

    let mut result = tool("New tool result 中文 🙂");
    result.created = anchor.created - chrono::Duration::seconds(60);
    let followup = user("Use the new evidence");
    let mut media = event(500, 40);
    media
        .metadata
        .execution
        .get_or_insert_with(Default::default)
        .media = Some(sailry_protocol::media::Kind::Image);
    fixture
        .append([result.clone(), media, followup.clone()])
        .await;
    assert_context(&fixture, fixture.session, 120, &[&result, &followup]).await;
    let statistics = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(statistics.responses, 2);
    assert_eq!(statistics.usage.as_ref().unwrap().input, 600);
    assert_eq!(statistics.usage.as_ref().unwrap().output, 60);

    let latest = user("After the next observation");
    fixture.append([event(200, 30), latest.clone()]).await;
    assert_context(&fixture, fixture.session, 230, &[&latest]).await;
}

#[tokio::test]
async fn excludes_cached_subsets_from_context() {
    let mut fixture = Fixture::new().await;
    let mut response = event(200_000, 12_000);
    let usage = response.message_mut().unwrap().usage.as_mut().unwrap();
    usage.cache_read = Some(190_000);
    usage.reasoning = Some(10_000);
    fixture.append([event(300_000, 20_000), response]).await;
    assert_context(&fixture, fixture.session, 212_000, &[]).await;
    let statistics = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let usage = statistics.usage.unwrap();
    assert_eq!(usage.input, 500_000);
    assert_eq!(usage.output, 32_000);
    assert_eq!(usage.cached_input, 190_002);
    assert_eq!(usage.reasoning, 10_001);
}

#[tokio::test]
async fn follows_fork_and_rewind_membership() {
    let mut fixture = Fixture::new().await;
    let source = fixture.session;
    let inherited = user("Inherited pending input");
    let first = fixture
        .append([timed(100, 20, 125_000), inherited.clone()])
        .await;
    let source_result = tool("Source-only evidence");
    let head = fixture
        .append([timed(200, 30, 250_000), source_result.clone()])
        .await;

    let Output::Session(fork) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: source,
            through: first,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("fork expected");
    };
    fixture.session = fork.id;
    let fork_input = user("Fork-only input");
    fixture.append([fork_input.clone()]).await;
    assert_context(&fixture, fork.id, 120, &[&inherited, &fork_input]).await;
    assert_eq!(
        read(&fixture.database.connection, fork.id)
            .await
            .unwrap()
            .first_token_us,
        Some(125_000)
    );
    assert_context(&fixture, source, 230, &[&source_result]).await;
    assert_eq!(
        read(&fixture.database.connection, source)
            .await
            .unwrap()
            .first_token_us,
        Some(250_000)
    );

    let retained = fixture.fork(source, head).await;
    let Output::Rewound(rewind) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: source,
            through: Some(first),
            expected_head: head,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("rewind expected");
    };
    assert_context(&fixture, source, 120, &[&inherited]).await;
    assert_eq!(
        read(&fixture.database.connection, source)
            .await
            .unwrap()
            .first_token_us,
        Some(125_000)
    );
    assert!(rewind.backup.is_none());
    assert_context(&fixture, retained.id, 230, &[&source_result]).await;
    assert_context(&fixture, fork.id, 120, &[&inherited, &fork_input]).await;

    super::super::super::history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_context(&fixture, source, 120, &[&inherited]).await;
    assert_context(&fixture, retained.id, 230, &[&source_result]).await;
    assert_context(&fixture, fork.id, 120, &[&inherited, &fork_input]).await;
}

#[tokio::test]
async fn freezes_observed_model_limits() {
    let mut fixture = Fixture::new().await;
    let config = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
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
    )
    .await
    else {
        panic!("provider expected")
    };
    fixture.append([event(100, 20)]).await;
    let observed = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
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
    )
    .await;
    let retained = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(
        (retained.context_tokens, retained.context_limit),
        (Some(120), Some(1_000))
    );
    assert_eq!(retained.context_model.as_deref(), Some("fixture"));
    assert_eq!(retained.context_timestamp_ms, observed.context_timestamp_ms);
    assert_eq!(retained.budget.as_ref().unwrap().context_limit, 2_000);
    let turn = fixture.append([event(200, 30)]).await;
    let latest = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
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
        .await
        .unwrap();
    let historical = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(
        (historical.context_tokens, historical.context_limit),
        (Some(230), None)
    );
}

#[tokio::test]
async fn estimates_only_after_summary_reset() {
    let mut fixture = Fixture::new().await;
    let anchor = event(14_000, 10);
    fixture
        .append([user("Preserved requirement"), anchor.clone()])
        .await;
    let summary = summary(&anchor, "Verified handoff");
    fixture.append([summary]).await;
    let growth = tool(&"Unobserved pending evidence ".repeat(3000));
    fixture.append([growth.clone()]).await;
    let usage = context_usage(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .unwrap();
    assert!(!usage.observed);
    assert_eq!(usage.tokens, 0);
    let totals = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(totals.context_tokens, None);
    assert_eq!(
        totals.context_estimate,
        Some(crate::agent::estimate_context(&usage.pending))
    );
    assert!(totals.context_estimate.unwrap() > 0);
    assert_eq!(totals.usage.as_ref().unwrap().input, 14_000);
    assert_eq!(totals.usage.as_ref().unwrap().output, 10);
    assert!(matches!(&usage.pending[0].kind,
            canonical::Kind::Compaction(value) if matches!(&value.summary.content[0].kind,
                canonical::BlockKind::Text { text } if text.contains("Verified handoff"))));
    assert_eq!(usage.pending.last().unwrap().id, growth.id);
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .context_tokens,
        None
    );
    let mut unknown = event(10, 2);
    unknown.message_mut().unwrap().usage = None;
    fixture.append([unknown]).await;
    let pending = context_usage(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .unwrap();
    assert!(!pending.observed);
    assert!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .context_estimate
            .unwrap()
            > totals.context_estimate.unwrap()
    );
    fixture.append([event(40, 8)]).await;
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .context_estimate,
        None
    );
    assert!(
        context_usage(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .unwrap()
            .observed
    );
}
