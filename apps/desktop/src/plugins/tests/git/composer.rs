use super::*;
use crate::conversation::live::tests::{contributions, fixture as chat};
use sailry_link::{Admission, CancellationToken, Pending, Subscription, Transport};
use sailry_protocol::{
    ErrorCode, Fault, NodeId, Topic, WorktreeId, conversation::Status, plugin::ui::Slot,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum DelayedKind {
    Submit,
    Git,
}

#[derive(Clone, Debug)]
enum MoveOutcome {
    Session(WorktreeId, u64),
    Fault(ErrorCode),
    Other,
}

#[derive(Clone, Debug)]
struct MoveTrace {
    target: WorktreeId,
    expected_revision: u64,
    outcome: Option<MoveOutcome>,
}

struct Delayed {
    inner: Arc<dyn Transport>,
    paused: AtomicBool,
    entered: Mutex<Vec<DelayedKind>>,
    release: CancellationToken,
    movement: Arc<Mutex<Option<MoveTrace>>>,
}

impl Delayed {
    fn new(inner: Arc<dyn Transport>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            paused: AtomicBool::new(false),
            entered: Mutex::default(),
            release: CancellationToken::new(),
            movement: Arc::new(Mutex::new(None)),
        })
    }

    fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    fn entered(&self, kind: DelayedKind) -> bool {
        self.entered.lock().unwrap().contains(&kind)
    }

    fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.release.cancel();
    }

    fn movement(&self) -> Option<MoveTrace> {
        self.movement.lock().unwrap().clone()
    }
}

impl Transport for Delayed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let movement = match &request.command {
                Command::MoveConversation {
                    worktree,
                    expected_revision,
                    ..
                } => Some((*worktree, *expected_revision)),
                _ => None,
            };
            if let Some((target, expected_revision)) = movement {
                *self.movement.lock().unwrap() = Some(MoveTrace {
                    target,
                    expected_revision,
                    outcome: None,
                });
            }
            let plugin_git = request
                .plugin
                .as_ref()
                .is_some_and(|scope| scope.package.name == "git")
                && matches!(
                    &request.command,
                    Command::InspectGit { .. }
                        | Command::ListGitBranches { .. }
                        | Command::ReadGitDiff { .. }
                );
            let kind = if matches!(&request.command, Command::SubmitTurn { .. }) {
                Some(DelayedKind::Submit)
            } else if plugin_git {
                Some(DelayedKind::Git)
            } else {
                None
            };
            let admission = match self.inner.dispatch(request).await {
                Ok(admission) => admission,
                Err(fault) => {
                    if movement.is_some()
                        && let Some(movement) = self.movement.lock().unwrap().as_mut()
                    {
                        movement.outcome = Some(MoveOutcome::Fault(fault.code));
                    }
                    return Err(fault);
                }
            };
            if movement.is_some() {
                let Admission {
                    receipt,
                    completion: pending,
                } = admission;
                let (sender, completion) = tokio::sync::oneshot::channel();
                let trace = self.movement.clone();
                tokio::spawn(async move {
                    let result = pending.await.expect("move command completion expected");
                    let outcome = match &result {
                        Ok(Output::Session(session)) => {
                            MoveOutcome::Session(session.worktree, session.revision)
                        }
                        Ok(_) => MoveOutcome::Other,
                        Err(fault) => MoveOutcome::Fault(fault.code),
                    };
                    if let Some(movement) = trace.lock().unwrap().as_mut() {
                        movement.outcome = Some(outcome);
                    }
                    let _ = sender.send(result);
                });
                return Ok(Admission {
                    receipt,
                    completion,
                });
            }
            let Some(kind) = kind.filter(|_| self.paused.load(Ordering::SeqCst)) else {
                return Ok(admission);
            };
            self.entered.lock().unwrap().push(kind);
            let Admission {
                receipt,
                completion: pending,
            } = admission;
            let (sender, completion) = tokio::sync::oneshot::channel();
            let release = self.release.clone();
            tokio::spawn(async move {
                let result = pending.await.expect("delayed command completion expected");
                release.cancelled().await;
                let _ = sender.send(result);
            });
            Ok(Admission {
                receipt,
                completion,
            })
        })
    }
}

fn install(fixture: &Fixture) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/git");
    fixture::copy_package(
        &source,
        &fixture.directory.path().join("project/git-package"),
    );
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "git-package".into(),
        name: "git".into(),
        expected_revision: package(fixture).summary.revision,
        expected_digest: None,
    }) else {
        panic!("Git package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    fixture: &Fixture,
    remote: bool,
    transport: Arc<Delayed>,
) -> (
    Entity<Shell>,
    Entity<crate::conversation::live::View>,
    &'a mut VisualTestContext,
) {
    let (shell, visual) = super::super::files::mount(fixture, remote, cx);
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            let live = shell.live.as_mut().unwrap();
            live.override_transport(transport);
            live.select(fixture.node.id(), cx);
        })
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| {
                snapshot.node == fixture.node.id()
                    && snapshot
                        .sessions
                        .iter()
                        .any(|session| session.id == fixture.session.id)
            })
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.reveal_session(fixture.session.clone(), window, cx)
        })
    });
    wait(visual, |cx| {
        shell.read(cx).current_chat().is_some_and(|view| {
            view.read(cx).connected() && view.read(cx).session() == Some(fixture.session.id)
        })
    });
    let view = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
    (shell, view, visual)
}

fn reads(fixture: &Fixture) -> Vec<Request> {
    fixture
        .transport
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| {
            request
                .plugin
                .as_ref()
                .is_some_and(|scope| scope.package.name == "git")
                && matches!(
                    request.command,
                    Command::InspectGit { .. }
                        | Command::ListGitBranches { .. }
                        | Command::ReadGitDiff { .. }
                )
        })
        .cloned()
        .collect()
}

fn ready(view: &Entity<crate::conversation::live::View>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        view.read(cx).connected()
            && contributions(view.read(cx)).read(cx).ready(cx)
            && view
                .read(cx)
                .plugin_panel("git", cx)
                .is_some_and(|panel| !panel.read(cx).loading)
    });
}

fn badge(view: &Entity<crate::conversation::live::View>, cx: &App) -> Option<Vec<String>> {
    contributions(view.read(cx))
        .read(cx)
        .entries(Slot::Status, cx)
        .into_iter()
        .find(|entry| entry.key.package.name == "git" && entry.key.id == "changes")
        .map(|entry| {
            entry
                .state
                .segments
                .into_iter()
                .map(|segment| segment.label.label)
                .collect()
        })
}

fn turn_finished(fixture: &Fixture) -> bool {
    let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
        session: fixture.session.id,
        before: None,
        limit: 100,
    }) else {
        panic!("conversation expected")
    };
    history.page.runs.len() == 1
        && history.page.runs.iter().all(|run| {
            !matches!(
                run.status,
                Status::Queued | Status::Running | Status::Stopping
            )
        })
}

fn wait_visible(visual: &mut VisualTestContext, selector: &'static str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        if visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(Instant::now() < deadline, "control deadline: {selector}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn skips_reads_without_a_repository(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::directory(remote);
        install(&fixture);
        let Output::Session(unassigned) = fixture.execute(Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(fixture.session.config.clone()),
        }) else {
            panic!("session expected")
        };
        for session in [fixture.session.clone(), unassigned] {
            let mut binding = fixture.binding.clone();
            binding.project = session.project;
            binding.worktree = Some(session.worktree);
            if session.project.is_none() {
                binding.project_name = "".into();
                binding.branch = "".into();
            }
            fixture.transport.requests.lock().unwrap().clear();
            let (view, visual) = chat::open(cx, binding, session);
            ready(&view, visual);
            assert!(reads(&fixture).is_empty());
            assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
            visual.update(|window, _| window.remove_window());
            drop(view);
        }
        fixture.close();
    }
}

#[gpui::test]
fn reads_after_repository_detection(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        prepare(&fixture);
        fixture.transport.requests.lock().unwrap().clear();
        let (view, visual) = chat::open(cx, fixture.binding.clone(), fixture.session.clone());
        ready(&view, visual);
        wait(visual, |_| !reads(&fixture).is_empty());
        assert!(
            reads(&fixture)
                .iter()
                .any(|request| matches!(request.command, Command::InspectGit { .. }))
        );
        assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn retains_badge_until_scope_changes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        prepare(&fixture);
        let Output::Plugin(worktrees) = fixture.execute(Command::ReadPlugin {
            name: "worktrees".into(),
        }) else {
            panic!("Worktrees package expected")
        };
        let worktrees = worktrees.summary.reference();
        let root = fixture.directory.path().join("project");
        let linked_path = fixture.directory.path().join("linked");
        git2::Repository::open(&root)
            .unwrap()
            .worktree("badge-scope", &linked_path, None)
            .unwrap();
        let Output::Worktree(linked) = fixture.execute(Command::RegisterWorktree {
            project: fixture.binding.project.unwrap(),
            path: linked_path.to_str().unwrap().into(),
        }) else {
            panic!("registered linked worktree expected")
        };
        let Output::GitWorktrees(found) = fixture.execute(Command::ListWorktrees {
            worktree: fixture.session.worktree,
        }) else {
            panic!("worktrees expected")
        };
        let entry = found
            .entries
            .iter()
            .position(|entry| entry.path == linked.path)
            .expect("linked worktree expected");
        let entry: &'static str = Box::leak(format!("entry-{entry}").into_boxed_str());
        let delayed = Delayed::new(fixture.transport.clone());
        let (shell, view, visual) = open(cx, &fixture, remote, delayed.clone());
        ready(&view, visual);
        assert_eq!(
            visual.read(|cx| view.read(cx).binding().client.target()),
            fixture.node.id()
        );
        assert_eq!(
            visual.read(|cx| view.read(cx).binding().worktree),
            Some(fixture.session.worktree)
        );
        wait(visual, |cx| {
            let registry = contributions(view.read(cx)).read(cx);
            registry
                .entries(Slot::Context, cx)
                .iter()
                .any(|entry| entry.key.id == "location" && entry.key.package == worktrees)
                && view
                    .read(cx)
                    .plugin_panel("worktrees", cx)
                    .is_some_and(|panel| panel.read(cx).selected.as_ref() == Some(&worktrees))
        });
        visual.update(|window, _| window.activate_window());
        visual.run_until_parked();
        visual.update(|window, cx| window.render_frame(cx));
        assert!(visual.update(|window, _| window.is_window_active()));
        wait(visual, |cx| badge(&view, cx).is_some());
        let known = visual.read(|cx| badge(&view, cx).unwrap());
        fixture.transport.requests.lock().unwrap().clear();
        delayed.pause();
        click(visual, "live-chat-input");
        visual.simulate_input("Keep the Git badge stable");
        click(visual, "live-chat-send");
        wait(visual, |_| {
            delayed.entered(DelayedKind::Submit) && delayed.entered(DelayedKind::Git)
        });
        assert_eq!(visual.read(|cx| badge(&view, cx)), Some(known.clone()));
        assert!(visual.debug_bounds("plugin-control-git-changes").is_some());
        delayed.resume();
        wait(visual, |cx| {
            reads(&fixture)
                .iter()
                .any(|request| matches!(request.command, Command::ReadGitDiff { .. }))
                && badge(&view, cx) == Some(known.clone())
                && turn_finished(&fixture)
                && view.read(cx).location_state()["can_move"].as_bool() == Some(true)
        });
        wait(visual, |cx| {
            view.read(cx)
                .can_invoke_contribution(sailry_protocol::plugin::ui::Intent::Worktrees, cx)
        });
        click(visual, "plugin-control-worktrees-location");
        wait_visible(visual, "worktrees-1");
        wait_visible(visual, entry);
        click(visual, entry);
        wait_visible(visual, "action-0");
        let expected_revision = visual.read(|cx| {
            view.read(cx).location_state()["revision"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
        });
        visual.update(|window, cx| window.click((SharedString::from("worktrees-1"), 0usize), cx));
        visual.run_until_parked();
        visual.update(|window, cx| window.render_frame(cx));
        wait(visual, |cx| {
            view.read(cx).binding().worktree == Some(linked.id)
                && delayed
                    .movement()
                    .is_some_and(|movement| movement.outcome.is_some())
        });
        let movement = delayed.movement().expect("move request expected");
        assert_eq!(movement.target, linked.id);
        assert_eq!(movement.expected_revision, expected_revision);
        let moved_revision = match movement.outcome {
            Some(MoveOutcome::Session(worktree, revision)) => {
                assert_eq!(worktree, linked.id);
                revision
            }
            outcome => panic!("successful move expected: {outcome:?}"),
        };
        assert_eq!(moved_revision, expected_revision + 1);
        assert_eq!(
            visual.read(|cx| view.read(cx).location_state()["revision"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()),
            moved_revision
        );
        assert!(shell.read_with(visual, |shell, _| {
            shell
                .current_chat()
                .is_some_and(|current| current.entity_id() == view.entity_id())
        }));
        wait(visual, |cx| badge(&view, cx).is_none());
        assert!(visual.debug_bounds("plugin-control-git-changes").is_none());
        visual.update(|window, _| window.remove_window());
        drop(view);
        drop(shell);
        fixture.close();
    }
}
