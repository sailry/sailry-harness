use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init, open, tap},
    wait,
};
use core::prelude::v1::test;
use sailry_link::Transport;
use serde_json::json;
use std::time::Duration;

mod lifecycle;
mod permissions;
#[cfg(unix)]
mod process;

fn sample(display: Option<serde_json::Value>) -> (Call, Page) {
    let turn = sailry_protocol::TurnId::new();
    let call = Call {
        turn,
        id: Some("call".into()),
        name: "example".into(),
        presentation: Default::default(),
        grouping: Default::default(),
        source: sailry_client::conversation::tools::Reference {
            entry: "entry".into(),
            index: 0,
        },
        response: None,
        state: sailry_client::conversation::tools::State::Waiting,
        approval: None,
        question: None,
        progress: None,
        content: None,
        resolved: None,
    };
    let page = Page {
        session: sailry_protocol::SessionId::new(),
        revision: 1,
        entries: vec![sailry_protocol::conversation::Entry {
            sequence: 1,
            id: "entry".into(),
            turn,
            author: "assistant".into(),
            branch: String::new(),
            timestamp_ms: 1,
            parts: vec![sailry_protocol::conversation::Part::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: json!({"name":"**Task**\n```text\n🙂", "text":"Unrelated write body"}),
                presentation: Default::default(),
                grouping: Default::default(),
                display: display.map(|display| serde_json::from_value(display).unwrap()),
            }],
            citations: vec![],
            search_suggestions: None,
            usage: None,
        }],
        runs: vec![],
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        next_before: None,
    };
    (call, page)
}

#[test]
fn uses_descriptions_without_input_fields() {
    let (call, page) = sample(Some(json!({
        "label":"Example", "approval":[{
            "message":{"label":"Create %{name}"},
            "values":{"name":{"source":"arguments", "path":"/name"}},
        }],
    })));
    let details = details(&call, &page);
    assert_eq!(
        details.description.as_deref(),
        Some("Create **Task**\n```text\n🙂")
    );
    assert_eq!(details.prompt, details.description);
    assert_eq!(details.context, None);
}

#[test]
fn never_uses_argument_payloads_as_descriptions() {
    for display in [None, Some(json!({"label":"Example"}))] {
        let (call, page) = sample(display);
        assert!(details(&call, &page).description.is_none());
    }
    let (call, page) = sample(Some(json!({
        "label":"Example", "input":{
            "summary":{"path":"/name", "code":true},
            "content":{"path":"/text"},
        },
    })));
    assert_eq!(
        details(&call, &page).description.as_deref(),
        Some("**Task**\n```text\n🙂")
    );
}

impl Fixture {
    fn new(remote: bool) -> Self {
        Self::with_tools(
            remote,
            vec![
                (
                    crate::agent_fixture::plugin_tool("files", "write_file"),
                    json!({"path": "资料.txt", "text": "完整内容 中文 🙂\n```rust\n", "expected_revision": null}),
                ),
                (
                    crate::agent_fixture::plugin_tool("files", "write_file"),
                    json!({"path": "denied.txt", "text": "never", "expected_revision": null}),
                ),
            ],
        )
    }
}

fn pending(view: &Entity<View>, cx: &App) -> Option<ApprovalId> {
    view.read(cx)
        .history
        .snapshot
        .as_ref()?
        .page
        .approvals
        .iter()
        .find(|approval| approval.state == ApprovalState::Pending)
        .map(|approval| approval.id)
}

#[gpui::test]
fn resolves_and_reopens(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx).is_some()
        });
        let turn = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.runs[0].turn
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let avatar = Box::leak(format!("live-turn-avatar-{turn}").into_boxed_str());
        assert!(visual.debug_bounds(avatar).is_some());
        let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-approval-title-{id}").into_boxed_str()
                ))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-approval-content-{id}").into_boxed_str()
                ))
                .is_none()
        );
        assert!(!fixture.directory.path().join("project/资料.txt").exists());
        assert!(view.read_with(visual, |view, _| view.approvals.open));
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert!(!view.read_with(visual, |view, _| view.approvals.open));
        view.update(visual, |view, cx| {
            view.approvals.observe(&view.history);
            cx.notify();
        });
        assert!(!view.read_with(visual, |view, _| view.approvals.open));
        tap(visual, "live-approval-trigger");
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-approval-description-{id}").into_boxed_str()
                ))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-approval-details-{id}").into_boxed_str()
                ))
                .is_none()
        );
        visual.simulate_keystrokes("escape");
        visual.executor().advance_clock(Duration::from_millis(400));
        visual.run_until_parked();
        tap(visual, "live-chat-input");
        visual.simulate_input("next draft 中文");
        tap(visual, "live-approval-trigger");
        tap(visual, &format!("live-approval-approve-{id}"));
        wait(visual, |cx| {
            pending(&view, cx).is_some_and(|next| next != id) && !view.read(cx).approvals.pending
        });
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/资料.txt")).unwrap(),
            "完整内容 中文 🙂\n```rust\n"
        );
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "next draft 中文"
        );
        let next = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && pending(&view, cx) == Some(next)
        });
        tap(visual, &format!("live-approval-reject-{next}"));
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).approvals.pending
        });
        view.read_with(visual, |view, _| {
            assert_eq!(
                view.history.calls[0].approval.as_ref().unwrap().state,
                ApprovalState::Approved
            );
            assert_eq!(
                view.history.calls[1].approval.as_ref().unwrap().state,
                ApprovalState::Denied
            );
            assert!(view.approvals.error.is_none());
        });
        assert!(visual.debug_bounds("pending-approvals").is_none());
        assert!(!fixture.directory.path().join("project/denied.txt").exists());
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
