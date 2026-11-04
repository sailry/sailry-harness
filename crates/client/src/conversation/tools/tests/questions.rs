use super::*;
use sailry_protocol::{
    QuestionId,
    conversation::question::{Input, Question, Spec, State as QuestionState},
};

fn review() -> Page {
    let mut page = page();
    page.runs[0].status = Status::Completed;
    page.runs[0].finished_ms = Some(2000);
    std::sync::Arc::make_mut(&mut page.entries)[0].author = "plan_review".into();
    let id = page.entries[0].id.clone();
    std::sync::Arc::make_mut(&mut page.entries)[0].parts[0] = Part::ToolCall {
        presentation: Default::default(),
        grouping: Default::default(),
        id: Some(id.clone()),
        name: "plan_review".into(),
        summary: input_summary(
            serde_json::to_value(Spec {
                prompt: String::new(),
                input: Input::Plan,
            })
            .unwrap(),
        ),
    };
    page.questions.push(Question {
        id: QuestionId::new(),
        session: page.session,
        turn: page.runs[0].turn,
        entry: id,
        index: 0,
        state: QuestionState::Pending,
    });
    page
}

#[test]
fn retains_only_canonical_review_consent() {
    for (case_name, run) in [
        (
            "retains_completed_review_as_consent",
            (|case_name| {
                let page = review();
                let calls = collect(&page);
                assert_eq!(calls[0].state, State::Waiting, "{case_name}");
                assert_eq!(
                    calls[0].question_pending(&page),
                    Some(&page.questions[0]),
                    "{case_name}"
                );
                assert_eq!(page.runs[0].status, Status::Completed, "{case_name}");
                let restored: Page =
                    serde_json::from_slice(&serde_json::to_vec(&page).unwrap()).unwrap();
                assert_eq!(collect(&restored), calls, "{case_name}");
            }) as fn(&str),
        ),
        (
            "excludes_unowned_and_nonsynthetic_completion",
            (|case_name| {
                for case in [
                    "origin",
                    "session",
                    "author",
                    "tool",
                    "prompt",
                    "failed",
                    "cancelled",
                    "closed",
                ] {
                    let mut page = review();
                    match case {
                        "origin" => page.runs[0].origin = Some(SessionId::new()),
                        "session" => page.runs[0].session = SessionId::new(),
                        "author" => {
                            std::sync::Arc::make_mut(&mut page.entries)[0].author =
                                "assistant".into()
                        }
                        "tool" => {
                            if let Part::ToolCall { name, .. } =
                                &mut std::sync::Arc::make_mut(&mut page.entries)[0].parts[0]
                            {
                                *name = "ask_user".into();
                            }
                        }
                        "prompt" => {
                            if let Part::ToolCall { summary, .. } =
                                &mut std::sync::Arc::make_mut(&mut page.entries)[0].parts[0]
                            {
                                summary.question.as_mut().unwrap().prompt =
                                    "Model plan question".into();
                            }
                        }
                        "failed" => page.runs[0].status = Status::Failed,
                        "cancelled" => page.runs[0].status = Status::Cancelled,
                        _ => page.questions[0].state = QuestionState::Cancelled,
                    }
                    assert!(
                        collect(&page)[0].question_pending(&page).is_none(),
                        "{case_name}: {}",
                        format_args!("{case}")
                    );
                }
            }) as fn(&str),
        ),
        (
            "canonical_result_closes_pending_review",
            (|case_name| {
                let mut page = review();
                let mut result = entry(2, page.runs[0].turn, "", true);
                result.author = "plan_review".into();
                result.parts[0] = Part::ToolResult {
                    id: Some(page.questions[0].entry.clone()),
                    name: "plan_review".into(),
                    summary: Box::default(),
                    images: Vec::new(),
                };
                std::sync::Arc::make_mut(&mut page.entries).push(result);
                let call = &collect(&page)[0];
                assert_eq!(
                    page.questions[0].state,
                    QuestionState::Pending,
                    "{case_name}"
                );
                assert_eq!(call.state, State::Returned, "{case_name}");
                assert!(call.question_pending(&page).is_none(), "{case_name}");
                assert!(call.result(&page).is_none(), "{case_name}");
                page.questions[0].state = QuestionState::Cancelled;
                let call = &collect(&page)[0];
                assert_eq!(call.state, State::Returned, "{case_name}");
                assert!(call.question_pending(&page).is_none(), "{case_name}");
                assert!(call.result(&page).is_none(), "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
