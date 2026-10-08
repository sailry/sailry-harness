use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init, tap},
    wait,
};
use core::prelude::v1::test;
use std::time::{Duration, Instant};
mod lifecycle;
mod updates;

#[derive(Default)]
struct RetryRegions(std::collections::HashMap<WindowId, (Bounds<Pixels>, Bounds<Pixels>)>);
impl Global for RetryRegions {}

// Observe the real native target's inherited clip without adding a hitbox or changing layout.
pub(super) fn record_retry(bounds: Bounds<Pixels>, window: &Window, cx: &mut App) {
    cx.default_global::<RetryRegions>().0.insert(
        window.window_handle().window_id(),
        (bounds, window.content_mask().bounds),
    );
}

fn visible_retry(cx: &mut VisualTestContext) {
    let (bounds, mask) =
        cx.update(|window, cx| cx.global::<RetryRegions>().0[&window.window_handle().window_id()]);
    assert!(
        mask.contains(&bounds.center()),
        "asset Retry is clipped: bounds={bounds:?}, mask={mask:?}"
    );
}

struct Harness(Entity<View>);
impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(self.0.read(cx).header_controls())
            .child(div().flex_1().min_h_0().child(self.0.clone()))
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    binding: Binding,
    session: Session,
) -> (Entity<View>, &'a mut VisualTestContext) {
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| View::new(binding, Some(session), window, cx));
        entity = Some(view.clone());
        let harness = cx.new(|_| Harness(view));
        Root::new(harness, window, cx)
    });
    (entity.unwrap(), visual)
}

fn submit(fixture: &Fixture, text: String) -> TurnId {
    submit_to(fixture, fixture.session.id, text)
}

fn submit_to(fixture: &Fixture, session: SessionId, text: String) -> TurnId {
    let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
        session,
        expected_revision: 1,
        message: text.into(),
    }) else {
        panic!("turn expected")
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let page = fixture
            .runtime
            .block_on(fixture.binding.client.read_conversation(session, None, 1))
            .unwrap();
        if page
            .page
            .runs
            .iter()
            .any(|run| run.turn == turn.id && run.status == Status::Completed)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "asset fixture completion deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    turn.id
}

#[gpui::test]
fn excludes_project_writes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::with_tools(remote, ["main.rs", "data.json"].into_iter().map(|path| (
            crate::agent_fixture::plugin_tool("files", "write_file"),
            serde_json::json!({"path": path, "text": "written fixture", "expected_revision": null}),
        )).collect());
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        }) else {
            panic!("session expected")
        };
        fixture.session = session;
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Write the fixture files".into(),
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == Status::Completed)
                })
        });
        for path in ["main.rs", "data.json"] {
            assert_eq!(
                std::fs::read_to_string(fixture.directory.path().join("project").join(path))
                    .unwrap(),
                "written fixture"
            );
        }
        tap(visual, "live-assets");
        let panel = view.read_with(visual, |view, _| view.assets.clone());
        wait(visual, |cx| !panel.read(cx).loading);
        panel.read_with(visual, |panel, _| {
            assert!(!panel.failed);
            assert!(
                panel.groups.is_empty(),
                "ordinary file writes are not deliveries"
            );
            assert!(panel.before.is_none());
        });
        fixture.close();
    }
}

#[gpui::test]
fn browsing_preserves_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::markdown(
                "[Report](report.md)".into(),
            ))
        });
        for index in 0..22 {
            submit(&fixture, format!("[Input](input-{index}.txt)"));
        }
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        assert_eq!(view.read_with(visual, |view, _| view.rows.len()), 20);
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep my draft");
        tap(visual, "live-assets");
        wait(visual, |cx| !view.read(cx).assets.read(cx).loading);
        let panel = view.read_with(visual, |view, _| view.assets.clone());
        assert_eq!(panel.read_with(visual, |panel, _| panel.groups.len()), 20);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        for selector in [
            "asset-activity",
            "asset-progress",
            "asset-output",
            "asset-sources",
        ] {
            assert!(visual.debug_bounds(selector).is_some());
        }
        assert!(visual.debug_bounds("asset-tab-0").is_none());
        assert!(visual.debug_bounds("asset-refresh").is_none());
        assert_eq!(
            visual.debug_bounds("asset-activity").unwrap().top(),
            visual.debug_bounds("asset-panel").unwrap().top()
        );
        for expected in [40, 44] {
            panel.update(visual, |panel, cx| {
                panel.scroll.scroll_to_bottom();
                cx.notify();
            });
            wait(visual, |_| true);
            tap(visual, "asset-more");
            wait(visual, |cx| !panel.read(cx).loading);
            assert_eq!(
                panel.read_with(visual, |panel, _| panel.groups.len()),
                expected
            );
        }
        panel.read_with(visual, |panel, _| {
            assert!(!panel.failed);
            assert!(panel.before.is_none());
            assert_eq!(
                panel
                    .groups
                    .iter()
                    .filter(|group| group.kind == Kind::Resource)
                    .count(),
                22
            );
            assert_eq!(
                panel
                    .groups
                    .iter()
                    .filter(|group| group.kind == Kind::Artifact)
                    .count(),
                22
            );
        });
        panel.update(visual, |panel, cx| {
            panel.scroll.set_offset(point(px(0.), px(0.)));
            cx.notify();
        });
        wait(visual, |_| true);
        let outputs = panel.read_with(visual, |panel, _| {
            panel
                .groups
                .iter()
                .filter(|group| group.kind == Kind::Artifact)
                .map(|group| format!("asset-{}-0", group.sequence))
                .collect::<Vec<_>>()
        });
        assert!(
            visual
                .debug_bounds(Box::leak(outputs[0].clone().into_boxed_str()))
                .is_some()
        );
        for duplicate in outputs.into_iter().skip(1) {
            assert!(
                visual
                    .debug_bounds(Box::leak(duplicate.into_boxed_str()))
                    .is_none(),
                "repeated outputs must stay hidden across loaded pages"
            );
        }
        let chosen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let captured = chosen.clone();
        visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::Artifact(worktree, file) = event {
                    captured.borrow_mut().push((*worktree, file.clone()));
                }
            })
            .detach();
        });
        let selector = panel.read_with(visual, |panel, _| {
            let group = panel
                .groups
                .iter()
                .find(|group| group.kind == Kind::Artifact)
                .unwrap();
            format!("asset-{}-0", group.sequence)
        });
        tap(visual, Box::leak(selector.into_boxed_str()));
        let chosen = chosen.borrow();
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].0, fixture.session.worktree);
        assert_eq!(chosen[0].1.path, "report.md");
        assert_eq!(chosen[0].1.mime, "text/markdown");
        assert!(visual.debug_bounds("asset-panel").is_none());
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "Keep my draft"
        );
        assert_eq!(fixture.task_requests(), 22);
        fixture.close();
    }
}

#[gpui::test]
fn opens_image_preview_and_returns_to_inventory(cx: &mut TestAppContext) {
    init(cx);
    // Kit dialog entrances use wall time, not the fixture's executor clock.
    cx.update(|cx| cx.set_reduce_motion(true));
    for remote in [false, true] {
        let fixture = Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::markdown(
                "![Image](result.png)".into(),
            ))
        });
        image::DynamicImage::new_rgb8(2, 2)
            .save(fixture.directory.path().join("project/result.png"))
            .unwrap();
        submit(&fixture, "Create a picture".into());
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-assets");
        let panel = view.read_with(visual, |view, _| view.assets.clone());
        wait(visual, |cx| !panel.read(cx).loading);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        let selector = panel.read_with(visual, |panel, _| {
            let group = panel
                .groups
                .iter()
                .find(|group| group.kind == Kind::Artifact)
                .unwrap();
            format!("asset-{}-0", group.sequence)
        });
        let selector: &'static str = Box::leak(selector.into_boxed_str());
        let inventory = panel.read_with(visual, |panel, _| {
            (
                panel.groups.clone(),
                panel.before,
                panel.loaded_revision,
                panel.scroll.offset(),
                panel.stamp,
            )
        });
        tap(visual, selector);
        assert!(visual.debug_bounds("attachment-image-preview").is_some());
        assert!(!panel.read_with(visual, |panel, _| panel.open));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            wait(visual, |_| true);
            if visual.debug_bounds("image-lightbox-image").is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "image preview load deadline (remote={remote})"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let position = visual
            .debug_bounds("image-lightbox-image")
            .unwrap()
            .center();
        visual.simulate_click(position, Modifiers::default());
        assert!(!panel.read_with(visual, |panel, _| panel.open));
        let canvas = visual.debug_bounds("image-lightbox-canvas").unwrap();
        visual.simulate_click(
            point(canvas.left() + px(2.), canvas.top() + px(2.)),
            Modifiers::default(),
        );
        wait(visual, |cx| panel.read(cx).open);
        assert!(visual.debug_bounds("asset-panel").is_some());
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(
            panel.read_with(visual, |panel, _| {
                (
                    panel.groups.clone(),
                    panel.before,
                    panel.loaded_revision,
                    panel.scroll.offset(),
                    panel.stamp,
                )
            }),
            inventory
        );
        tap(visual, selector);
        assert!(visual.debug_bounds("attachment-image-preview").is_some());
        assert!(!panel.read_with(visual, |panel, _| panel.open));
        visual.simulate_keystrokes("escape");
        wait(visual, |_| true);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(panel.read_with(visual, |panel, _| panel.open));
        assert!(visual.debug_bounds("asset-panel").is_some());
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| !panel.read(cx).open);
        let other = Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::markdown(
                "Other session".into(),
            ))
        });
        for binding_changed in [false, true] {
            // Mount the real panel alone: a mounted View would resync its original
            // scope each frame and undo this fixture's deliberate scope change.
            let mut owner = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let view = cx.new(|cx| {
                    View::new(
                        fixture.binding.clone(),
                        Some(fixture.session.clone()),
                        window,
                        cx,
                    )
                });
                let panel = view.read(cx).assets.clone();
                owner = Some(view);
                Root::new(panel, window, cx)
            });
            let owner = owner.unwrap();
            wait(visual, |cx| owner.read(cx).connected());
            let panel = owner.read_with(visual, |view, _| view.assets.clone());
            visual.update(|window, cx| {
                let view = owner.read(cx);
                let images = view.images.clone();
                let stamp = view.history.snapshot.as_ref().map(|snapshot| {
                    (
                        snapshot.page.revision,
                        snapshot
                            .page
                            .entries
                            .last()
                            .map_or(0, |entry| entry.sequence),
                    )
                });
                panel.update(cx, |panel, cx| {
                    panel.sync(
                        &fixture.binding,
                        &images,
                        Some(fixture.session.id),
                        stamp,
                        window,
                        cx,
                    );
                });
            });
            tap(visual, "live-assets");
            wait(visual, |cx| panel.read(cx).open && !panel.read(cx).loading);
            tap(visual, selector);
            assert!(visual.debug_bounds("attachment-image-preview").is_some());
            assert!(!panel.read_with(visual, |panel, _| panel.open));
            visual.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    let binding = if binding_changed {
                        &other.binding
                    } else {
                        &fixture.binding
                    };
                    panel.sync(
                        binding,
                        &panel.images.clone(),
                        binding_changed.then_some(fixture.session.id),
                        panel.stamp,
                        window,
                        cx,
                    );
                });
                assert!(!panel.read(cx).open);
            });
            wait(visual, |_| true);
            panel.read_with(visual, |panel, _| {
                assert_eq!(panel.session, binding_changed.then_some(fixture.session.id));
                assert_eq!(
                    panel.binding.client.target(),
                    if binding_changed {
                        other.binding.client.target()
                    } else {
                        fixture.binding.client.target()
                    }
                );
            });
            visual.simulate_keystrokes("escape");
            wait(visual, |cx| !panel.read(cx).open);
            assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
            assert!(visual.debug_bounds("asset-panel").is_none());
            visual.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.sync(
                        &fixture.binding,
                        &panel.images.clone(),
                        Some(fixture.session.id),
                        inventory.4,
                        window,
                        cx,
                    );
                });
            });
            tap(visual, "live-assets");
            wait(visual, |cx| panel.read(cx).open && !panel.read(cx).loading);
            assert_eq!(
                panel.read_with(visual, |panel, _| {
                    (panel.groups.clone(), panel.before, panel.loaded_revision)
                }),
                (inventory.0.clone(), inventory.1, inventory.2)
            );
            visual.simulate_keystrokes("escape");
            wait(visual, |cx| !panel.read(cx).open);
        }
        other.close();
        fixture.close();
    }
}

#[gpui::test]
fn opens_child_without_discarding_draft(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = super::super::subagents::fixture::Fixture::new(remote);
        fixture.start();
        let binding = Binding {
            client: fixture.client.clone(),
            defaults: fixture.client.clone(),
            runtime: fixture.runtime.clone(),
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            host: "Fixture".into(),
            project_name: "Overview".into(),
            branch: "main".into(),
        };
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            let view = view.read(cx);
            view.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.page.children.len() == 2
                    && snapshot
                        .page
                        .children
                        .iter()
                        .all(|child| view.child_session(child.run.session).is_some())
            }) && view
                .history
                .calls
                .iter()
                .any(|call| call.progress.is_some())
        });
        let (turn, children) = view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            (
                page.runs[0].turn,
                page.children
                    .iter()
                    .map(|child| child.run.session)
                    .collect::<Vec<_>>(),
            )
        });
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep my draft");
        let selected = std::rc::Rc::new(std::cell::Cell::new(None));
        let captured = selected.clone();
        let _subscription = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::Subagent(id) = event {
                    captured.set(Some(*id));
                }
            })
        });
        tap(visual, "live-assets");
        let panel = view.read_with(visual, |view, _| view.assets.clone());
        wait(visual, |cx| !panel.read(cx).loading);
        assert!(visual.debug_bounds("asset-activity-heading").is_some());
        assert!(visual.debug_bounds("asset-progress-heading").is_some());
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-progress-overview-{turn}").into_boxed_str()
                ))
                .is_some()
        );
        for child in &children {
            assert!(
                visual
                    .debug_bounds(Box::leak(format!("asset-child-{child}").into_boxed_str()))
                    .is_some()
            );
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("asset-child-avatar-{child}").into_boxed_str()
                    ))
                    .is_some()
            );
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("asset-child-status-{child}").into_boxed_str()
                    ))
                    .is_none()
            );
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("asset-child-completed-{child}").into_boxed_str()
                    ))
                    .is_none()
            );
        }
        let original = view.read_with(visual, |view, _| view.history.clone());
        view.update(visual, |view, cx| {
            let page =
                Arc::make_mut(&mut Arc::make_mut(view.history.snapshot.as_mut().unwrap()).page);
            page.children[0].run.status = Status::Completed;
            cx.notify();
        });
        wait(visual, |_| true);
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("asset-child-completed-{}", children[0]).into_boxed_str()
                ))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("asset-child-status-{}", children[0]).into_boxed_str()
                ))
                .is_none()
        );
        // Both activity surfaces clear when all children finish, without deleting history.
        view.update(visual, |view, cx| {
            let page =
                Arc::make_mut(&mut Arc::make_mut(view.history.snapshot.as_mut().unwrap()).page);
            for child in &mut page.children {
                child.run.status = Status::Completed;
            }
            assert!(view.child_activity_turn().is_none());
            assert!(overview::Overview::new(view).children.is_empty());
            assert_eq!(view.child_rows(turn).len(), 2);
            cx.notify();
        });
        wait(visual, |_| true);
        assert!(visual.debug_bounds("live-composer-subagents").is_none());
        for child in &children {
            assert!(
                visual
                    .debug_bounds(Box::leak(format!("asset-child-{child}").into_boxed_str()))
                    .is_none()
            );
        }
        // A terminal parent must not repopulate activity from its historical children.
        for status in [
            Status::Completed,
            Status::Cancelled,
            Status::Failed,
            Status::Interrupted,
            Status::Continued,
        ] {
            view.update(visual, |view, cx| {
                view.history = original.clone();
                let page =
                    Arc::make_mut(&mut Arc::make_mut(view.history.snapshot.as_mut().unwrap()).page);
                page.runs
                    .iter_mut()
                    .find(|run| run.turn == turn)
                    .unwrap()
                    .status = status;
                assert!(view.active().is_none());
                assert!(view.child_activity_turn().is_none());
                assert!(overview::Overview::new(view).children.is_empty());
                assert_eq!(view.child_rows(turn).len(), 2);
                cx.notify();
            });
            wait(visual, |_| true);
            assert!(visual.debug_bounds("live-composer-subagents").is_none());
            for child in &children {
                assert!(
                    visual
                        .debug_bounds(Box::leak(format!("asset-child-{child}").into_boxed_str()))
                        .is_none()
                );
            }
            assert!(panel.read_with(visual, |panel, _| panel.open));
        }
        view.update(visual, |view, cx| {
            view.history = original;
            cx.notify();
        });
        tap(visual, &format!("asset-child-{}", children[0]));
        assert_eq!(selected.get(), Some(children[0]));
        assert!(!panel.read_with(visual, |panel, _| panel.open));
        assert!(visual.debug_bounds("asset-panel").is_none());
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "Keep my draft"
        );
        fixture.execute(Command::StopTurn { turn });
        wait(visual, |_| {
            let page = fixture.page(fixture.session.id);
            page.runs[0].status == Status::Cancelled
                && page.children.iter().all(|child| {
                    fixture.page(child.run.session).runs[0].status == Status::Cancelled
                })
        });
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn scopes_current_turn(cx: &mut TestAppContext) {
    use sailry_client::conversation::tools::{Call, Reference, State};
    use sailry_protocol::conversation::{
        Citation,
        progress::{Progress, State as StepState, Step},
    };
    use sailry_protocol::tool::{Grouping, Presentation};

    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::markdown("Done".into()))
        });
        submit(&fixture, "Earlier request".into());
        submit(&fixture, "Current request".into());
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.page.runs.len() == 2)
        });
        let original = view.update(visual, |view, cx| {
            let original = view.history.clone();
            let page =
                Arc::make_mut(&mut Arc::make_mut(view.history.snapshot.as_mut().unwrap()).page);
            let earlier = page
                .runs
                .iter()
                .min_by_key(|run| run.sequence)
                .unwrap()
                .turn;
            let current = page
                .runs
                .iter()
                .max_by_key(|run| run.sequence)
                .unwrap()
                .turn;
            let citation = |uri: &str| Citation {
                uri: uri.into(),
                title: None,
                start: None,
                end: None,
            };
            for (turn, uri) in [
                (earlier, "https://example.invalid/earlier"),
                (current, "https://example.invalid/current"),
            ] {
                let entry = page
                    .entries
                    .iter_mut()
                    .rev()
                    .find(|entry| entry.turn == turn)
                    .unwrap();
                entry.citations = vec![citation(uri), citation(uri)];
            }
            let plan = |turn, title: &str| Call {
                turn,
                id: None,
                name: crate::agent_fixture::plugin_tool("progress", "update_plan"),
                presentation: Presentation::Progress,
                grouping: Grouping::Standalone,
                source: Reference {
                    entry: title.into(),
                    index: 0,
                },
                response: None,
                state: State::Returned,
                approval: None,
                question: None,
                progress: Some(Progress {
                    title: Some(title.into()),
                    steps: vec![Step {
                        description: title.into(),
                        state: StepState::Completed,
                    }],
                }),
                content: None,
                resolved: None,
            };
            view.history.calls = Arc::new(vec![
                plan(earlier, "Earlier plan"),
                plan(current, "Superseded plan"),
                plan(current, "Current plan"),
            ]);
            let overview = overview::Overview::new(view);
            assert_eq!(overview.turn, Some(current));
            assert_eq!(
                overview.progress.unwrap().title.as_deref(),
                Some("Current plan")
            );
            assert_eq!(
                overview.sources,
                [citation("https://example.invalid/current")]
            );

            let page =
                Arc::make_mut(&mut Arc::make_mut(view.history.snapshot.as_mut().unwrap()).page);
            page.runs
                .iter_mut()
                .find(|run| run.turn == earlier)
                .unwrap()
                .status = Status::Running;
            let overview = overview::Overview::new(view);
            assert_eq!(overview.turn, Some(earlier));
            assert_eq!(
                overview.progress.unwrap().title.as_deref(),
                Some("Earlier plan")
            );
            assert_eq!(
                overview.sources,
                [citation("https://example.invalid/earlier")]
            );

            let page =
                Arc::make_mut(&mut Arc::make_mut(view.history.snapshot.as_mut().unwrap()).page);
            page.runs
                .iter_mut()
                .find(|run| run.turn == earlier)
                .unwrap()
                .status = Status::Completed;
            Arc::make_mut(&mut view.history.calls).retain(|call| call.turn != current);
            let overview = overview::Overview::new(view);
            assert_eq!(overview.turn, Some(current));
            assert!(overview.progress.is_none());
            view.history.calls = Arc::new(Vec::new());
            cx.notify();
            original
        });
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep my source draft");
        let selected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let captured = selected.clone();
        let _subscription = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::Link(uri) = event {
                    captured.borrow_mut().push(uri.to_string());
                }
            })
        });
        tap(visual, "live-assets");
        let panel = view.read_with(visual, |view, _| view.assets.clone());
        wait(visual, |cx| !panel.read(cx).loading);
        panel.update(visual, |panel, cx| {
            panel.scroll.scroll_to_bottom();
            cx.notify();
        });
        wait(visual, |_| true);
        assert!(visual.debug_bounds("asset-source-0").is_some());
        assert!(visual.debug_bounds("asset-source-1").is_none());
        tap(visual, "asset-source-0");
        assert_eq!(&*selected.borrow(), &["https://example.invalid/current"]);
        assert!(!panel.read_with(visual, |panel, _| panel.open));
        assert!(visual.debug_bounds("asset-panel").is_none());
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "Keep my source draft"
        );
        assert_eq!(fixture.task_requests(), 2);
        view.update(visual, |view, cx| {
            view.history = original;
            cx.notify();
        });
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
