use super::*;

mod deferred;

fn source() -> AdkEvent {
    let mut event = AdkEvent::new("fixture");
    event.author = "assistant".into();
    event.set_content(adk_core::Content::new("model").with_text("The complete plan 中文 🙂"));
    event.llm_response.turn_complete = true;
    event
}

fn append(fixture: &mut Fixture, event: AdkEvent) -> adk_core::Result<()> {
    history::append(
        &mut fixture.database,
        fixture.session,
        fixture.turn,
        event,
        &fixture.events,
    )
}

#[test]
fn preserves_canonical_identity() {
    let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan);
    let source = source();
    let source_id = source.id.clone();
    append(&mut fixture, source).unwrap();
    let event = review::event(fixture.turn, source_id);
    assert!(event.content().is_none());
    assert_eq!(event.author, review::TOOL);
    let call = event.id.clone();
    append(&mut fixture, event).unwrap();
    fixture.request = ToolConfirmationRequest {
        acting: None,
        origin: None,
        tool_name: review::TOOL.into(),
        function_call_id: Some(call.clone()),
        args: serde_json::json!({"prompt":"","input":{"kind":"plan"}}),
    };
    let mut wrong = fixture.request.clone();
    wrong.args["prompt"] = serde_json::json!("not the admitted review");
    assert!(begin(&mut fixture.database, fixture.turn, &wrong, &fixture.events).is_err());
    let question = fixture.begin();
    assert_eq!(question.entry, call);
    assert_eq!(question.index, 0);
    assert_eq!(
        spec(&fixture.database.connection, &question).unwrap(),
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
        .is_err()
    );
    let answer = Response::Answer(Answer::Text(String::new()));
    resolve(
        &fixture.database.connection,
        fixture.session,
        question.id,
        &answer,
    )
    .unwrap();
    let event = review::result(fixture.turn, call, answer);
    append(&mut fixture, event).unwrap();
    let page = history::pages::read(&fixture.database.connection, fixture.session, None, 100)
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
    assert!(page.entries.iter().any(|entry| {
        entry.parts.iter().any(|part| {
            matches!(part, Part::ToolResult {name,result,..}
        if name == review::TOOL && result == &serde_json::json!({"status":"answered","answer":""}))
        })
    }));
}

#[test]
fn requires_completed_planning_source() {
    for case in ["code", "user", "call", "content"] {
        let mut fixture = Fixture::with_input(
            if case == "code" {
                WorkMode::Code
            } else {
                WorkMode::Plan
            },
            Input::Plan,
        );
        let mut source = source();
        if case == "user" {
            source.set_content(adk_core::Content::new("user").with_text("A user message"));
        }
        if case == "call" {
            source.set_content(adk_core::Content {
                role: "model".into(),
                parts: vec![adk_core::Part::FunctionCall {
                    id: Some("incomplete-call".into()),
                    name: "read_file".into(),
                    args: serde_json::json!({}),
                    thought_signature: None,
                }],
            });
        }
        let source_id = source.id.clone();
        append(&mut fixture, source).unwrap();
        let mut event = review::event(fixture.turn, source_id);
        if case == "content" {
            event.set_content(adk_core::Content::new("model").with_text("Fabricated completion"));
        }
        assert!(
            append(&mut fixture, event).is_err(),
            "invalid review accepted: {case}"
        );
    }
}

#[test]
fn retains_nonempty_model_prompts() {
    let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan);
    fixture.request.args["prompt"] = serde_json::json!("");
    let fault = begin(
        &mut fixture.database,
        fixture.turn,
        &fixture.request,
        &fixture.events,
    )
    .unwrap_err();
    assert_eq!(fault.code, ErrorCode::InvalidRequest);
    assert_eq!(fault.message, "question prompt is empty");
}
