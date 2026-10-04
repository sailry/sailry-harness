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

fn painted_bounds(window: &Window, bounds: Bounds<Pixels>) -> Bounds<ScaledPixels> {
    Bounds::from_corners(
        window.pixel_snap_point(bounds.origin),
        window.pixel_snap_point(bounds.bottom_right()),
    )
    .scale(window.scale_factor())
}

fn surface(visual: &mut VisualTestContext, bounds: Bounds<Pixels>, background: Hsla) {
    visual.update(|window, cx| {
        let bounds = painted_bounds(window, bounds);
        let quads: Vec<_> = window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.bounds == bounds)
            .collect();
        // GPUI paints the fill and the border as separate native quads.
        assert!(
            quads.iter().any(|quad| {
                quad.border_widths == gpui_kit::Edges::all(ScaledPixels(window.scale_factor()))
                    && quad.border_color == cx.theme().border
            }),
            "native surface border {bounds:?}"
        );
        let fills: Vec<_> = quads
            .iter()
            .filter(|quad| {
                quad.border_widths == gpui_kit::Edges::default()
                    && !quad.background.is_transparent()
            })
            .collect();
        if background.a > 0. {
            assert!(
                fills
                    .iter()
                    .any(|quad| quad.background == background.into()),
                "native surface fill {bounds:?}"
            );
        } else {
            assert!(fills.is_empty(), "transparent native surface {bounds:?}");
        }
    });
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
        let project: &'static str = Box::leak(
            format!(
                "activity-project-activity_idle-{}",
                fixture.session.project.unwrap()
            )
            .into_boxed_str(),
        );
        wait(visual, |cx| snapshot(&panel, cx).contains(project));
        click(visual, project);
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
        let viewport: &'static str = Box::leak(format!("{project}-viewport").into_boxed_str());
        assert!(
            visual.debug_bounds(viewport).is_none(),
            "cached disclosures survive leaving the page"
        );
        held.release.cancel();
        wait(visual, |cx| snapshot(&panel, cx).contains("Fresh preview"));
        click(visual, project);
        assert!(visual.debug_bounds(viewport).is_some());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn cards_fit_short_and_clamped_previews(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let mut sessions = vec![fixture.session.clone()];
        for _ in 0..2 {
            let Output::Session(session) = fixture.execute(Command::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(fixture.session.config.clone()),
            }) else {
                panic!("session expected");
            };
            sessions.push(session);
        }
        let release = CancellationToken::new();
        release.cancel();
        let held = Arc::new(Held {
            inner: fixture.transport.clone(),
            reads: AtomicUsize::new(0),
            cancelled: AtomicUsize::new(0),
            release,
            text: [
                (sessions[0].id, "Short preview".into()),
                (sessions[1].id, "Wrapped preview ".repeat(24)),
                (
                    sessions[2].id,
                    "First line\nSecond line\nThird line\nFourth line".into(),
                ),
            ]
            .into(),
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
        wait(visual, |cx| {
            let view = snapshot(&panel, cx);
            view.contains("Short preview")
                && view.contains("Wrapped preview")
                && view.contains("Third line")
        });
        let handle = visual.update(|window, _| window.window_handle());
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            visual.update(|window, cx| Theme::change(mode, Some(window), cx));
            for width in [1440., 900.] {
                visual.simulate_window_resize(handle, size(px(width), px(1000.)));
                visual.update(|window, cx| window.draw(cx).clear(cx));
                for lane in [
                    "activity_board_waiting",
                    "activity_board_running",
                    "activity_completed",
                    "activity_idle",
                ] {
                    let selector = Box::leak(format!("{lane}-heading").into_boxed_str());
                    let bounds = visual.debug_bounds(selector).unwrap();
                    let background = visual.update(|_, cx| cx.theme().group_box);
                    // The absolute probe measures the padding box inside the border.
                    surface(visual, bounds.dilate(px(1.)), background);
                }
                let rem = visual.update(|window, _| window.rem_size());
                let mut heights = Vec::new();
                for (index, session) in sessions.iter().enumerate() {
                    let part = |suffix: &str| {
                        Box::leak(format!("activity-{}{suffix}", session.id).into_boxed_str())
                            as &'static str
                    };
                    let card = visual.debug_bounds(part("")).unwrap();
                    let header = visual.debug_bounds(part("-header")).unwrap();
                    let title = visual.debug_bounds(part("-title")).unwrap();
                    let message = visual.debug_bounds(part("-content")).unwrap();
                    let footer = visual.debug_bounds(part("-location")).unwrap();
                    let project = visual.debug_bounds(part("-project")).unwrap();
                    let border = px(1.);
                    assert_eq!(title.size.height, rem * 1.25);
                    assert_eq!(
                        message.size.height,
                        rem * if index == 0 { 1.25 } else { 2.5 }
                    );
                    assert_eq!(
                        card.size.height,
                        title.size.height + message.size.height + rem * 2. + border * 2.
                    );
                    assert_eq!(header.left(), card.left() + rem * 0.75 + border);
                    assert_eq!(header.right(), card.right() - rem * 0.75 - border);
                    assert_eq!(title.left(), header.left());
                    assert_eq!(message.right(), header.right());
                    assert!((project.right() - header.right()).abs() < px(1.));
                    let background = visual.update(|_, cx| {
                        if mode.is_dark() {
                            cx.theme().muted
                        } else {
                            cx.theme().group_box
                        }
                    });
                    assert_eq!(background.a == 0., !mode.is_dark());
                    surface(visual, card, background);
                    assert!(title.bottom() < message.top());
                    assert_eq!(footer.top(), title.top() + rem * 0.125);
                    assert!(footer.left() > title.right());
                    assert!(card.contains(&footer.center()));
                    heights.push(card.size.height);
                }
                assert_eq!(heights[1] - heights[0], rem * 1.25);
                assert_eq!(heights[2], heights[1]);
                let project = format!(
                    "activity-project-activity_idle-{}",
                    fixture.session.project.unwrap()
                );
                let header: &'static str = Box::leak(project.clone().into_boxed_str());
                let viewport: &'static str =
                    Box::leak(format!("{project}-viewport").into_boxed_str());
                let list = visual.debug_bounds(viewport).unwrap();
                assert_eq!(
                    list.size.height,
                    heights.iter().copied().sum::<Pixels>() + rem * 1.5
                );
                assert!(visual.debug_bounds(header).unwrap().bottom() < list.top());
            }
        }
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
