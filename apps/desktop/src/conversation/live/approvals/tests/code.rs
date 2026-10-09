use super::*;
use sailry_client::conversation::tools::State as CallState;
use sailry_protocol::{
    Permission, WorkMode,
    conversation::{ApprovalSource, question},
};
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};

fn fixture(remote: bool, mode: WorkMode, calls: usize) -> Fixture {
    let tools = (0..calls)
        .map(|_| {
            (
                crate::agent_fixture::plugin_tool("code", "code_hover"),
                json!({"path":"main.rs", "line":1, "symbol":"symbol"}),
            )
        })
        .collect();
    let mut fixture = Fixture::with_environment(
        remote,
        |directory| {
            let root = directory.join("project");
            let bin = directory.canonicalize().unwrap().join("bin");
            std::fs::create_dir(&bin).unwrap();
            std::fs::write(
                root.join("Cargo.toml"),
                "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\n",
            )
            .unwrap();
            std::fs::write(root.join("main.rs"), "🙂 broken symbol\n").unwrap();
            let python = std::process::Command::new("python3")
                .args(["-c", "import sys; print(sys.executable)"])
                .output()
                .unwrap();
            assert!(python.status.success());
            let python = String::from_utf8(python.stdout).unwrap();
            let server = bin.join("rust-analyzer");
            std::fs::write(
                &server,
                format!(
                    "#!{}\n{}",
                    python.trim(),
                    include_str!(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../crates/node-runtime/src/code/tests/server.py"
                    ))
                ),
            )
            .unwrap();
            std::fs::set_permissions(server, std::fs::Permissions::from_mode(0o700)).unwrap();
            Some(sailry_node_runtime::EnvironmentFixture {
                home: directory.into(),
                shell: "/bin/false".into(),
                environment: BTreeMap::from([("PATH".into(), bin.into_os_string())]),
                timeout: Duration::from_secs(1),
            })
        },
        |runtime| runtime.block_on(crate::agent_fixture::Server::tools(tools)),
    );
    let mut config = fixture.session.config.clone();
    config.mode = mode;
    config.permission = Permission::Ask;
    let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        config,
    }) else {
        panic!("session expected")
    };
    fixture.session = session;
    fixture
}

fn start(fixture: &Fixture) {
    fixture.execute(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        message: "Inspect the language service".into(),
    });
}

#[gpui::test]
fn starts_once_from_captured_metadata(cx: &mut TestAppContext) {
    init(cx);
    for mode in [WorkMode::Code, WorkMode::Plan] {
        for remote in [false, true] {
            let fixture = fixture(remote, mode, 2);
            start(&fixture);
            let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| {
                view.read(cx).connected() && pending(&view, cx).is_some()
            });
            let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
            view.read_with(visual, |view, _| {
                let approval = view.history.calls[0].approval.as_ref().unwrap();
                assert_eq!(approval.source, ApprovalSource::User);
                assert_eq!(approval.startup.as_ref().unwrap().server, "rust-analyzer");
                assert_eq!(
                    approval.startup.as_ref().unwrap().worktree,
                    fixture.session.worktree
                );
                assert!(!approval.can_remember);
                assert!(approval.access.is_none());
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("live-approval-startup-{id}").into_boxed_str()
                    ))
                    .is_some()
            );
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("live-approval-approve-options-{id}").into_boxed_str()
                    ))
                    .is_none()
            );
            assert!(!fixture.directory.path().join("project/pid").exists());
            tap(visual, &format!("live-approval-approve-{id}"));
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                wait(visual, |_| true);
                let ready = view.read_with(visual, |view, _| {
                    let calls = &view.history.calls;
                    let returned = calls.len() >= 2
                        && calls[..2].iter().all(|call| {
                            call.name == crate::agent_fixture::plugin_tool("code", "code_hover")
                                && call.state == CallState::Returned
                        });
                    returned
                        && match mode {
                            WorkMode::Code => calls.len() == 2,
                            WorkMode::Plan => {
                                calls.len() == 3
                                    && calls[2].name == "plan_review"
                                    && calls[2].state == CallState::Waiting
                                    && calls[2].question.as_ref().is_some_and(|question| {
                                        question.state == question::State::Pending
                                    })
                            }
                        }
                });
                if ready {
                    break;
                }
                view.read_with(visual, |view, _| {
                    assert!(std::time::Instant::now() < deadline,
                        "startup result deadline: remote={remote}, mode={mode:?}, calls={:?}, page={:?}",
                        view.history.calls,
                        view.history.snapshot.as_ref().map(|snapshot| &snapshot.page));
                });
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(fixture.directory.path().join("project/pid").exists());
            view.read_with(visual, |view, _| {
                let page = &view.history.snapshot.as_ref().unwrap().page;
                assert_eq!(
                    page.approvals
                        .iter()
                        .filter(|approval| approval.startup.is_some()
                            && approval.source == ApprovalSource::User)
                        .count(),
                    1
                );
                assert_eq!(page.session, fixture.session.id);
                let calls = &view.history.calls;
                let expected = if mode == WorkMode::Plan { 3 } else { 2 };
                assert_eq!(calls.len(), expected);
                for call in &calls[..2] {
                    assert_eq!(
                        call.name,
                        crate::agent_fixture::plugin_tool("code", "code_hover")
                    );
                    assert_eq!(call.state, CallState::Returned);
                    assert!(
                        call.result(page)
                            .unwrap()
                            .to_string()
                            .contains("fixture signature")
                    );
                }
                if mode == WorkMode::Plan {
                    let review = &calls[2];
                    assert_eq!(review.name, "plan_review");
                    assert_eq!(review.state, CallState::Waiting);
                    assert!(review.response.is_none());
                    assert!(review.result(page).is_none());
                    let spec = review.question_spec(page).unwrap();
                    assert!(spec.prompt.is_empty());
                    assert_eq!(spec.input, question::Input::Plan);
                    let question = review.question.as_ref().unwrap();
                    assert_eq!(question.session, fixture.session.id);
                    assert_eq!(question.turn, review.turn);
                    assert_eq!(question.turn, calls[0].turn);
                    assert_eq!(question.state, question::State::Pending);
                    assert_eq!(page.questions, vec![question.clone()]);
                } else {
                    assert!(page.questions.is_empty());
                }
            });
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}

#[gpui::test]
fn decline_keeps_the_server_stopped(cx: &mut TestAppContext) {
    init(cx);
    for mode in [WorkMode::Code, WorkMode::Plan] {
        for remote in [false, true] {
            let fixture = fixture(remote, mode, 1);
            start(&fixture);
            let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| {
                view.read(cx).connected() && pending(&view, cx).is_some()
            });
            let id = view.read_with(visual, |_, cx| pending(&view, cx).unwrap());
            choices::reason(visual, id, "Keep this workspace process-free");
            tap(visual, &format!("live-approval-reason-submit-{id}"));
            wait(visual, |cx| {
                view.read(cx).history.calls[0].state
                    == sailry_client::conversation::tools::State::Returned
            });
            assert!(!fixture.directory.path().join("project/pid").exists());
            view.read_with(visual, |view, _| {
                let approval = view.history.calls[0].approval.as_ref().unwrap();
                assert_eq!(approval.state, ApprovalState::Denied);
                assert_eq!(
                    approval.reason.as_deref(),
                    Some("Keep this workspace process-free")
                );
                assert!(
                    view.history.calls[0]
                        .result(&view.history.snapshot.as_ref().unwrap().page)
                        .unwrap()
                        .to_string()
                        .contains("Keep this workspace process-free")
                );
            });
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}
