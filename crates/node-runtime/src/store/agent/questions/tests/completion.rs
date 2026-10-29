use super::*;
use crate::agent::canonical::{Block, Finish, Record};

mod deferred;

fn source(turn: TurnId) -> Record {
    let mut event = message(
        "assistant",
        MessageRole::Assistant,
        vec![BlockKind::Text {
            text: "The complete plan 中文 🙂".into(),
        }],
    );
    event.turn = turn;
    event.message_mut().unwrap().finish = Some(Finish::Completed);
    event
}

async fn append(fixture: &mut Fixture, event: Record) -> Result<(), Fault> {
    history::append(
        &mut fixture.database,
        fixture.session,
        fixture.turn,
        event,
        &fixture.events,
    )
    .await
}

#[tokio::test]
async fn preserves_canonical_identity() {
    let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan).await;
    let source = source(fixture.turn);
    let source_id = source.id.clone();
    append(&mut fixture, source).await.unwrap();
    let event = review::event(fixture.turn, source_id);
    assert!(event.message().is_none());
    assert_eq!(event.provenance.author, review::TOOL);
    let call = event.id.clone();
    append(&mut fixture, event).await.unwrap();
    fixture.request = ToolIdentity {
        name: review::TOOL.into(),
        id: Some(call.clone()),
        arguments: serde_json::json!({"prompt":"","input":{"kind":"plan"}}),
    };
    let mut wrong = fixture.request.clone();
    wrong.arguments["prompt"] = serde_json::json!("not the admitted review");
    assert!(
        begin(&mut fixture.database, fixture.turn, &wrong, &fixture.events)
            .await
            .is_err()
    );
    let question = fixture.begin().await;
    assert_eq!(question.entry, call);
    assert_eq!(question.index, 0);
    assert_eq!(
        spec(&fixture.database.connection, &question).await.unwrap(),
        Spec {
            prompt: String::new(),
            input: Input::Plan,
        }
    );
    assert!(
        begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            &fixture.events
        )
        .await
        .is_err()
    );
    let answer = Response::Answer(Answer::Text(String::new()));
    resolve(
        &fixture.database.connection,
        fixture.session,
        question.id,
        &answer,
    )
    .await
    .unwrap();
    let event = review::result(fixture.turn, call, answer);
    append(&mut fixture, event).await.unwrap();
    let page = history::pages::read(&fixture.database.connection, fixture.session, None, 100)
        .await
        .unwrap()
        .page;
    assert_eq!(
        page.questions[0].state,
        State::Answered(Answer::Text(String::new()))
    );
    assert!(page.entries.iter().any(|entry| {
        entry
            .parts
            .contains(&Part::Text("The complete plan 中文 🙂".into()))
    }));
    let entry =
        page.entries
            .iter()
            .find(|entry| {
                entry.parts.iter().any(
                    |part| matches!(part, Part::ToolResult { name, .. } if name == review::TOOL),
                )
            })
            .unwrap();
    let index = entry
        .parts
        .iter()
        .position(|part| matches!(part, Part::ToolResult { name, .. } if name == review::TOOL))
        .unwrap();
    let Part::ToolResult { id, summary, .. } = &entry.parts[index] else {
        unreachable!()
    };
    let detail = history::tools::page(
        &fixture.database.connection,
        &fixture.database.tool_details,
        tool::detail::Reference {
            session: page.session,
            turn: entry.turn,
            entry: entry.id.clone(),
            index,
            call: id.clone(),
            revision: summary.revision.clone(),
        },
        None,
        tool::detail::MAX_DETAIL_PAGE_BYTES,
    )
    .await
    .unwrap();
    let detail: tool::detail::Detail = serde_json::from_str(&detail.text).unwrap();
    assert_eq!(
        detail.result,
        Some(serde_json::json!({"status":"answered","answer":""}))
    );
}

#[tokio::test]
async fn requires_completed_planning_source() {
    for case in ["code", "user", "call"] {
        let mut fixture = Fixture::with_input(
            if case == "code" {
                WorkMode::Code
            } else {
                WorkMode::Plan
            },
            Input::Plan,
        )
        .await;
        let mut source = source(fixture.turn);
        if case == "user" {
            let message = source.message_mut().unwrap();
            message.role = MessageRole::User;
            message.content[0].kind = BlockKind::Text {
                text: "A user message".into(),
            };
        }
        if case == "call" {
            source.message_mut().unwrap().content = vec![Block {
                index: 0,
                kind: BlockKind::ToolRequest(tool_request(
                    "incomplete-call",
                    "read_file",
                    serde_json::json!({}),
                )),
            }];
        }
        let source_id = source.id.clone();
        append(&mut fixture, source).await.unwrap();
        let event = review::event(fixture.turn, source_id);
        assert!(
            append(&mut fixture, event).await.is_err(),
            "invalid review accepted: {case}"
        );
    }
}

#[tokio::test]
async fn retains_nonempty_model_prompts() {
    let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan).await;
    fixture.request.arguments["prompt"] = serde_json::json!("");
    let fault = begin(
        &mut fixture.database,
        fixture.turn,
        &fixture.request,
        &fixture.events,
    )
    .await
    .unwrap_err();
    assert_eq!(fault.code, ErrorCode::InvalidRequest);
    assert_eq!(fault.message, "question prompt is empty");
}
