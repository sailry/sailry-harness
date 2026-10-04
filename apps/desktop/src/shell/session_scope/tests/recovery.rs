use super::*;
use sailry_link::Transport;
use sailry_protocol::{ErrorCode, Fault, NodeId, Request, Topic};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

mod offline;

#[gpui::test]
fn invalid_layout_at_startup(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let invalid = serde_json::json!({"version": 1, "groups": "invalid"});
    cx.update(|cx| {
        crate::preferences::update(cx, |preferences| {
            preferences.workspaces = Some(invalid.clone());
        });
    });

    let (shell, visual) = mount(cx, &fixture);
    crate::feedback::tests::shown(visual);
    visual.update(|window, cx| {
        assert_eq!(window.notifications(cx).len(), 1);
        assert_eq!(
            crate::feedback::tests::summary(window, cx),
            crate::tr("pane_restore_failed")
        );
        let splits = shell.read(cx).splits.clone();
        splits.update(cx, |splits, cx| splits.persist(cx));
    });
    visual.executor().advance_clock(Duration::from_millis(400));
    visual.run_until_parked();
    assert_eq!(
        visual.update(|_, cx| crate::preferences::data(cx).workspaces),
        Some(invalid)
    );
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

struct BusyReads {
    inner: Arc<dyn Transport>,
    remaining: AtomicUsize,
    rejected: AtomicUsize,
    snapshot_busy: AtomicBool,
}

impl Transport for BusyReads {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(
        &self,
        topic: Topic,
    ) -> sailry_link::Pending<'_, Result<Box<dyn sailry_link::Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(
        &self,
        request: Request,
    ) -> sailry_link::Pending<'_, Result<sailry_link::Admission, Fault>> {
        Box::pin(async move {
            if matches!(request.command, Command::Snapshot)
                && self.snapshot_busy.load(Ordering::SeqCst)
            {
                return Err(Fault::new(ErrorCode::Busy, "injected snapshot contention"));
            }
            if matches!(request.command, Command::InspectGit { .. })
                && self
                    .remaining
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                        count.checked_sub(1)
                    })
                    .is_ok()
            {
                self.rejected.fetch_add(1, Ordering::SeqCst);
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "injected startup Git contention",
                ));
            }
            self.inner.dispatch(request).await
        })
    }
}

#[gpui::test]
fn restores_pane_git_state(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let mut services = fixture.services();
    let transport = Arc::new(BusyReads {
        inner: services.local.clone(),
        remaining: AtomicUsize::new(0),
        rejected: AtomicUsize::new(0),
        snapshot_busy: AtomicBool::new(false),
    });
    services.local = transport.clone();
    let (shell, visual) = mount_with_services(cx, services);
    for index in 0..2 {
        let client = Client::new(fixture.nodes[index].local());
        let Output::Snapshot(snapshot) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected");
        };
        let session = &fixture.sessions[index];
        let root = &snapshot
            .worktrees
            .iter()
            .find(|tree| tree.id == session.worktree)
            .unwrap()
            .path;
        git2::Repository::init(root)
            .unwrap()
            .set_head("refs/heads/main")
            .unwrap();
        std::fs::write(std::path::Path::new(root).join("change.txt"), "one\ntwo\n").unwrap();
        open(&shell, visual, &fixture, index, session);
        let original =
            shell.read_with(visual, |shell, _| shell.current_chat().unwrap().entity_id());
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let saved = shell.splits.read(cx).saved(cx);
                shell.chats.views.clear();
                crate::preferences::update(cx, |preferences| {
                    preferences.workspaces = Some(serde_json::to_value(&saved).unwrap());
                });
                if index == 0 {
                    transport.remaining.store(3, Ordering::SeqCst);
                }
                shell.restore_split_workspaces(window, cx);
            });
        });
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.entity_id() != original
                    && chat.read(cx).connected()
                    && chat.read(cx).binding().branch == "main"
            })
        });
        // Rendering waits for real local/Link reads, without another focus or selection.
        let deadline = Instant::now() + Duration::from_secs(10);
        while visual.debug_bounds("plugin-control-git-changes").is_none() {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(
                Instant::now() < deadline,
                "restored Git summary did not appear"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(visual.debug_bounds("plugin-control-git-branch").is_some());
    }
    assert_eq!(transport.rejected.load(Ordering::SeqCst), 3);
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

#[gpui::test]
fn missing_session_fallback(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    for index in 0..2 {
        let (shell, visual) = mount(cx, &fixture);
        let session = &fixture.sessions[index];
        open(&shell, visual, &fixture, index, session);
        let target = crate::panes::Target::Session(fixture.nodes[index].id(), session.id);
        let saved = shell.read_with(visual, |shell, cx| shell.splits.read(cx).saved(cx));
        assert!(saved.targets().contains(&target));
        visual.update(|window, _| window.remove_window());
        drop(shell);
        let client = Client::new(fixture.nodes[index].local());
        fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::RemoveSession {
                session: session.id,
                expected_revision: session.revision,
            })))
            .unwrap();
        cx.update(|cx| {
            crate::preferences::update(cx, |preferences| {
                preferences.workspaces = Some(serde_json::to_value(&saved).unwrap());
            });
        });
        let (shell, visual) = mount(cx, &fixture);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            visual.run_until_parked();
            let state = visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                let shell = shell.read(cx);
                (
                    shell.splits.read(cx).contains(target),
                    shell.session_scope.active,
                    shell
                        .current_chat()
                        .map(|chat| (chat.read(cx).session(), chat.read(cx).connected())),
                )
            });
            if !state.0 && state.2 == Some((None, true)) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "restored missing session {index}: {state:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.session_scope.active),
            Key::Draft
        );
        assert!(visual.debug_bounds("live-chat-input").is_some());
        crate::feedback::tests::shown(visual);
        assert_eq!(
            visual.update(|window, cx| window.notifications(cx).len()),
            1
        );
        visual.run_until_parked();
        visual.executor().advance_clock(Duration::from_millis(400));
        wait(visual, |cx| {
            crate::preferences::data(cx)
                .workspaces
                .and_then(|value| serde_json::from_value::<crate::panes::Saved>(value).ok())
                .is_some_and(|saved| !saved.targets().contains(&target))
        });
        visual.update(|window, _| window.remove_window());
        drop(shell);
    }
    fixture.close();
}

#[gpui::test]
fn failed_reads_keep_sessions(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let mut services = fixture.services();
    let transport = Arc::new(BusyReads {
        inner: services.local.clone(),
        remaining: AtomicUsize::new(0),
        rejected: AtomicUsize::new(0),
        snapshot_busy: AtomicBool::new(false),
    });
    services.local = transport.clone();
    let (shell, visual) = mount_with_services(cx, services);
    open(&shell, visual, &fixture, 0, &fixture.sessions[0]);
    let target = crate::panes::Target::Session(fixture.nodes[0].id(), fixture.sessions[0].id);
    let pane = shell.read_with(visual, |shell, cx| {
        shell.splits.read(cx).pane(target).unwrap().entity_id()
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let saved = shell.splits.read(cx).saved(cx);
            shell.chats.views.clear();
            crate::preferences::update(cx, |preferences| {
                preferences.workspaces = Some(serde_json::to_value(&saved).unwrap());
            });
            transport.snapshot_busy.store(true, Ordering::SeqCst);
            shell.restore_split_workspaces(window, cx);
        });
    });
    let retry = Box::leak(format!("pane-restore-retry-{target:?}").into_boxed_str());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.debug_bounds(&*retry).is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "restore failure state deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    shell.read_with(visual, |shell, cx| {
        assert_eq!(
            shell.splits.read(cx).pane(target).unwrap().entity_id(),
            pane
        );
        assert_eq!(shell.splits.read(cx).active, Some(target));
        assert_eq!(
            shell.session_scope.active,
            Key::Session(fixture.nodes[0].id(), fixture.sessions[0].id)
        );
        assert!(shell.chats.views.is_empty());
    });
    transport.snapshot_busy.store(false, Ordering::SeqCst);
    click(&shell, visual, retry.to_string());
    wait(visual, |cx| {
        shell
            .read(cx)
            .current_chat()
            .is_some_and(|chat| chat.read(cx).connected())
    });
    assert_eq!(
        shell.read_with(visual, |shell, cx| {
            shell.splits.read(cx).pane(target).unwrap().entity_id()
        }),
        pane
    );
    assert!(visual.debug_bounds("live-chat-input").is_some());
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
