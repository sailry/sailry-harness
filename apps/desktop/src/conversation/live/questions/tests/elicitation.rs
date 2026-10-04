use super::*;
use crate::mcp_fixture;
use std::sync::atomic::Ordering;

#[gpui::test]
fn opens_urls_only_after_consent(cx: &mut TestAppContext) {
    urls(cx, false);
}

#[gpui::test]
fn continues_task_input(cx: &mut TestAppContext) {
    urls(cx, true);
}

fn urls(cx: &mut TestAppContext, task: bool) {
    init(cx);
    for remote in [false, true] {
        let previous_url = cx.opened_url();
        let fixture = Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::turn_tools(vec![
                (
                    mcp_fixture::alias("input", "read"),
                    json!({})
                );
                2
            ]))
        });
        let url = "https://example.invalid/continue?task=fixture";
        let peer = fixture.runtime.block_on(async {
            if task {
                mcp_fixture::Server::task(json!({"mode":"url","url":url}), false).await
            } else {
                mcp_fixture::Server::url(url, false).await
            }
        });
        peer.package(&fixture.directory.path().join("project/package"));
        let Output::Plugin(_plugin) = fixture.execute(Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: 0,
        }) else {
            panic!("plugin expected")
        };
        fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Continue the task".into(),
        });
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        let first = ready(&view, visual, None);
        assert_eq!(visual.opened_url(), previous_url);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        tap(visual, "live-question-submit");
        assert_eq!(visual.opened_url().as_deref(), Some(url));
        let second = ready(&view, visual, Some(first));
        visual.update(|_, cx| cx.open_url("https://example.invalid/sentinel"));
        tap(visual, "live-question-decline");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .questions
                        .iter()
                        .any(|question| question.id == second && question.state == State::Declined)
                })
        });
        assert_eq!(
            visual.opened_url().as_deref(),
            Some("https://example.invalid/sentinel")
        );
        view.read_with(visual, |view, _| {
            assert_eq!(
                view.history.snapshot.as_ref().unwrap().page.questions[0].state,
                State::Answered(Answer::Opened)
            );
        });
        assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
        wait(visual, |cx| {
            view.read(cx).history.snapshot.as_ref().unwrap().page.runs[0].status
                == sailry_protocol::conversation::Status::Completed
        });
        let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Continue on the same connection".into(),
        }) else {
            panic!("turn expected")
        };
        let next = ready(&view, visual, Some(second));
        view.read_with(visual, |view, _| {
            assert_eq!(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .questions
                    .iter()
                    .find(|question| question.id == next)
                    .unwrap()
                    .turn,
                turn.id
            );
        });
        fixture.execute(Command::StopTurn { turn: turn.id });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .runs
                .last()
                .unwrap()
                .status
                == sailry_protocol::conversation::Status::Cancelled
        });
        assert_eq!(peer.initializations.load(Ordering::SeqCst), 1);
        assert_eq!(peer.calls.load(Ordering::SeqCst), 3);
        fixture.close();
    }
}

#[gpui::test]
fn optional_fields_and_decline(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(
            remote,
            vec![(mcp_fixture::alias("input", "read"), json!({})); 2],
        );
        let peer = fixture.runtime.block_on(mcp_fixture::Server::start(json!({"type":"object","properties":{
            "count":{"type":"integer","title":"Copies","minimum":1,"maximum":5},
            "email":{"type":"string","title":"Email","format":"email"},
            "publish":{"type":"boolean","title":"Publish"},
            "sections":{"type":"array","title":"Sections","items":{"type":"string","enum":["summary","details"]},"minItems":1}
        },"required":["count","publish","sections"]})));
        peer.package(&fixture.directory.path().join("project/package"));
        let Output::Plugin(_plugin) = fixture.execute(Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: 0,
        }) else {
            panic!("plugin expected")
        };
        fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Prepare a report".into(),
        });
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        let first = ready(&view, visual, None);
        view.update_in(visual, |view, window, cx| {
            let fields = &mut view.questions.editing.as_mut().unwrap().fields;
            let Some(Field::Line(input)) = &fields[0].input.field else {
                panic!("number input expected")
            };
            input.update(cx, |input, cx| input.set_value("2.5", window, cx));
            fields[2].input.selected.insert(1);
            fields[3].input.selected.insert(0);
            assert!(
                view.questions
                    .editing
                    .as_ref()
                    .unwrap()
                    .answer(cx)
                    .is_none()
            );
        });
        view.update_in(visual, |view, window, cx| {
            let fields = &mut view.questions.editing.as_mut().unwrap().fields;
            let Some(Field::Line(input)) = &fields[0].input.field else {
                unreachable!()
            };
            input.update(cx, |input, cx| input.set_value("2", window, cx));
            let answer = view.questions.editing.as_ref().unwrap().answer(cx).unwrap();
            assert_eq!(
                answer,
                Answer::Form(
                    json!({"count":2,"publish":false,"sections":["summary"]})
                        .as_object()
                        .unwrap()
                        .clone()
                )
            );
        });
        tap(visual, "live-form-include-1");
        assert!(view.read_with(visual, |view, _| {
            view.questions.editing.as_ref().unwrap().fields[1].included
        }));
        tap(visual, "live-form-input-1");
        visual.simulate_input("invalid email");
        assert_eq!(
            view.read_with(visual, |view, cx| {
                view.questions.editing.as_ref().unwrap().fields[1]
                    .input
                    .field
                    .as_ref()
                    .unwrap()
                    .value(cx)
            }),
            "invalid email"
        );
        tap(visual, "live-question-submit");
        wait(visual, |cx| {
            view.read(cx).questions.error == Some("question_invalid_answer")
        });
        view.read_with(visual, |view, cx| {
            assert!(view.questions.attempt.is_none());
            assert!(!view.questions.pending);
            let draft = &view.questions.editing.as_ref().unwrap().fields[1];
            assert!(draft.included);
            assert_eq!(
                draft.input.field.as_ref().unwrap().value(cx),
                "invalid email"
            );
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        visual.update(|window, cx| window.clear_notifications(cx));
        tap(visual, "live-form-include-1");
        tap(visual, "live-question-submit");
        let second = ready(&view, visual, Some(first));
        visual.update(|window, cx| window.clear_notifications(cx));
        crate::feedback::tests::settle(visual);
        let body = visual.debug_bounds("pending-question").unwrap();
        for selector in [
            "live-question-decline",
            "live-question-secondary",
            "live-question-submit",
        ] {
            let button = visual.debug_bounds(selector).unwrap();
            assert!(
                button.left() >= body.left() && button.right() <= body.right(),
                "{selector} must fit the popover"
            );
        }
        tap(visual, "live-question-decline");
        view.read_with(visual, |view, _| {
            assert!(
                view.questions.pending
                    || view.questions.attempt.is_some()
                    || view
                        .history
                        .snapshot
                        .as_ref()
                        .unwrap()
                        .page
                        .questions
                        .iter()
                        .any(|question| question.id == second && question.state == State::Declined),
                "decline was not admitted: open={}, current={:?}, error={:?}, pending={:?}",
                view.questions.open,
                current(view),
                view.questions.error,
                view.question_pending(second)
                    .map(|question| &question.state)
            );
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .questions
                        .iter()
                        .any(|question| question.id == second && question.state == State::Declined)
                })
        });
        assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        fixture.close();
    }
}
