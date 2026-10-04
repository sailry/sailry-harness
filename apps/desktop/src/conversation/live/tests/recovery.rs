use super::*;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{ErrorCode, Fault, Topic};
use std::sync::atomic::{AtomicBool, Ordering};

struct Failing {
    inner: Arc<dyn Transport>,
    fail: AtomicBool,
    release: CancellationToken,
    node: bool,
}

impl Transport for Failing {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        self.inner.dispatch(request)
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            if matches!(topic, Topic::Conversation(_)) || (self.node && topic == Topic::Node) {
                self.release.cancelled().await;
                if self.fail.load(Ordering::SeqCst) {
                    return Err(Fault::new(
                        ErrorCode::InvalidRequest,
                        "injected snapshot failure",
                    ));
                }
            }
            self.inner.subscribe(topic).await
        })
    }
}

#[gpui::test]
fn retries_failed_restore(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        fixture.start();
        let (original, visual) =
            fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            original
                .read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|s| s.page.runs.iter().any(|r| r.status == Status::Completed))
        });
        let turns = original.read_with(visual, |view, _| view.rows.clone());
        visual.update(|window, _| window.remove_window());
        let transport = Arc::new(Failing {
            inner: fixture.transport.clone(),
            fail: AtomicBool::new(true),
            release: CancellationToken::new(),
            node: false,
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(transport.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).node.connected);
        assert!(visual.debug_bounds("empty-chat_history_loading").is_some());
        assert!(visual.debug_bounds("live-chat-input").is_none());
        transport.release.cancel();
        wait(visual, |cx| view.read(cx).history.error.is_some());
        crate::feedback::tests::settle(visual);
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            for width in [420., 1280.] {
                let handle = visual.update(|window, cx| {
                    Theme::change(mode, Some(window), cx);
                    window.window_handle()
                });
                visual.simulate_window_resize(handle, size(px(width), px(780.)));
                visual.update(|window, cx| {
                    let _ = window.draw(cx);
                });
                let panel = visual.debug_bounds("chat-unavailable").unwrap();
                assert!(visual.debug_bounds("empty-chat_sync_failed").is_none());
                assert!(visual.debug_bounds("chat-sync-retry").is_some());
                assert_eq!(panel.size, size(px(width), px(780.)));
                assert!(!visual.update(|window, cx| window.notifications(cx).is_empty()));
                assert!(visual.debug_bounds("chat-connection-status").is_none());
                assert!(visual.debug_bounds("chat-connection-reason").is_none());
                assert!(visual.debug_bounds("empty-chat_history_loading").is_some());
                assert!(visual.debug_bounds("live-history-viewport").is_none());
                assert!(visual.debug_bounds("live-chat-input").is_none());
                assert!(visual.debug_bounds("chat_sync_failed").is_none());
            }
        }
        transport.fail.store(false, Ordering::SeqCst);
        fixture::tap(visual, "chat-sync-retry");
        wait(visual, |cx| view.read(cx).connected());
        assert_eq!(view.read_with(visual, |view, _| view.rows.clone()), turns);
        assert!(visual.debug_bounds("chat-unavailable").is_none());
        assert!(visual.debug_bounds("chat-connection-status").is_none());
        assert!(visual.debug_bounds("live-chat-input").is_some());
        assert_eq!(
            fixture.task_requests(),
            1,
            "recovery must not replay the Agent turn"
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn initial_connection_is_not_reconnecting(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let transport = Arc::new(Failing {
            inner: fixture.transport.clone(),
            fail: AtomicBool::new(true),
            release: CancellationToken::new(),
            node: true,
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(transport.clone()));
        let (view, visual) = fixture::open_session(cx, binding, None);
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(!view.read_with(visual, |view, _| view.connected()));
        assert!(visual.debug_bounds("chat-connection-status").is_none());
        assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
        assert!(visual.debug_bounds("live-chat-input").is_some());
        let before = visual.debug_bounds("live-history-viewport").unwrap();
        transport.release.cancel();
        wait(visual, |cx| view.read(cx).node.error.is_some());
        crate::feedback::tests::settle(visual);
        assert!(visual.debug_bounds("error-toast-detail").is_some());
        assert!(visual.debug_bounds("chat-sync-retry").is_some());
        assert!(visual.debug_bounds("chat-connection-status").is_none());
        assert_eq!(
            visual.debug_bounds("live-history-viewport").unwrap(),
            before
        );
        for _ in 0..3 {
            view.update(visual, |_, cx| cx.notify());
            visual.run_until_parked();
        }
        assert_eq!(
            visual.update(|window, cx| window.notifications(cx).len()),
            1
        );
        transport.fail.store(false, Ordering::SeqCst);
        fixture::tap(visual, "chat-sync-retry");
        wait(visual, |cx| view.read(cx).connected());
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn restores_calls_after_restart(cx: &mut TestAppContext) {
    use sailry_protocol::conversation::ApprovalState;
    fixture::init(cx);
    for remote in [false, true] {
        for interrupt in [false, true] {
            let mut fixture = fixture::Fixture::with_server(remote, |runtime| {
                runtime.block_on(support::Server::phases_with_tool(&support::plugin_tool("files", "write_file"), serde_json::json!({
                    "path": "restore.txt", "text": "Persisted fixture", "expected_revision": null
                })))
            });
            let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
                panic!("snapshot expected")
            };
            let mut provider = snapshot
                .providers
                .into_iter()
                .find(|p| p.id == fixture.session.config.provider)
                .unwrap();
            provider.api = ModelApi::Responses;
            fixture.execute(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            });
            let (view, visual) =
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
            fixture.start();
            wait(visual, |cx| {
                view.read(cx).history.snapshot.as_ref().is_some_and(|s| {
                    s.page
                        .approvals
                        .iter()
                        .any(|a| a.state == ApprovalState::Pending)
                })
            });
            let approval = view.read_with(visual, |view, _| {
                view.history.snapshot.as_ref().unwrap().page.approvals[0].clone()
            });
            assert!(approval.index > 0, "commentary precedes the approved call");
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("live-approval-approve-{}", approval.id).into_boxed_str()
                    ))
                    .is_some()
            );
            if !interrupt {
                wait(visual, |cx| view.read(cx).connected());
                fixture::tap(visual, &format!("live-approval-approve-{}", approval.id));
                wait(visual, |cx| {
                    view.read(cx)
                        .history
                        .snapshot
                        .as_ref()
                        .is_some_and(|s| s.page.runs[0].status == Status::Completed)
                });
            }
            visual.update(|window, _| window.remove_window());
            drop(view);
            fixture.runtime.block_on(fixture.node.shutdown()).unwrap();
            fixture.node = fixture
                .runtime
                .block_on(Node::start(fixture.directory.path().join("node")))
                .unwrap();
            let address = fixture
                .runtime
                .block_on(
                    fixture
                        .controller
                        .handle()
                        .pair(fixture.node.link().invite().unwrap().ticket()),
                )
                .unwrap();
            fixture.transport = if remote {
                fixture.controller.handle().remote(address)
            } else {
                fixture.node.local()
            };
            fixture.binding.client = Arc::new(Client::new(fixture.transport.clone()));
            fixture.binding.defaults = fixture.binding.client.clone();
            let (restored, visual) =
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| restored.read(cx).connected());
            restored.read_with(visual, |view, _| {
                let page = &view.history.snapshot.as_ref().unwrap().page;
                assert_eq!(page.runs[0].status, if interrupt { Status::Interrupted } else { Status::Completed });
                let actual = &page.approvals[0];
                assert_eq!(actual.id, approval.id);
                assert_eq!(actual.index, approval.index);
                assert_eq!(actual.state, if interrupt { ApprovalState::Interrupted } else { ApprovalState::Approved });
                let entry = page.entries.iter().find(|entry| entry.id == actual.entry).unwrap();
                assert!(matches!(&entry.parts[actual.index], Part::ToolCall { name, .. } if name == &support::plugin_tool("files", "write_file")));
                assert!(entry.parts.iter().any(|p| matches!(p, Part::Resource(v) if v["type"] == "provider_context")));
                let call = view.history.calls.iter().find(|call| call.approval.as_ref().is_some_and(|a| a.id == approval.id)).unwrap();
                assert_eq!(call.state, if interrupt { sailry_client::conversation::tools::State::NotExecuted } else { sailry_client::conversation::tools::State::Returned });
            });
            if interrupt {
                let turn = approval.turn;
                restored.read_with(visual, |view, _| {
                    let error = view.history.snapshot.as_ref().unwrap().page.runs[0]
                        .error
                        .as_ref()
                        .expect("interrupted run retains its reason");
                    assert!(!error.message.is_empty());
                });
                assert!(
                    visual
                        .debug_bounds(Box::leak(
                            format!("live-turn-error-{turn}").into_boxed_str()
                        ))
                        .is_some()
                );
                assert!(
                    visual
                        .debug_bounds(Box::leak(
                            format!("live-turn-error-reason-{turn}").into_boxed_str()
                        ))
                        .is_none()
                );
                fixture::tap(visual, &format!("live-turn-error-{turn}"));
                assert!(
                    visual
                        .debug_bounds(Box::leak(
                            format!("live-turn-error-reason-{turn}").into_boxed_str()
                        ))
                        .is_some()
                );
            }
            assert!(visual.debug_bounds("chat-unavailable").is_none());
            assert!(visual.debug_bounds("live-chat-input").is_some());
            assert_eq!(fixture.task_requests(), if interrupt { 1 } else { 2 });
            assert_eq!(
                fixture
                    .directory
                    .path()
                    .join("project/restore.txt")
                    .exists(),
                !interrupt
            );
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}
