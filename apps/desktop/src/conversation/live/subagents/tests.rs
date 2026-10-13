use super::super::tests::{
    fixture::{init, open, tap},
    wait,
};
use super::*;
use core::prelude::v1::test;
use sailry_protocol::conversation::ApprovalState;

fn assert_icons(cx: &mut VisualTestContext, children: &[Child], rotating: bool) -> Bounds<Pixels> {
    let first = key(&children[0]);
    let strip = cx
        .debug_bounds(Box::leak(
            format!("live-subagent-icons-{first}").into_boxed_str(),
        ))
        .unwrap();
    let mut previous = strip.left();
    for child in children {
        let id = format!("live-subagent-icon-{}", key(child));
        let bounds = cx
            .debug_bounds(Box::leak(id.clone().into_boxed_str()))
            .unwrap();
        assert_eq!(bounds.size, size(px(16.), px(16.)));
        assert!(bounds.left() >= previous);
        assert!(bounds.right() <= strip.right());
        assert_eq!(bounds.center().y, strip.center().y);
        previous = bounds.right();
        for (state, expected) in [("rotating", rotating), ("static", !rotating)] {
            assert_eq!(
                cx.debug_bounds(Box::leak(format!("{id}-{state}").into_boxed_str()))
                    .is_some(),
                expected,
                "unexpected {state} state for {id}"
            );
        }
    }
    strip
}

#[gpui::test]
fn keeps_active_children_after_handoff(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;

    init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_children(remote, 1);
        fixture.start();
        let binding = Binding {
            client: fixture.client.clone(),
            defaults: fixture.client.clone(),
            runtime: fixture.runtime.clone(),
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            host: "Fixture".into(),
            project_name: "Child handoff".into(),
            branch: "main".into(),
        };
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.page.children.first().is_some_and(|child| {
                        child.run.status == Status::Running
                            && view.read(cx).child_session(child.run.session).is_some()
                    })
                })
        });
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        let original = view.read_with(visual, |view, _| view.history.clone());
        let continued = TurnId::new();
        let child = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.children[0].clone()
        });
        let removed_turn = TurnId::new();
        view.update(visual, |view, cx| {
            let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
            let page = Arc::make_mut(&mut snapshot.page);
            let previous = page
                .runs
                .iter_mut()
                .find(|run| run.turn == child.origin.turn)
                .unwrap();
            previous.status = Status::Cancelled;
            let mut next = previous.clone();
            next.turn = continued;
            next.sequence += 1;
            next.status = Status::Running;
            page.runs.push(next);
            let mut removed = child.clone();
            removed.run.turn = removed_turn;
            removed.run.sequence += 1;
            removed.run.status = Status::Cancelled;
            page.children.push(removed);
            assert_eq!(view.active(), Some(continued));
            assert_eq!(view.activity_rows().len(), 1);
            assert!(view.activity_rows()[0].running);
            assert_eq!(view.activity_children()[0].origin, child.origin);
            cx.notify();
        });
        wait(visual, |_| true);
        assert!(visual.debug_bounds("live-composer-subagents").is_some());
        visual.update(|window, cx| {
            window.render_frame(cx);
            let label = rust_i18n::t!("composer_subagent_count", count = 1).to_string();
            assert_eq!(
                window.find("live-composer-subagents").label(),
                Some(label.as_str())
            );
        });
        tap(visual, "live-composer-subagents");
        assert!(visual.debug_bounds("subagent-picker").is_some());
        let status = Box::leak(format!("subagent-status-{}", child.run.session).into_boxed_str());
        assert!(visual.debug_bounds(status).is_none());
        view.update(visual, |view, cx| {
            let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
            let page = Arc::make_mut(&mut snapshot.page);
            page.children.retain(|child| child.run.turn != removed_turn);
            page.children[0].run.status = Status::Completed;
            cx.notify();
        });
        wait(visual, |_| true);
        assert!(visual.debug_bounds(status).is_none());
        assert!(visual.debug_bounds("live-composer-subagents").is_none());
        assert!(visual.debug_bounds("subagent-picker").is_none());
        let label = view.read_with(visual, |view, _| {
            let completed = view.child(child.run.session).unwrap();
            assert_eq!(completed.run.status, Status::Completed);
            assert!(view.activity_rows().is_empty());
            format!(
                "{}: {}",
                view.child_name(completed),
                tr(status_key(Status::Completed))
            )
        });
        tap(visual, &format!("live-turn-work-{}", child.origin.turn));
        wait(visual, |_| true);
        visual.update(|window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window
                    .find(SharedString::from(format!(
                        "live-subagent-row-{}",
                        key(&child)
                    )))
                    .label(),
                Some(label.as_str())
            );
        });
        view.update(visual, |view, cx| {
            view.history = original;
            cx.notify();
        });
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn single_child_opens_directly(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_children(remote, 1);
        fixture.start();
        let binding = Binding {
            client: fixture.client.clone(),
            defaults: fixture.client.clone(),
            runtime: fixture.runtime.clone(),
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            host: "Fixture".into(),
            project_name: "Child".into(),
            branch: "main".into(),
        };
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.page.children.first().is_some_and(|child| {
                        view.read(cx).child_session(child.run.session).is_some()
                    })
                })
        });
        let child = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.children[0].clone()
        });
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        let selected = std::rc::Rc::new(std::cell::Cell::new(None));
        let captured = selected.clone();
        let _subscription = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::Subagent(id) = event {
                    captured.set(Some(*id));
                }
            })
        });
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-turn-subagents-{}", key(&child)).into_boxed_str()
                ))
                .is_none()
        );
        let row = format!("live-subagent-row-{}", key(&child));
        let bounds = visual
            .debug_bounds(Box::leak(row.clone().into_boxed_str()))
            .unwrap();
        assert!(bounds.size.width < px(400.));
        assert_eq!(bounds.size.height, px(24.));
        tap(visual, &row);
        assert_eq!(selected.get(), Some(child.run.session));
        assert!(visual.debug_bounds("subagent-picker").is_none());
        view.update(visual, |view, _| {
            for status in [
                Status::Queued,
                Status::Running,
                Status::Stopping,
                Status::Completed,
                Status::Continued,
                Status::Cancelled,
                Status::Interrupted,
                Status::Failed,
            ] {
                let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
                let current = Arc::make_mut(&mut snapshot.page)
                    .children
                    .iter_mut()
                    .find(|current| current.run.turn == child.run.turn)
                    .unwrap();
                current.run.status = status;
                current.run.error = Some(sailry_protocol::Fault::invalid("Fixture diagnostic"));
                assert_eq!(view.child(child.run.session).unwrap().run.status, status);
                let rows = view.activity_rows();
                if matches!(status, Status::Queued | Status::Running | Status::Stopping) {
                    assert_eq!(rows.len(), 1);
                    assert_eq!(
                        rows[0].running,
                        matches!(status, Status::Running | Status::Stopping)
                    );
                } else {
                    assert!(rows.is_empty());
                }
            }
            let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
            *Arc::make_mut(&mut snapshot.page)
                .children
                .iter_mut()
                .find(|current| current.run.turn == child.run.turn)
                .unwrap() = child.clone();
        });
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn readonly_approvals(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::new(remote);
        fixture.start();
        let binding = Binding {
            client: fixture.client.clone(),
            defaults: fixture.client.clone(),
            runtime: fixture.runtime.clone(),
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            host: "Fixture".into(),
            project_name: "Child approvals".into(),
            branch: "main".into(),
        };
        let (parent, visual) = open(cx, binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            parent.read(cx).activity_rows().len() == 2
                && fixture
                    .page(fixture.session.id)
                    .children
                    .iter()
                    .all(|child| {
                        parent.read(cx).child_session(child.run.session).is_some()
                            && !fixture.page(child.run.session).approvals.is_empty()
                    })
        });
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        let turn = fixture.page(fixture.session.id).runs[0].turn;
        let mut children = fixture.page(fixture.session.id).children;
        children.sort_by_key(|child| child.origin.index);
        assert_eq!(children.len(), 2);
        let first = key(&children[0]);
        parent.read_with(visual, |view, _| {
            assert!(
                view.history
                    .calls
                    .iter()
                    .filter(|call| call.name
                        == crate::agent_support::plugin_tool("delegation", "spawn_agent"))
                    .all(|call| call.grouping == sailry_protocol::tool::Grouping::Standalone)
            );
        });
        let key = format!("subagents-{first}");
        let toggle = format!("live-turn-subagents-{first}");
        let list = Box::leak(format!("live-subagent-list-{first}").into_boxed_str());
        assert!(visual.debug_bounds(list).is_none());
        let collapsed_icons = assert_icons(visual, &children, true);
        tap(visual, &toggle);
        assert!(visual.debug_bounds(list).is_some());
        assert_eq!(assert_icons(visual, &children, true), collapsed_icons);
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-turn-subagents-{first}-summary-shimmer").into_boxed_str()
                ))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-subagent-row-{first}-summary-shimmer").into_boxed_str()
                ))
                .is_some()
        );
        assert!(visual.debug_bounds("subagent-picker").is_none());
        tap(visual, &toggle);
        wait(visual, |_| true);
        assert!(!parent.read_with(visual, |view, _| view.expanded[&(turn, key.clone())]));
        tap(visual, &toggle);
        assert!(parent.read_with(visual, |view, _| view.expanded[&(turn, key.clone())]));
        let todo = visual.debug_bounds("composer-todo").unwrap();
        assert!(visual.debug_bounds("composer-activity").is_some());
        let children_button = visual.debug_bounds("live-composer-subagents").unwrap();
        assert!(children_button.left() >= todo.right());
        assert!((children_button.center().y - todo.center().y).abs() < px(1.));
        parent.read_with(visual, |view, _| {
            let names: Vec<_> = view
                .activity_rows()
                .into_iter()
                .map(|row| row.name)
                .collect();
            assert!(names.iter().any(|name| name == "Review Inspect files"));
            assert!(names.iter().any(|name| name == "Review Check lifecycle"));
        });
        let sessions = parent.read_with(visual, |parent, _| {
            children
                .iter()
                .map(|child| parent.child_session(child.run.session).unwrap())
                .collect::<Vec<_>>()
        });
        // Children can settle before their parent finishes its report.
        let original = parent.read_with(visual, |view, _| view.history.clone());
        parent.update(visual, |view, cx| {
            let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
            let page = Arc::make_mut(&mut snapshot.page);
            assert!(
                page.runs
                    .iter()
                    .any(|run| run.turn == turn && run.status == Status::Running)
            );
            for child in &mut page.children {
                child.run.status = Status::Completed;
            }
            for call in Arc::make_mut(&mut view.history.calls) {
                if let Some(progress) = &mut call.progress {
                    for step in &mut progress.steps {
                        step.state = sailry_protocol::conversation::progress::State::Completed;
                    }
                }
            }
            cx.notify();
        });
        wait(visual, |_| true);
        assert!(visual.debug_bounds("composer-todo").is_none());
        assert!(visual.debug_bounds("live-composer-subagents").is_none());
        assert!(visual.debug_bounds(list).is_some());
        assert_icons(visual, &children, false);
        // The last activity keeps its loader while the parent awaits new content.
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-turn-subagents-{first}-summary-shimmer").into_boxed_str()
                ))
                .is_some()
        );
        parent.update(visual, |view, cx| {
            let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
            let page = Arc::make_mut(&mut snapshot.page);
            page.runs
                .iter_mut()
                .find(|run| run.turn == turn)
                .unwrap()
                .status = Status::Completed;
            cx.notify();
        });
        wait(visual, |_| true);
        tap(visual, &format!("live-turn-work-{turn}"));
        assert!(visual.debug_bounds(list).is_some());
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-turn-subagents-{first}-summary-shimmer").into_boxed_str()
                ))
                .is_none()
        );
        let progress = original
            .calls
            .iter()
            .find_map(|call| call.progress.clone())
            .unwrap();
        parent.update(visual, |view, cx| {
            view.history = original;
            cx.notify();
        });
        visual.update(|window, _| window.remove_window());
        drop(parent);
        for (index, session) in sessions.into_iter().enumerate() {
            let (child, visual) = open(cx, binding.clone(), session.clone());
            wait(visual, |cx| {
                child.read(cx).connected()
                    && child
                        .read(cx)
                        .history
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| !snapshot.page.approvals.is_empty())
            });
            child.update(visual, |view, cx| {
                Arc::make_mut(&mut view.history.calls)[0].progress = Some(progress.clone());
                // Prove the child has renderable status; its panel must still omit the strip.
                let _ = view.composer_activity(cx);
                cx.notify();
            });
            wait(visual, |_| true);
            assert!(visual.debug_bounds("live-chat-input").is_none());
            assert!(visual.debug_bounds("composer-activity").is_none());
            assert!(visual.debug_bounds("composer-todo").is_none());
            assert!(visual.debug_bounds("composer-toolbar").is_none());
            assert!(visual.debug_bounds("message-navigation").is_none());
            assert!(
                child.read_with(visual, |view, _| view.navigation.bounds.get().size.height
                    > px(0.))
            );
            assert!(visual.debug_bounds("live-chat-model").is_none());
            assert!(visual.debug_bounds("live-chat-mode").is_none());
            assert!(visual.debug_bounds("live-chat-send").is_none());
            visual.update(|window, cx| {
                child.update(cx, |child, cx| {
                    assert!(child.readonly());
                    child.input.update(cx, |input, cx| {
                        input.set_value("Must not submit", window, cx)
                    });
                    child.send(window, cx);
                    let mut config = session.config.clone();
                    config.effort = Effort::High;
                    child.configure(config, window, cx);
                    assert!(!child.pending && child.retry.is_none());
                    assert_eq!(child.config.as_ref(), Some(&session.config));
                })
            });
            let page = fixture.page(session.id);
            let approval = page
                .approvals
                .iter()
                .find(|approval| approval.state == ApprovalState::Pending)
                .unwrap()
                .id;
            assert!(visual.debug_bounds("live-approval-trigger").is_none());
            assert!(visual.debug_bounds("child-guide-input").is_none());
            visual.update(|window, cx| {
                child.update(cx, |view, cx| {
                    view.resolve_approval(
                        approval,
                        sailry_protocol::conversation::Decision::Approve,
                        window,
                        cx,
                    );
                    assert!(view.approvals.attempt.is_none());
                });
            });
            fixture.execute(Command::ResolveApproval {
                session: session.id,
                approval,
                decision: if index == 0 {
                    sailry_protocol::conversation::Decision::Approve
                } else {
                    sailry_protocol::conversation::Decision::Deny
                },
            });
            wait(visual, |cx| {
                child
                    .read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.page.runs[0].status == Status::Completed)
            });
            assert_eq!(fixture.page(session.id).runs.len(), 1);
            assert!(visual.debug_bounds("composer-activity").is_none());
            let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
                panic!("snapshot expected")
            };
            assert_eq!(
                snapshot
                    .sessions
                    .iter()
                    .find(|item| item.id == session.id)
                    .unwrap()
                    .revision,
                session.revision
            );
            visual.update(|window, _| window.remove_window());
        }
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/child.txt")).unwrap(),
            "Delegated Unicode content 中文 🙂"
        );
        fixture.runtime.block_on(fixture.parent.wait_count(2));
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 4);
        let (parent, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            parent
                .read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.page.runs[0].status == Status::Completed)
        });
        parent.read_with(visual, |view, _| {
            let children = latest(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .children
                    .iter()
                    .filter(|child| child.origin.turn == turn),
            );
            assert_eq!(children.len(), 2);
            assert!(
                children
                    .iter()
                    .all(|child| child.run.status == Status::Completed)
            );
            assert!(view.activity_rows().is_empty());
            assert!(View::child_label(2, 2).starts_with("2/2"));
        });
        assert!(visual.debug_bounds("composer-todo").is_none());
        assert!(visual.debug_bounds("live-composer-subagents").is_none());
        tap(visual, &format!("live-turn-work-{turn}"));
        tap(visual, &toggle);
        assert!(parent.read_with(visual, |view, _| view.expanded[&(turn, key.clone())]));
        assert!(visual.debug_bounds("subagent-picker").is_none());
        assert_icons(visual, &children, false);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn picker_spaces_rows_and_preserves_keyboard(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_children(remote, 2);
        fixture.start();
        let binding = Binding {
            client: fixture.client.clone(),
            defaults: fixture.client.clone(),
            runtime: fixture.runtime.clone(),
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            host: "Fixture".into(),
            project_name: "Picker".into(),
            branch: "main".into(),
        };
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.page.children.len() == 2
                        && snapshot
                            .page
                            .children
                            .iter()
                            .all(|child| view.read(cx).child_session(child.run.session).is_some())
                })
        });
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        let selected = std::rc::Rc::new(std::cell::Cell::new(None));
        let captured = selected.clone();
        let _subscription = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::Subagent(id) = event {
                    captured.set(Some(*id));
                }
            })
        });
        tap(visual, "live-composer-subagents");
        wait(visual, |_| true);
        let ids = view.read_with(visual, |view, _| {
            view.history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .children
                .iter()
                .map(|child| child.run.session)
                .collect::<Vec<_>>()
        });
        let mut rows = ids
            .into_iter()
            .map(|id| {
                (
                    id,
                    visual
                        .debug_bounds(Box::leak(format!("subagent-row-{id}").into_boxed_str()))
                        .unwrap(),
                )
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|(_, bounds)| bounds.top());
        assert_eq!(rows[0].1.size.height, px(26.));
        assert_eq!(rows[1].1.size.height, px(26.));
        assert!(rows[1].1.top() - rows[0].1.bottom() >= px(1.));
        visual.update(|window, cx| {
            let entries = window
                .find_all(0usize)
                .into_iter()
                .filter(|entry| entry.role() == Some(Role::TreeItem))
                .collect::<Vec<_>>();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].selected(), Some(true));
            let bounds = rows[0].1.scale(window.scale_factor());
            assert!(
                !window
                    .painted_quads()
                    .iter()
                    .any(|quad| quad.bounds == bounds
                        && (quad.background == cx.theme().list_active.into()
                            || quad.background == cx.theme().accent.into()))
            );
        });
        visual.simulate_mouse_move(rows[0].1.center(), None, Modifiers::default());
        wait(visual, |_| true);
        visual.update(|window, cx| {
            let bounds = rows[0].1.scale(window.scale_factor());
            assert!(
                window
                    .painted_quads()
                    .iter()
                    .any(|quad| quad.bounds == bounds
                        && quad.background == cx.theme().tokens.list_hover.into())
            );
        });
        visual.simulate_keystrokes("down");
        wait(visual, |_| true);
        visual.update(|window, cx| {
            let entries = window
                .find_all(1usize)
                .into_iter()
                .filter(|entry| entry.role() == Some(Role::TreeItem))
                .collect::<Vec<_>>();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].selected(), Some(true));
            let bounds = rows[1].1.scale(window.scale_factor());
            assert!(
                window
                    .painted_quads()
                    .iter()
                    .any(|quad| quad.bounds == bounds && quad.border_color == cx.theme().selection)
            );
        });
        visual.simulate_keystrokes("enter");
        wait(visual, |_| true);
        assert_eq!(selected.get(), Some(rows[1].0));
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn background_activity_outlives_parent(cx: &mut TestAppContext) {
    use crate::{backend::Services, resources::SideResource, shell::Shell};
    init(cx);
    cx.update(crate::shell::init);
    for remote in [false, true] {
        let release = CancellationToken::new();
        let fixture = fixture::Fixture::with_background(remote, release.clone());
        cx.update(|cx| {
            cx.set_global::<Services>(fixture.services());
            cx.set_global(crate::preferences::Preferences::default());
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1440.), px(900.)));
        let target = fixture.client.target();
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&target)
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(target, cx)
            });
        });
        wait(visual, |cx| {
            let live = shell.read(cx).live.as_ref().unwrap();
            live.selected == target
                && live.view.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot
                        .sessions
                        .iter()
                        .any(|session| session.id == fixture.session.id)
                })
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.session.clone(), window, cx)
            });
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .current_chat()
                .is_some_and(|source| source.read(cx).connected())
        });
        let source = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        source.read_with(visual, |view, _| {
            assert_eq!(view.binding().client.target(), target);
            assert_eq!(view.session(), Some(fixture.session.id));
            assert!(view.connected());
        });
        fixture.start();
        wait(visual, |cx| {
            let view = source.read(cx);
            view.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .page
                    .runs
                    .iter()
                    .any(|run| run.status == Status::Completed)
                    && snapshot.page.children.len() == 1
                    && snapshot.page.children[0].run.status == Status::Running
                    && view
                        .child_session(snapshot.page.children[0].run.session)
                        .is_some()
            })
        });
        fixture.runtime.block_on(fixture.child.wait_count(1));
        assert!(source.read_with(visual, |view, _| view.active().is_none()));
        assert!(visual.debug_bounds("live-composer-subagents").is_some());
        let first = source.read_with(visual, |view, _| view.activity_children()[0].clone());
        visual.update(|window, cx| {
            source.update(cx, |view, cx| {
                view.input.update(cx, |input, cx| {
                    input.set_value("Keep my parent draft", window, cx)
                })
            })
        });
        fixture.start();
        wait(visual, |cx| {
            let view = source.read(cx);
            view.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .page
                    .runs
                    .iter()
                    .filter(|run| run.status == Status::Completed)
                    .count()
                    == 2
                    && view.activity_children().len() == 2
                    && view
                        .activity_children()
                        .iter()
                        .all(|child| child.run.status == Status::Running)
                    && view
                        .activity_children()
                        .iter()
                        .all(|child| view.child_session(child.run.session).is_some())
            })
        });
        fixture.runtime.block_on(fixture.child.wait_count(2));
        let children = source.read_with(visual, |view, _| {
            let children = view
                .activity_children()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            assert_ne!(children[0].origin.turn, children[1].origin.turn);
            assert!(
                children
                    .iter()
                    .any(|child| child.run.session == first.run.session)
            );
            assert!(
                children
                    .iter()
                    .all(|child| child.origin.session == fixture.session.id)
            );
            assert!(
                children
                    .iter()
                    .all(|child| child.run.worktree != fixture.session.worktree)
            );
            children
        });
        tap(visual, "live-assets");
        wait(visual, |_| true);
        for child in &children {
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("asset-child-{}", child.run.session).into_boxed_str()
                    ))
                    .is_some()
            );
        }
        tap(visual, "live-assets");
        tap(visual, "live-composer-subagents");
        wait(visual, |_| true);
        let selected = &children[0];
        tap(visual, &format!("subagent-row-{}", selected.run.session));
        wait(visual, |cx| {
            matches!(&shell.read(cx).side_resource,
            Some(SideResource::Child(panel)) if panel.selected == selected.run.session)
        });
        shell.read_with(visual, |shell, cx| {
            let Some(SideResource::Child(panel)) = &shell.side_resource else {
                panic!("child panel expected");
            };
            assert_eq!(
                panel.child().read(cx).binding().worktree,
                Some(selected.run.worktree)
            );
            assert_eq!(
                source.read(cx).input.read(cx).value().as_ref(),
                "Keep my parent draft"
            );
        });
        visual.update(|window, _| window.remove_window());
        drop(shell);
        drop(source);
        let binding = Binding {
            client: fixture.client.clone(),
            defaults: fixture.client.clone(),
            runtime: fixture.runtime.clone(),
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            host: "Fixture".into(),
            project_name: "Background children".into(),
            branch: "main".into(),
        };
        let (view, visual) =
            super::super::assets::tests::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).activity_children().len() == 2);
        assert!(view.read_with(visual, |view, _| view.active().is_none()));
        assert!(visual.debug_bounds("live-composer-subagents").is_some());
        // A foreign origin is a pure projection scope check, not service evidence.
        let canonical = view.read_with(visual, |view, _| view.history.clone());
        view.update(visual, |view, _| {
            let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
            let mut foreign = children[0].clone();
            foreign.origin.session = SessionId::new();
            foreign.run.session = SessionId::new();
            Arc::make_mut(&mut snapshot.page).children.push(foreign);
            assert_eq!(view.activity_children().len(), 2);
            view.history = canonical;
        });
        release.cancel();
        wait(visual, |cx| {
            let view = view.read(cx);
            view.activity_rows().is_empty()
                && view.history.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.page.children.len() == 2
                        && snapshot
                            .page
                            .children
                            .iter()
                            .all(|child| child.run.status == Status::Completed)
                })
        });
        assert!(visual.debug_bounds("live-composer-subagents").is_none());
        assert!(visual.debug_bounds("subagent-picker").is_none());
        view.read_with(visual, |view, _| {
            assert!(view.activity_children().is_empty());
            assert!(view.activity_rows().is_empty());
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert_eq!(
                page.runs
                    .iter()
                    .filter(|run| run.status == Status::Completed)
                    .count(),
                2
            );
            assert_eq!(latest(page.children.iter()).len(), 2);
        });
        tap(visual, "live-assets");
        wait(visual, |_| true);
        assert!(visual.debug_bounds("asset-panel").is_some());
        assert!(visual.debug_bounds("asset-activity-heading").is_some());
        for child in &children {
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("asset-child-{}", child.run.session).into_boxed_str()
                    ))
                    .is_none()
            );
        }
        visual.update(|window, _| window.remove_window());
        drop(view);
        cx.update(|cx| cx.remove_global::<Services>());
        fixture.close();
    }
}
