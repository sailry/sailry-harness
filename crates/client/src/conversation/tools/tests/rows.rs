use super::*;
use sailry_protocol::{
    ErrorCode,
    conversation::{Approval, ApprovalSource, ApprovalState},
    tool::{Failure, Presentation, detail::Summary},
};

fn returned(summary: Summary) -> Page {
    let mut page = page();
    let mut response = entry(2, page.runs[0].turn, "", true);
    let Part::ToolResult {
        summary: target, ..
    } = &mut response.parts[0]
    else {
        unreachable!()
    };
    **target = summary;
    page.entries.push(response);
    page
}

#[test]
fn distinguishes_errors_uncertainty_and_expected_absence() {
    for (code, expected, failed, warning, state) in [
        (ErrorCode::Internal, None, true, false, RowState::Failed),
        (ErrorCode::Unavailable, None, true, false, RowState::Failed),
        (ErrorCode::Cancelled, None, false, true, RowState::Cancelled),
        (
            ErrorCode::OutcomeUnknown,
            None,
            false,
            true,
            RowState::Interrupted,
        ),
        (
            ErrorCode::NotFound,
            Some(sailry_protocol::tool::Expected::FileNotFound),
            false,
            false,
            RowState::Failed,
        ),
    ] {
        let page = returned(Summary {
            failure: Some(Failure { code: Some(code) }),
            diagnostics: true,
            expected,
            has_details: true,
            ..Default::default()
        });
        let calls = collect(&page);
        let row = calls[0].row(&page);
        assert_eq!((row.failed, row.warning), (failed, warning), "{code:?}");
        assert_eq!(row.state, (failed || warning).then_some(state));
        assert!(row.has_details);
    }
}

#[test]
fn shows_captured_error_in_summary() {
    let page = returned(Summary {
        failure: Some(Failure {
            code: Some(ErrorCode::Unavailable),
        }),
        error: Some("File could not be read".into()),
        ..Default::default()
    });
    let calls = collect(&page);
    let row = calls[0].row(&page);
    assert_eq!(row.state, Some(RowState::Failed));
    assert_eq!(row.detail.unwrap().text, "File could not be read");
}

#[test]
fn preserves_terminal_and_active_states() {
    let page = page();
    for (state, code, warning, failed) in [
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
        let mut call = collect(&page).remove(0);
        call.state = state;
        call.failure = code.map(|code| Failure { code: Some(code) });
        let row = call.row(&page);
        assert_eq!(
            (row.warning, row.failed),
            (warning, failed),
            "{state:?} {code:?}"
        );
    }
}

#[test]
fn hides_details_when_approval_is_denied() {
    let mut page = returned(Summary {
        has_details: true,
        ..Default::default()
    });
    page.approvals.push(Approval {
        id: sailry_protocol::ApprovalId::new(),
        session: page.session,
        turn: page.runs[0].turn,
        entry: page.entries[0].id.clone(),
        index: 0,
        state: ApprovalState::Denied,
        source: ApprovalSource::User,
        access: None,
        startup: None,
        ssh: None,
        can_remember: false,
        prefix: None,
        choice: None,
        reason: None,
    });
    let calls = collect(&page);
    let row = calls[0].row(&page);
    assert_eq!(row.state, Some(RowState::Denied));
    assert!(row.warning);
    assert!(!row.has_details);
    page.approvals[0].state = ApprovalState::Pending;
    let row = collect(&page)[0].row(&page);
    assert!(row.pending);
    assert!(!row.running);
}

#[test]
fn preserves_bounded_labels_and_path_fallback() {
    let page = page();
    let call = &collect(&page)[0];
    assert_eq!(call.row(&page).detail.unwrap().text, "source.rs");
    let mut page = page;
    let Part::ToolCall { summary, .. } = &mut page.entries[0].parts[0] else {
        unreachable!()
    };
    summary.resolved = Some(display::Resolved {
        label: Some(sailry_protocol::tool::LocalText {
            label: "Read".into(),
            locales: Default::default(),
        }),
        input: display::Input {
            summary: Some(display::Text {
                text: "Captured name".into(),
                code: true,
            }),
            ..Default::default()
        },
        output: None,
        approval: vec![],
        status: None,
        command: None,
        diagnostics: vec![],
    });
    let row = collect(&page)[0].row(&page);
    assert_eq!(row.label.unwrap().label, "Read");
    assert_eq!(
        row.detail.unwrap(),
        display::Text {
            text: "Captured name".into(),
            code: true
        }
    );
}

#[test]
fn retains_hidden_activity_exceptions() {
    let mut page = returned(Summary::default());
    let Part::ToolCall { presentation, .. } = &mut page.entries[0].parts[0] else {
        unreachable!()
    };
    *presentation = Presentation::Hidden;
    assert!(collect(&page)[0].hidden(&page));
    let Part::ToolResult { summary, .. } = &mut page.entries[1].parts[0] else {
        unreachable!()
    };
    summary.diagnostics = true;
    assert!(!collect(&page)[0].hidden(&page));
    let Part::ToolCall { name, .. } = &mut page.entries[0].parts[0] else {
        unreachable!()
    };
    *name = "ask_user".into();
    assert!(collect(&page).iter().all(|call| !call.hidden(&page)));
}

#[test]
fn borrows_complete_details_without_snapshot_mutation() {
    let page = returned(Summary {
        has_details: true,
        raw_result: true,
        ..Default::default()
    });
    let original = page.clone();
    let call = collect(&page).remove(0);
    assert!(call.result(&page).is_none());
    let detail = Arc::new(sailry_protocol::tool::detail::Detail {
        arguments: json!({"path":"source.rs"}),
        result: Some(json!({"text":"中文 🙂".repeat(50_000)})),
        resolved: None,
        content: None,
        images: vec![],
    });
    let hydrated = call.with_detail(detail.clone());
    assert!(Arc::ptr_eq(hydrated.detail.as_ref().unwrap(), &detail));
    assert_eq!(hydrated.arguments(&page).unwrap()["path"], "source.rs");
    assert_eq!(hydrated.result(&page), detail.result.as_ref());
    assert_eq!(page, original);
    assert!(
        serde_json::to_value(hydrated)
            .unwrap()
            .get("detail")
            .is_none()
    );
}

#[test]
fn uses_captured_child_titles_and_role_names() {
    let mut page = page();
    let mut child = sailry_protocol::conversation::Child {
        run: page.runs[0].clone(),
        origin: sailry_protocol::Delegation {
            session: page.session,
            turn: page.runs[0].turn,
            entry: page.entries[0].id.clone(),
            index: 0,
            role: Some(sailry_protocol::RoleId::new()),
        },
        name: Some("Explore".into()),
    };
    let Part::ToolCall { summary, .. } = &mut page.entries[0].parts[0] else {
        unreachable!()
    };
    summary.title = Some("Review sources".into());
    assert_eq!(
        display::child_title(Some(&page), &child).as_deref(),
        Some("Explore Review sources")
    );
    child.origin.role = None;
    assert_eq!(
        display::child_title(Some(&page), &child).as_deref(),
        Some("Review sources")
    );
    child.origin.entry = "unloaded".into();
    assert_eq!(
        display::child_title(Some(&page), &child).as_deref(),
        Some("Explore")
    );
    assert_eq!(
        display::child_title(None, &child).as_deref(),
        Some("Explore")
    );
    child.name = Some(" \n ".into());
    assert!(display::child_title(Some(&page), &child).is_none());
}

#[test]
fn published_only_content_has_no_empty_disclosure() {
    let mut page = returned(Summary {
        has_details: false,
        published: true,
        ..Default::default()
    });
    let call = collect(&page).remove(0);
    let result = json!({"sailry_content":{"version":1,"blocks":[{
        "kind":"file","path":"index.html","mime":"text/html"
    }]},"text":"Original HTML"});
    let mut detail = sailry_protocol::tool::detail::Detail {
        arguments: json!({"path":"index.html"}),
        content: Presentation::Content.content(&result),
        result: Some(result),
        resolved: None,
        images: vec![],
    };
    assert!(!call.details(&page));
    assert!(!call.with_detail(Arc::new(detail.clone())).details(&page));
    detail
        .content
        .as_mut()
        .unwrap()
        .blocks
        .push(sailry_protocol::tool::Block::Text(
            sailry_protocol::tool::Text {
                path: "/text".into(),
                notices: vec![],
            },
        ));
    let Part::ToolResult { summary, .. } = &mut page.entries[1].parts[0] else {
        unreachable!()
    };
    summary.has_details = true;
    assert!(call.details(&page));
    assert!(call.with_detail(Arc::new(detail)).details(&page));
}

#[test]
fn keeps_summary_disclosure_after_hydration() {
    let page = returned(Summary {
        has_details: true,
        raw_result: true,
        published: true,
        ..Default::default()
    });
    let call = collect(&page).remove(0);
    let result = json!({"sailry_content":{"version":1,"blocks":[{
        "kind":"file","path":"index.html","mime":"text/html"
    }]},"text":"Original HTML"});
    let detail = sailry_protocol::tool::detail::Detail {
        arguments: json!({"path":"index.html"}),
        content: Presentation::Content.content(&result),
        result: Some(result),
        resolved: None,
        images: vec![],
    };
    assert!(call.details(&page));
    assert!(call.with_detail(Arc::new(detail)).details(&page));
}
