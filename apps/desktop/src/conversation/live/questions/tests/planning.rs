use super::*;
use sailry_link::{Admission, Pending, Subscription as Stream, Transport};
use sailry_protocol::{ErrorCode, Fault, Permission, RequestId, Topic, WorkMode};
use std::sync::Mutex;

fn proposal() -> (String, serde_json::Value) {
    (
        "ask_user".into(),
        json!({"prompt": "Review the plan 中文 🙂", "input": {"kind": "plan"}}),
    )
}

fn write() -> (String, serde_json::Value) {
    (
        crate::agent_fixture::plugin_tool("files", "write_file"),
        json!({"path": "plan.txt", "text": "Reviewed coding output 中文 🙂", "expected_revision": null}),
    )
}

fn fixture(remote: bool, tools: Vec<(String, serde_json::Value)>) -> Fixture {
    let mut fixture = Fixture::with_tools(remote, tools);
    let mut config = fixture.session.config.clone();
    config.mode = WorkMode::Plan;
    config.permission = Permission::Full;
    let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        config,
    }) else {
        panic!("session expected")
    };
    fixture.session = session;
    fixture.execute(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        message: "Plan the requested change".into(),
    });
    fixture
}

fn completed(view: &Entity<View>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        view.read(cx)
            .history
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| {
                snapshot.page.runs.len() == 2
                    && snapshot
                        .page
                        .runs
                        .iter()
                        .all(|run| run.status == Status::Completed)
            })
    });
}

#[gpui::test]
fn preserves_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote, vec![proposal(), write()]);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        let first = ready(&view, visual, None);
        tap(visual, "live-chat-input");
        visual.simulate_input("Unsent draft 中文 🙂");
        tap(visual, "live-question-trigger");
        view.read_with(visual, |view, cx| {
            assert_eq!(view.session.as_ref().unwrap().config.mode, WorkMode::Plan);
            assert_eq!(view.input.read(cx).value(), "Unsent draft 中文 🙂");
            assert!(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .approvals
                    .is_empty()
            );
        });
        assert!(!fixture.directory.path().join("project/plan.txt").exists());
        tap(visual, "live-question-submit");
        completed(&view, visual);
        wait(visual, |cx| !view.read(cx).questions.locked());
        visual.simulate_input(" retained");
        let session = view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), "Unsent draft 中文 🙂 retained");
            assert_eq!(view.session.as_ref().unwrap().config.mode, WorkMode::Code);
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert_eq!(page.questions[0].id, first);
            let State::Answered(Answer::Plan { turn }) = page.questions[0].state else { panic!("accepted plan expected") };
            assert_eq!(page.approvals.len(), 1);
            assert_eq!(page.approvals[0].turn, turn);
            assert_eq!(page.approvals[0].source, sailry_protocol::conversation::ApprovalSource::Full);
            assert!(page.entries.iter().filter(|entry| entry.turn == turn).flat_map(|entry| &entry.parts).any(|part| matches!(part, sailry_protocol::conversation::Part::Text(text) if text == &tr("plan_coding_message").to_string())));
            view.session.clone().unwrap()
        });
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/plan.txt")).unwrap(),
            "Reviewed coding output 中文 🙂"
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = open(cx, fixture.binding.clone(), session);
        completed(&view, visual);
        assert!(view.read_with(visual, |view, _| view.questions.editing.is_none()));
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn continues_without_generation(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote, vec![proposal(), write()]);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        let id = ready(&view, visual, None);
        tap(visual, "live-question-secondary");
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).questions.locked()
        });
        view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert_eq!(page.runs.len(), 1);
            assert_eq!(page.runs[0].status, Status::Completed);
            assert_eq!(page.questions[0].id, id);
            assert_eq!(
                page.questions[0].state,
                State::Answered(Answer::Text(String::new()))
            );
            assert_eq!(view.session.as_ref().unwrap().config.mode, WorkMode::Plan);
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        assert!(!fixture.directory.path().join("project/plan.txt").exists());
        let turn = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.runs[0].turn
        });
        tap(visual, &format!("live-turn-work-{turn}"));
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-question-answer-{id}").into_boxed_str()
                ))
                .is_some()
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

struct Held {
    inner: Arc<dyn Transport>,
    before: bool,
    ids: Mutex<Vec<RequestId>>,
    release: tokio::sync::Semaphore,
}

impl Transport for Held {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let first = if matches!(request.command, Command::ResolveQuestion { .. }) {
                let mut ids = self.ids.lock().unwrap();
                ids.push(request.id);
                ids.len() == 1
            } else {
                false
            };
            if first && self.before {
                self.release.acquire().await.unwrap().forget();
            }
            let admission = self.inner.dispatch(request).await?;
            if first && !self.before {
                admission.completion.await.unwrap().unwrap();
                self.release.acquire().await.unwrap().forget();
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "injected missing acceptance",
                ));
            }
            Ok(admission)
        })
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Stream>, Fault>> {
        self.inner.subscribe(topic)
    }
}

#[gpui::test]
fn recovers_with_newer_config(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for before in [false, true] {
            let fixture = fixture(remote, vec![proposal(), write()]);
            let held = Arc::new(Held {
                inner: fixture.transport.clone(),
                before,
                ids: Mutex::new(Vec::new()),
                release: tokio::sync::Semaphore::new(0),
            });
            let mut binding = fixture.binding.clone();
            binding.client = Arc::new(Client::new(held.clone()));
            let (view, visual) = open(cx, binding, fixture.session.clone());
            ready(&view, visual, None);
            tap(visual, "live-chat-input");
            visual.simulate_input("Draft survives acceptance");
            tap(visual, "live-question-trigger");
            tap(visual, "live-question-submit");
            wait(visual, |_| held.ids.lock().unwrap().len() == 1);
            if !before {
                completed(&view, visual);
            }
            let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
                panic!("snapshot expected")
            };
            let current = snapshot
                .sessions
                .iter()
                .find(|session| session.id == fixture.session.id)
                .unwrap();
            let mut config = current.config.clone();
            config.mode = WorkMode::Plan;
            config.effort = Effort::Low;
            let Output::Session(revised) = fixture.execute(Command::SetSessionConfig {
                session: current.id,
                expected_revision: current.revision,
                config,
            }) else {
                panic!("session expected")
            };
            wait(visual, |cx| {
                view.read(cx).session.as_ref().unwrap().revision == revised.revision
            });
            held.release.add_permits(1);
            wait(visual, |cx| view.read(cx).questions.error.is_some());
            view.read_with(visual, |view, cx| {
                assert_eq!(view.input.read(cx).value(), "Draft survives acceptance");
                assert_eq!(
                    view.questions.error,
                    Some(if before {
                        "plan_config_changed"
                    } else {
                        "chat_action_unknown"
                    })
                );
                assert_eq!(view.questions.attempt.is_some(), !before);
                assert_eq!(view.session.as_ref().unwrap().config, revised.config);
            });
            if before {
                assert!(!fixture.directory.path().join("project/plan.txt").exists());
                assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
            }
            tap(visual, "live-question-submit");
            completed(&view, visual);
            wait(visual, |cx| !view.read(cx).questions.locked());
            view.read_with(visual, |view, cx| {
                assert_eq!(view.input.read(cx).value(), "Draft survives acceptance");
                assert_eq!(
                    view.session.as_ref().unwrap().config.mode,
                    if before {
                        WorkMode::Code
                    } else {
                        WorkMode::Plan
                    }
                );
                assert_eq!(view.session.as_ref().unwrap().config.effort, Effort::Low);
                assert!(view.questions.error.is_none());
            });
            let ids = held.ids.lock().unwrap();
            assert_eq!(ids.len(), 2);
            assert_eq!(ids[0] == ids[1], !before);
            assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}

#[gpui::test]
fn retries_full_queue(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote, vec![proposal(), write()]);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        let question = ready(&view, visual, None);
        let mut held = None;
        for _ in 0..256 {
            let Output::QueuedTurn(turn) = fixture.execute(Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Held queued input".into(),
            }) else {
                panic!("queued turn expected")
            };
            held = Some(turn.id);
        }
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .queue
                .items
                .len()
                == 256
        });
        tap(visual, "live-question-submit");
        wait(visual, |cx| view.read(cx).questions.error.is_some());
        view.read_with(visual, |view, _| {
            assert!(view.questions.attempt.is_none());
            assert!(view.question_pending(question).is_some());
            assert_eq!(view.session.as_ref().unwrap().config.mode, WorkMode::Plan);
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        fixture.execute(Command::RemoveQueuedTurn {
            turn: held.unwrap(),
            expected_revision: 1,
        });
        tap(visual, "live-question-submit");
        wait(visual, |cx| {
            let view = view.read(cx);
            view.history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .runs
                .iter()
                .filter(|run| run.status == Status::Completed)
                .count()
                == 2
                && !view.questions.locked()
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/plan.txt")).unwrap(),
            "Reviewed coding output 中文 🙂"
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
