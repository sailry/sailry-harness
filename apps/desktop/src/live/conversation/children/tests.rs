use super::super::tests::{click, wait};
use super::*;
use crate::conversation::live::subagents::fixture::Fixture;
use core::prelude::v1::test;
use sailry_protocol::conversation::{ApprovalState, Status};

#[gpui::test]
fn local(cx: &mut TestAppContext) {
    tabs(cx, false, false);
}

#[gpui::test]
fn remote(cx: &mut TestAppContext) {
    tabs(cx, true, false);
}

#[gpui::test]
fn local_worktrees(cx: &mut TestAppContext) {
    tabs(cx, false, true);
}

#[gpui::test]
fn remote_worktrees(cx: &mut TestAppContext) {
    tabs(cx, true, true);
}

fn tabs(cx: &mut TestAppContext, remote: bool, isolated: bool) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    let fixture = if isolated {
        Fixture::with_worktrees(remote)
    } else {
        Fixture::new(remote)
    };
    cx.update(|cx| cx.set_global(fixture.services()));
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
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
                snapshot
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
    fixture.start();
    wait(visual, |cx| {
        shell.read(cx).current_chat().is_some_and(|view| {
            let page = fixture.page(fixture.session.id);
            page.children.len() == 2
                && page.children.iter().all(|child| {
                    view.read(cx).child_session(child.run.session).is_some()
                        && fixture
                            .page(child.run.session)
                            .approvals
                            .iter()
                            .any(|approval| approval.state == ApprovalState::Pending)
                })
        })
    });
    let page = fixture.page(fixture.session.id);
    let first = page.children[0].run.session;
    let second = page.children[1].run.session;
    for id in [first, second] {
        assert!(
            visual
                .debug_bounds(Box::leak(format!("live-session-{id}").into_boxed_str()))
                .is_none()
        );
    }
    let source = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
    // Public activity shows the parent; children remain in its subagent panel.
    wait(visual, |cx| {
        shell
            .read(cx)
            .activity_snapshot(target)
            .is_some_and(|snapshot| {
                [first, second]
                    .iter()
                    .all(|id| snapshot.sessions.iter().any(|session| session.id == *id))
            })
    });
    shell.read_with(visual, |shell, _| {
        let active = shell.active_sessions();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].0, target);
        assert_eq!(active[0].1.id, fixture.session.id);
    });
    for id in [first, second] {
        assert!(
            visual
                .debug_bounds(Box::leak(format!("active-session-{id}").into_boxed_str()))
                .is_none()
        );
    }
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("active-session-{}", fixture.session.id).into_boxed_str()
            ))
            .is_some()
    );
    assert!(visual.debug_bounds("activity-open").is_none());
    for id in [first, second] {
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("header-session-{target:?}-{id}").into_boxed_str()
                ))
                .is_none()
        );
    }
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("header-session-{target:?}-{}", fixture.session.id).into_boxed_str()
            ))
            .is_some()
    );
    click(visual, "live-composer-subagents");
    click(
        visual,
        Box::leak(format!("subagent-row-{first}").into_boxed_str()),
    );
    wait(
        visual,
        |cx| matches!(&shell.read(cx).side_resource, Some(SideResource::Child(panel)) if panel.selected == first && panel.child().read(cx).connected()),
    );
    shell.read_with(visual, |shell, cx| {
        let Some(SideResource::Child(panel)) = &shell.side_resource else {
            panic!("child panel expected")
        };
        let target = page.children[0].run.worktree;
        assert_eq!(panel.child().read(cx).binding().worktree, Some(target));
        assert_eq!(
            source.read(cx).binding().worktree,
            Some(fixture.session.worktree)
        );
        if isolated {
            assert_ne!(target, fixture.session.worktree);
        }
    });
    let released = shell.read_with(visual, |shell, _| match &shell.side_resource {
        Some(SideResource::Child(panel)) => panel.child().downgrade(),
        _ => panic!("child panel expected"),
    });
    click(visual, "toggle-details");
    wait(visual, |_| released.upgrade().is_none());
    assert!(
        fixture
            .page(fixture.session.id)
            .children
            .iter()
            .all(|child| child.run.status == Status::Running)
    );
    click(visual, "live-composer-subagents");
    click(
        visual,
        Box::leak(format!("subagent-row-{first}").into_boxed_str()),
    );
    wait(
        visual,
        |cx| matches!(&shell.read(cx).side_resource, Some(SideResource::Child(panel)) if panel.child().read(cx).connected()),
    );
    let first_view = shell.read_with(visual, |shell, _| match &shell.side_resource {
        Some(SideResource::Child(panel)) => panel.child().clone(),
        _ => panic!("child panel expected"),
    });
    visual.update(|_, cx| source.update(cx, |_, cx| cx.emit(Event::Subagent(second))));
    visual.run_until_parked();
    shell.read_with(visual, |shell, _| {
        let Some(SideResource::Child(panel)) = &shell.side_resource else {
            panic!("child panel expected after opening another child")
        };
        assert_eq!(panel.selected, second);
        assert_eq!(panel.tabs.len(), 2);
    });
    wait(
        visual,
        |cx| matches!(&shell.read(cx).side_resource, Some(SideResource::Child(panel)) if panel.child().read(cx).connected()),
    );
    visual.update(|_, cx| source.update(cx, |_, cx| cx.emit(Event::Subagent(second))));
    shell.read_with(visual, |shell, _| {
        let Some(SideResource::Child(panel)) = &shell.side_resource else {
            panic!("child panel expected")
        };
        assert_eq!(panel.tabs.len(), 2);
        assert_eq!(panel.tabs[0].view, first_view);
    });
    assert!(visual.debug_bounds("live-subagent-stop").is_none());
    assert!(visual.debug_bounds("live-subagent-switch").is_none());
    for id in [first, second] {
        let bounds = visual
            .debug_bounds(Box::leak(format!("child-tab-{id}").into_boxed_str()))
            .unwrap();
        assert_eq!(bounds.size.height, px(28.));
        assert!(bounds.size.width <= px(180.));
    }
    // Dismiss the approval overlay before interacting with its enclosing tabs.
    visual.simulate_keystrokes("escape");
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(400));
    visual.run_until_parked();
    click(
        visual,
        Box::leak(format!("child-tab-{first}").into_boxed_str()),
    );
    shell.read_with(visual, |shell, _| {
        let Some(SideResource::Child(panel)) = &shell.side_resource else {
            panic!("child panel expected")
        };
        assert_eq!(panel.selected, first);
        assert_eq!(panel.child(), &first_view);
    });
    visual.simulate_keystrokes("escape");
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(400));
    visual.run_until_parked();
    click(
        visual,
        Box::leak(format!("close-child-{second}").into_boxed_str()),
    );
    shell.read_with(visual, |shell, _| {
        let Some(SideResource::Child(panel)) = &shell.side_resource else {
            panic!("child panel expected")
        };
        assert_eq!(panel.selected, first);
        assert_eq!(panel.tabs.len(), 1);
    });
    visual.update(|_, cx| source.update(cx, |_, cx| cx.emit(Event::Subagent(second))));
    wait(
        visual,
        |cx| matches!(&shell.read(cx).side_resource, Some(SideResource::Child(panel)) if panel.selected == second && panel.child().read(cx).connected()),
    );
    visual.simulate_keystrokes("escape");
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(400));
    visual.run_until_parked();
    visual.update(|window, cx| {
        let focus = shell.read(cx).panel_focus.clone();
        focus.focus(window, cx);
    });
    visual.simulate_keystrokes("secondary-w");
    visual.run_until_parked();
    assert!(shell.read_with(visual, |shell, _| matches!(&shell.side_resource, Some(SideResource::Child(panel)) if panel.selected == first)));
    let released = first_view.downgrade();
    drop(first_view);
    click(
        visual,
        Box::leak(format!("close-child-{first}").into_boxed_str()),
    );
    assert!(
        shell.read_with(visual, |shell, _| shell.side_resource.is_none()
            && !shell.layout.panel_open[0])
    );
    wait(visual, |_| released.upgrade().is_none());
    assert!(source.read_with(visual, |view, _| {
        [first, second]
            .iter()
            .all(|id| view.child(*id).unwrap().run.status == Status::Running)
    }));
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.current_chat().cloned()),
        Some(source.clone())
    );
    visual.update(|_, cx| source.update(cx, |_, cx| cx.emit(Event::Subagent(first))));
    wait(visual, |cx| shell.read(cx).side_resource.is_some());
    visual
        .update(|window, cx| shell.update(cx, |shell, cx| shell.new_live_conversation(window, cx)));
    wait(visual, |cx| shell.read(cx).side_resource.is_none());
    // Split-pane actions reactivate their owner before opening its child.
    visual.update(|_, cx| source.update(cx, |_, cx| cx.emit(Event::Subagent(first))));
    wait(
        visual,
        |cx| matches!(&shell.read(cx).side_resource, Some(SideResource::Child(panel)) if panel.source == source && panel.selected == first),
    );
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.current_chat().cloned()),
        Some(source.clone())
    );
    assert_eq!(
        source.read_with(visual, |view, _| view.child(first).unwrap().run.status),
        Status::Running
    );
    let path = "source #1.txt";
    std::fs::write(
        fixture.directory.path().join("project").join(path),
        "File preview",
    )
    .unwrap();
    if isolated {
        let sailry_protocol::Output::Snapshot(snapshot) =
            fixture.execute(sailry_protocol::Command::Snapshot)
        else {
            panic!("snapshot expected")
        };
        for child in &page.children {
            let tree = snapshot
                .worktrees
                .iter()
                .find(|tree| tree.id == child.run.worktree)
                .unwrap();
            std::fs::write(
                std::path::Path::new(&tree.path).join(path),
                "Child file preview",
            )
            .unwrap();
        }
    }
    let child = shell.read_with(visual, |shell, _| match &shell.side_resource {
        Some(SideResource::Child(panel)) => panel.child().clone(),
        _ => panic!("child panel expected"),
    });
    for view in [&child, &source] {
        let content = if isolated && view == &child {
            "Child file preview"
        } else {
            "File preview"
        };
        visual.update(|_, cx| view.update(cx, |_, cx| cx.emit(Event::File(path.into()))));
        wait(visual, |cx| {
            matches!(&shell.read(cx).side_resource, Some(SideResource::Plugin(panel))
            if panel.read(cx).documents.as_ref().is_some_and(|documents| {
                let documents = documents.read(cx);
                documents.snapshot(cx)["documents"].as_array().unwrap().len() == 1
                    && documents.editor(path).is_some_and(|editor| editor.read(cx).value().as_ref() == content)
            }))
        });
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.current_chat().cloned()),
            Some(source.clone())
        );
    }
    let client = fixture.client.clone();
    let turn = page.runs[0].turn;
    let stopping = fixture.runtime.spawn(async move {
        client
            .execute(client.prepare(Command::StopTurn { turn }))
            .await
    });
    wait(visual, |_| stopping.is_finished());
    fixture.runtime.block_on(stopping).unwrap().unwrap();
    wait(visual, |cx| {
        source
            .read(cx)
            .child(first)
            .is_some_and(|child| child.run.status == Status::Cancelled)
    });
    assert!(!fixture.directory.path().join("project/child.txt").exists());
    assert_eq!(fixture.child.requests.lock().unwrap().len(), 2);
    assert_eq!(fixture.parent.requests.lock().unwrap().len(), 1);
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
