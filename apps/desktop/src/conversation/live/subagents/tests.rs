use super::super::tests::{
    fixture::{init, open, tap},
    wait,
};
use super::*;
use core::prelude::v1::test;
use sailry_protocol::conversation::ApprovalState;

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
                    format!("live-turn-subagents-{}", child.run.session).into_boxed_str()
                ))
                .is_none()
        );
        let row = format!("live-subagent-row-{}", child.run.session);
        let bounds = visual
            .debug_bounds(Box::leak(row.clone().into_boxed_str()))
            .unwrap();
        assert!(bounds.size.width < px(400.));
        assert_eq!(bounds.size.height, px(24.));
        tap(visual, &row);
        assert_eq!(selected.get(), Some(child.run.session));
        assert!(visual.debug_bounds("subagent-picker").is_none());
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
        let turn = fixture.page(fixture.session.id).runs[0].turn;
        let first = fixture
            .page(fixture.session.id)
            .children
            .iter()
            .min_by_key(|child| child.origin.index)
            .unwrap()
            .run
            .session;
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
        tap(visual, &toggle);
        assert!(visual.debug_bounds(list).is_some());
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
        let children_button = visual.debug_bounds("live-composer-subagents").unwrap();
        assert!(children_button.left() >= todo.right());
        assert!((children_button.center().y - todo.center().y).abs() < px(1.));
        let children = fixture.page(fixture.session.id).children;
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
            assert!(visual.debug_bounds("live-chat-input").is_none());
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
            tap(
                visual,
                &format!(
                    "live-approval-{}-{approval}",
                    if index == 0 { "approve" } else { "reject" }
                ),
            );
            wait(visual, |cx| {
                child
                    .read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.page.runs[0].status == Status::Completed)
            });
            assert_eq!(fixture.page(session.id).runs.len(), 1);
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
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
