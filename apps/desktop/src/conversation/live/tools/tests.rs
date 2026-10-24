use super::*;
use core::prelude::v1::test;
use sailry_client::conversation::tools::display;
use sailry_protocol::{ErrorCode, SessionId, conversation::Part, tool};
use serde_json::json;

fn page() -> Page {
    Page {
        session: SessionId::new(),
        revision: 1,
        next_before: None,
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        background: vec![],
        runs: vec![],
        entries: vec![],
    }
}

fn call(state: State, code: Option<ErrorCode>) -> Call {
    Call {
        turn: TurnId::new(),
        id: Some("call".into()),
        name: "fixture".into(),
        terminal_interaction: false,
        presentation: Default::default(),
        grouping: Default::default(),
        source: sailry_client::conversation::tools::Reference {
            entry: "source".into(),
            index: 0,
        },
        response: None,
        state,
        failure: code.map(|code| tool::Failure { code: Some(code) }),
        approval: None,
        question: None,
        progress: None,
        content: None,
        resolved: None,
    }
}

#[test]
fn distinguishes_uncertain_outcomes_from_errors() {
    let page = page();
    for (state, code, warning, error) in [
        (
            State::Returned,
            Some(ErrorCode::OutcomeUnknown),
            true,
            false,
        ),
        (State::Returned, Some(ErrorCode::Cancelled), true, false),
        (State::Interrupted, None, true, false),
        (State::NotExecuted, None, true, false),
        (State::Cancelled, None, true, false),
        (State::Returned, Some(ErrorCode::Internal), false, true),
        (State::Returned, Some(ErrorCode::Unavailable), false, true),
        (State::Returned, None, false, false),
        (State::Running, None, false, false),
    ] {
        let summary = summary(&call(state, code), &page);
        assert_eq!(summary.detail.warning, warning, "{state:?} {code:?}");
        assert_eq!(summary.detail.failed, error, "{state:?} {code:?}");
    }
}

#[test]
fn preserves_group_outcome_priority() {
    let page = page();
    let unknown = call(State::Returned, Some(ErrorCode::OutcomeUnknown));
    let success = call(State::Returned, None);
    let error = call(State::Returned, Some(ErrorCode::Internal));
    let running = call(State::Running, None);
    let warning = group_summary(&[&unknown, &success], &page);
    assert!(warning.detail.warning);
    assert!(!warning.detail.failed);
    let failure = group_summary(&[&unknown, &error, &success], &page);
    assert!(!failure.detail.warning);
    assert!(failure.detail.failed);
    let active = group_summary(&[&unknown, &error, &running], &page);
    assert!(active.detail.running);
    assert!(!active.detail.warning);
    assert!(!active.detail.failed);
}

#[test]
fn avoids_duplicate_file_context() {
    let mut call = call(State::Returned, Some(ErrorCode::InvalidRequest));
    call.name = "read_file".into();
    call.presentation = tool::Presentation::Details;
    call.response = Some(sailry_client::conversation::tools::Reference {
        entry: "source".into(),
        index: 1,
    });
    call.resolved = Some(display::Resolved {
        label: None,
        input: display::Input {
            summary: Some(display::Text {
                text: "docs".into(),
                code: true,
            }),
            ..Default::default()
        },
        output: Some(display::Output {
            preview: String::new(),
            preview_deferred: false,
            notices: vec![],
            paths: vec!["docs".into()],
            body: None,
            table: None,
        }),
        approval: vec![],
        status: None,
        command: None,
        diagnostics: vec![],
    });
    let mut page = page();
    page.entries.push(sailry_protocol::conversation::Entry {
        id: "source".into(),
        sequence: 1,
        turn: call.turn,
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 1,
        parts: vec![
            Part::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: json!({"path":"docs"}),
                presentation: call.presentation,
                grouping: call.grouping,
                display: None,
            },
            Part::ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                result: json!({"error":sailry_protocol::Fault::invalid("only regular files can be read")}),
                images: vec![],
            },
        ],
        usage: None,
        citations: vec![],
        search_suggestions: None,
    });
    let original = page.clone();
    assert!(call.details(&page));
    assert_eq!(summary(&call, &page).detail.text.as_ref(), "docs");
    assert!(!projection::grouped(&call, &page));
    assert_eq!(
        output::failure(call.failure.as_ref(), call.result(&page)).as_deref(),
        Some("only regular files can be read")
    );
    assert!(summary(&call, &page).detail.failed);
    for paths in [
        vec!["docs".into(), "notes".into()],
        vec!["different.txt".into()],
    ] {
        call.resolved
            .as_mut()
            .unwrap()
            .output
            .as_mut()
            .unwrap()
            .paths = paths;
        assert!(projection::grouped(&call, &page));
    }
    assert_eq!(page, original);
}
