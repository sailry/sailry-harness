use super::*;
use sailry_link::{Admission, CancellationToken, Pending, Subscription, Transport};
use sailry_protocol::{Fault, NodeId, Request, SessionId, Topic};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::AtomicUsize},
};

struct Held {
    inner: Arc<dyn Transport>,
    reads: AtomicUsize,
    cancelled: AtomicUsize,
    release: CancellationToken,
    text: BTreeMap<SessionId, String>,
}

struct Reading<'a> {
    cancelled: &'a AtomicUsize,
    completed: bool,
}

impl Drop for Reading<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.cancelled.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Transport for Held {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let preview = matches!(request.command, Command::ReadActivity { .. });
            if preview {
                self.reads.fetch_add(1, Ordering::SeqCst);
                let mut reading = Reading {
                    cancelled: &self.cancelled,
                    completed: false,
                };
                self.release.cancelled().await;
                reading.completed = true;
            }
            let mut admission = self.inner.dispatch(request).await?;
            if preview {
                let mut result = admission.completion.await.unwrap();
                if let Ok(Output::Activity(items)) = &mut result {
                    // The real authorized response supplies identity; this fixture supplies visible text.
                    for item in items {
                        item.text = Some(
                            self.text
                                .get(&item.session)
                                .cloned()
                                .unwrap_or_else(|| "Fresh preview".into()),
                        );
                    }
                }
                let (sender, completion) = tokio::sync::oneshot::channel();
                sender.send(result).unwrap();
                admission.completion = completion;
            }
            Ok(admission)
        })
    }
}

#[gpui::test]
fn cancels_on_hide_and_resumes_cached_page(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let (shell, visual) = files::mount(&fixture, remote, cx);
        let held = Arc::new(Held {
            inner: fixture.transport.clone(),
            reads: AtomicUsize::new(0),
            cancelled: AtomicUsize::new(0),
            release: CancellationToken::new(),
            text: BTreeMap::new(),
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, _| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .override_transport(held.clone());
            })
        });
        assert_eq!(held.reads.load(Ordering::SeqCst), 0);
        let panel = open(&shell, &fixture, visual);
        // Navigation was already initialized before the fixture transport changed.
        // Reopen the same package with the observed client so its native Host uses it.
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let package = panel.selected.clone().unwrap();
                panel.back(cx);
                panel.binding.client = Arc::new(sailry_client::Client::new(held.clone()));
                panel.open(package, window, cx);
            })
        });
        wait(visual, |_| held.reads.load(Ordering::SeqCst) > 0);
        wait(visual, |cx| snapshot(&panel, cx).contains("activity-world"));
        click(visual, "activity-zoom-in");
        let world = visual.debug_bounds("activity-world").unwrap().size;
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(Page::Settings, window, cx);
            })
        });
        wait(visual, |_| {
            held.cancelled.load(Ordering::SeqCst) == held.reads.load(Ordering::SeqCst)
        });
        let hidden_reads = held.reads.load(Ordering::SeqCst);
        fixture.execute(Command::CreateSession {
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            config: Some(fixture.session.config.clone()),
        });
        visual.run_until_parked();
        assert_eq!(held.reads.load(Ordering::SeqCst), hidden_reads);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let entry = shell
                    .extension_entries(cx)
                    .into_iter()
                    .find(|entry| entry.package.name == "progress")
                    .unwrap();
                shell.open_extension(entry, window, cx);
            })
        });
        wait(visual, |_| held.reads.load(Ordering::SeqCst) > hidden_reads);
        assert_eq!(
            shell.read_with(visual, |shell, _| shell
                .extensions
                .as_ref()
                .unwrap()
                .panel
                .as_ref()
                .unwrap()
                .clone()),
            panel
        );
        assert_eq!(
            visual.debug_bounds("activity-world").unwrap().size,
            world,
            "cached zoom survives leaving the page"
        );
        held.release.cancel();
        click(
            visual,
            selector(format!(
                "activity-room-{}",
                fixture.session.project.unwrap()
            )),
        );
        wait(visual, |cx| snapshot(&panel, cx).contains("Fresh preview"));
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn room_lists_use_current_previews(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let release = CancellationToken::new();
        release.cancel();
        let held = Arc::new(Held {
            inner: fixture.transport.clone(),
            reads: AtomicUsize::new(0),
            cancelled: AtomicUsize::new(0),
            release,
            text: [(fixture.session.id, "## Fresh preview\n**Verified**".into())].into(),
        });
        let (shell, visual) = files::mount(&fixture, remote, cx);
        let panel = open(&shell, &fixture, visual);
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                let package = panel.selected.clone().unwrap();
                panel.back(cx);
                panel.binding.client = Arc::new(sailry_client::Client::new(held.clone()));
                panel.open(package, window, cx);
            })
        });
        wait(visual, |cx| snapshot(&panel, cx).contains("activity-world"));
        click(
            visual,
            selector(format!(
                "activity-room-{}",
                fixture.session.project.unwrap()
            )),
        );
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Fresh preview Verified")
        });
        click(visual, "activity-dialog-close");
        let handle = visual.update(|window, _| window.window_handle());
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            visual.update(|window, cx| Theme::change(mode, Some(window), cx));
            for width in [1440., 900.] {
                visual.simulate_window_resize(handle, size(px(width), px(1000.)));
                visual.update(|window, cx| window.draw(cx).clear(cx));
                assert!(visual.debug_bounds("activity-world").is_some());
                assert!(visual.debug_bounds("activity-plaza").is_some());
                assert!(
                    visual
                        .debug_bounds(selector(format!(
                            "activity-room-{}",
                            fixture.session.project.unwrap()
                        )))
                        .is_some()
                );
            }
        }
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
