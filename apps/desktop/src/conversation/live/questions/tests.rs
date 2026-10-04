use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init, open, tap},
    wait,
};
use core::prelude::v1::test;
use serde_json::json;
use std::time::Duration;

mod elicitation;
mod lifecycle;
mod planning;

fn text(multiline: bool, max_bytes: usize) -> (String, serde_json::Value) {
    (
        "ask_user".into(),
        json!({"prompt": "Describe the task", "input": {"kind": "text", "multiline": multiline, "max_bytes": max_bytes}}),
    )
}

fn current(view: &View) -> Option<QuestionId> {
    view.questions.editing.as_ref().map(|editing| editing.id)
}

fn ready(
    view: &Entity<View>,
    visual: &mut VisualTestContext,
    previous: Option<QuestionId>,
) -> QuestionId {
    wait(visual, |cx| {
        let view = view.read(cx);
        view.connected()
            && !view.questions.pending
            && view.questions.open
            && current(view).is_some_and(|id| Some(id) != previous)
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visual
            .debug_bounds("live-question-prompt")
            .unwrap()
            .size
            .height
            > px(0.)
    );
    view.read_with(visual, |view, _| current(view).unwrap())
}

#[gpui::test]
fn choices_text_and_resume(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(
            remote,
            vec![
                (
                    "ask_user".into(),
                    json!({"prompt": "Pick one", "input": {"kind": "choice", "options": ["One", "Two"], "multiple": false, "allow_other": true}}),
                ),
                (
                    "ask_user".into(),
                    json!({"prompt": "Pick several", "input": {"kind": "choice", "options": ["One", "Two", "Three"], "multiple": true, "allow_other": true}}),
                ),
                text(true, 32),
                text(false, 100),
            ],
        );
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        let first = ready(&view, visual, None);
        let first_choice = visual.debug_bounds("live-question-choice-0").unwrap();
        let second_choice = visual.debug_bounds("live-question-choice-1").unwrap();
        assert!(second_choice.top() - first_choice.bottom() >= px(12.));
        assert!(
            visual.debug_bounds("live-question-trigger").unwrap().top()
                - visual.debug_bounds("pending-question").unwrap().bottom()
                >= px(20.)
        );
        for selector in [
            "live-question-input",
            "live-question-secondary",
            "live-question-submit",
        ] {
            assert_eq!(visual.debug_bounds(selector).unwrap().size.height, px(32.));
        }
        tap(visual, "live-chat-input");
        visual.simulate_input("next composer draft 中文");
        tap(visual, "live-question-trigger");
        tap(visual, "live-question-submit");
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        tap(visual, "live-question-choice-2");
        tap(visual, "live-question-input");
        visual.simulate_input("custom 中文");
        visual.simulate_keystrokes("shift-tab shift-tab");
        let keystroke = Keystroke::parse("space").unwrap();
        visual.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        visual.simulate_event(KeyUpEvent { keystroke });
        visual.run_until_parked();
        assert_eq!(
            view.read_with(visual, |view, cx| view
                .questions
                .editing
                .as_ref()
                .unwrap()
                .answer(cx)),
            Some(Answer::Choices {
                selected: vec![1],
                other: None
            })
        );
        visual.simulate_keystrokes("escape");
        visual.executor().advance_clock(Duration::from_millis(400));
        visual.run_until_parked();
        assert!(!view.read_with(visual, |view, _| view.questions.open));
        assert!(visual.debug_bounds("pending-question").is_none());
        tap(visual, "live-question-trigger");
        tap(visual, "live-question-choice-0");
        view.read_with(visual, |view, cx| {
            let editing = view.questions.editing.as_ref().unwrap();
            assert_eq!(editing.text(cx), "custom 中文");
            assert_eq!(
                editing.answer(cx),
                Some(Answer::Choices {
                    selected: vec![0],
                    other: None
                })
            );
        });
        tap(visual, "live-question-choice-2");
        tap(visual, "live-question-submit");
        let second = ready(&view, visual, Some(first));
        tap(visual, "live-question-choice-0");
        tap(visual, "live-question-choice-2");
        tap(visual, "live-question-choice-2");
        tap(visual, "live-question-choice-3");
        tap(visual, "live-question-input");
        visual.simulate_input("Extra");
        visual.simulate_keystrokes("enter");
        let third = ready(&view, visual, Some(second));
        tap(visual, "live-question-input");
        visual.simulate_input(&"x".repeat(33));
        tap(visual, "live-question-submit");
        assert_eq!(view.read_with(visual, |view, _| current(view)), Some(third));
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        tap(visual, "live-question-input");
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input(" 中文 🙂");
        visual.simulate_keystrokes("enter");
        visual.simulate_input("next ");
        view.read_with(visual, |view, cx| {
            assert_eq!(
                view.questions.editing.as_ref().unwrap().text(cx),
                " 中文 🙂\nnext "
            );
            assert_eq!(view.input.read(cx).value(), "next composer draft 中文");
        });
        tap(visual, "live-question-submit");
        let fourth = ready(&view, visual, Some(third));
        assert!(visual.debug_bounds("pending-question").is_some());
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        assert_eq!(ready(&view, visual, None), fourth);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 4);
        tap(visual, "live-question-secondary");
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).questions.pending
        });
        view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            let results: Vec<_> = view
                .history
                .calls
                .iter()
                .map(|call| call.result(page).unwrap().clone())
                .collect();
            assert_eq!(
                results,
                vec![
                    json!({"status": "answered", "answers": ["custom 中文"]}),
                    json!({"status": "answered", "answers": ["One", "Extra"]}),
                    json!({"status": "answered", "answer": " 中文 🙂\nnext "}),
                    json!({"status": "cancelled"})
                ]
            );
            assert_eq!(page.runs[0].status, Status::Completed);
            assert!(view.questions.editing.is_none());
            assert!(view.questions.error.is_none());
        });
        assert!(visual.debug_bounds("pending-question").is_none());
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 5);
        let answer = Box::leak(format!("live-question-answer-{third}").into_boxed_str());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(answer).is_none());
        let turn = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.runs[0].turn
        });
        tap(visual, &format!("live-turn-work-{turn}"));
        tap(visual, &format!("live-question-history-{third}"));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(answer).is_some());
        tap(visual, &format!("live-question-history-{third}"));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(answer).is_none());
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-question-answer-{fourth}").into_boxed_str()
                ))
                .is_none()
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn preserves_unavailable_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![text(false, 100), text(false, 100)]);
        fixture.start();
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        let first = ready(&view, visual, None);
        tap(visual, "live-question-input");
        visual.simulate_input("unsent 中文");
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.history.connected = false;
                cx.notify();
            })
        });
        tap(visual, "live-question-input");
        visual.simulate_input(" offline");
        tap(visual, "live-question-submit");
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        assert!(view.read_with(visual, |view, _| view.questions.attempt.is_none()));
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.history.connected = true;
                cx.notify();
            })
        });
        fixture.execute(Command::ResolveQuestion {
            session: fixture.session.id,
            question: first,
            response: Response::Answer(Answer::Text("another controller".into())),
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.page.questions.len() == 2)
        });
        view.read_with(visual, |view, cx| {
            assert_eq!(current(view), Some(first));
            assert_eq!(
                view.questions.editing.as_ref().unwrap().text(cx),
                "unsent 中文 offline"
            );
            assert!(view.question_pending(first).is_none());
        });
        assert!(visual.debug_bounds("live-question-submit").is_none());
        tap(visual, "live-question-input");
        visual.simulate_input("must not change a closed draft");
        assert_eq!(
            view.read_with(visual, |view, cx| view
                .questions
                .editing
                .as_ref()
                .unwrap()
                .text(cx)),
            "unsent 中文 offline"
        );
        tap(visual, "live-question-secondary");
        let second = ready(&view, visual, Some(first));
        assert!(view.read_with(visual, |view, cx| {
            view.questions.editing.as_ref().unwrap().text(cx).is_empty()
        }));
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).pending
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.resolve_question(
                    second,
                    Response::Answer(Answer::Text("late".into())),
                    window,
                    cx,
                )
            })
        });
        assert!(view.read_with(visual, |view, _| view.questions.attempt.is_none()));
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        assert!(visual.debug_bounds("pending-question").is_none());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
