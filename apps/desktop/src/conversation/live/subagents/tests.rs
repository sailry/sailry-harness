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
            removed.run.turn = TurnId::new();
            removed.run.sequence += 1;
            removed.run.status = Status::Cancelled;
            page.children.push(removed);
            assert_eq!(view.active(), Some(continued));
            assert_eq!(view.child_counts(continued), Some((1, 0, 0, 1)));
            assert_eq!(view.child_rows(continued).len(), 1);
            assert_eq!(
                view.current_children(child.origin.turn)[0].origin,
                child.origin
            );
            cx.notify();
        });
        wait(visual, |_| true);
        assert!(visual.debug_bounds("live-composer-subagents").is_some());
        tap(visual, "live-composer-subagents");
        assert!(visual.debug_bounds("subagent-picker").is_some());
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
                let rows = view.child_rows(child.origin.turn);
                assert_eq!(rows.len(), 1);
                assert_eq!(
                    rows[0].running,
                    matches!(status, Status::Running | Status::Stopping)
                );
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
            parent
                .read(cx)
                .child_rows(fixture.page(fixture.session.id).runs[0].turn)
                .len()
                == 2
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
                .child_rows(turn)
                .into_iter()
                .map(|row| row.name)
                .collect();
            assert!(names.iter().any(|name| name == "Inspect files"));
            assert!(names.iter().any(|name| name == "Check lifecycle"));
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
                assert!(view.composer_activity(cx).is_some());
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
            assert_eq!(view.child_counts(turn), Some((0, 2, 0, 2)));
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
        visual.simulate_keystrokes("down enter");
        wait(visual, |_| true);
        assert_eq!(selected.get(), Some(rows[1].0));
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
