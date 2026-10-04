use super::*;
use core::prelude::v1::test;
use gpui_kit::base::Placement;

mod arrange;

fn document_panel(shell: &crate::shell::Shell, cx: &App) -> Entity<crate::plugins::Panel> {
    match &shell.side_resource {
        Some(crate::resources::SideResource::Plugin(panel))
            if panel.read(cx).documents.is_some() =>
        {
            panel.clone()
        }
        _ => panic!("document panel expected"),
    }
}

struct Content;
impl Render for Content {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("Preview")
    }
}
fn session(index: u8) -> Target {
    Target::Session(HostId([index; 32]), SessionId::new())
}
fn mount(cx: &mut TestAppContext) -> (Entity<Workspaces>, &mut VisualTestContext) {
    let (workspaces, _, visual) = mount_observed(cx);
    (workspaces, visual)
}
fn mount_observed(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspaces>,
    std::rc::Rc<std::cell::Cell<usize>>,
    &mut VisualTestContext,
) {
    struct Observer {
        workspaces: Entity<Workspaces>,
        presses: std::rc::Rc<std::cell::Cell<usize>>,
    }
    impl Render for Observer {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let presses = self.presses.clone();
            div()
                .size_full()
                .on_mouse_down(MouseButton::Left, move |_, _, _| {
                    presses.set(presses.get() + 1)
                })
                .child(self.workspaces.clone())
        }
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let presses = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspaces = cx.new(|_| Workspaces::new());
        entity = Some(workspaces.clone());
        let observer = cx.new(|_| Observer {
            workspaces,
            presses: presses.clone(),
        });
        Root::new(observer, window, cx)
    });
    (entity.unwrap(), presses, visual)
}
fn open(workspaces: &Entity<Workspaces>, target: Target, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let content = cx.new(|_| Content);
        workspaces.update(cx, |workspaces, cx| {
            workspaces.open(
                target,
                "Session".into(),
                "Project".into(),
                content.into(),
                window,
                cx,
            )
        });
    });
    cx.run_until_parked();
}
fn drop_on(
    workspaces: &Entity<Workspaces>,
    target: Target,
    destination: Target,
    placement: Option<Placement>,
    cx: &mut VisualTestContext,
) -> bool {
    cx.update(|window, cx| {
        workspaces.update(cx, |workspaces, cx| {
            let workspace = workspaces.workspace(destination, cx).unwrap();
            let id = PanelId::from(workspaces.panes[&destination].entity_id());
            let node = workspaces.groups[&workspace]
                .area
                .read(cx)
                .layout(DockPlacement::Center)
                .unwrap()
                .find_panel_node(id)
                .unwrap();
            workspaces.drop(
                workspace,
                target,
                DropTarget::new(node, placement),
                window,
                cx,
            )
        })
    })
}
#[gpui::test]
fn close_returns_to_recent(cx: &mut TestAppContext) {
    let (workspaces, visual) = mount(cx);
    let first = session(1);
    let second = session(2);
    let closing = session(3);
    for target in [first, second, closing] {
        open(&workspaces, target, visual);
    }
    visual.update(|window, cx| {
        workspaces.update(cx, |workspaces, cx| {
            workspaces.focus(first, cx);
            workspaces.focus(closing, cx);
            workspaces.close(closing, window, cx);
            assert_eq!(workspaces.active, Some(first));
            workspaces.close(first, window, cx);
            assert_eq!(workspaces.active, Some(second));
        });
    });
}

#[gpui::test]
fn headers_follow_visible_panes(cx: &mut TestAppContext) {
    let (workspaces, presses, visual) = mount_observed(cx);
    let a = session(1);
    let close = Box::leak(format!("pane-close-{a:?}").into_boxed_str());
    open(&workspaces, Target::Draft, visual);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("pane-title-Draft").is_none());
    assert!(visual.debug_bounds("pane-close-Draft").is_none());
    assert!(visual.debug_bounds("pane-header").is_none());
    assert!(workspaces.read_with(visual, |spaces, cx| spaces.single_header(cx).is_some()));
    open(&workspaces, a, visual);
    visual.update(|window, cx| {
        let pane = workspaces.read(cx).panes[&a].clone();
        pane.update(cx, |pane, cx| {
            pane.title = "A long conversation title without a repeated project name "
                .repeat(8)
                .into();
            cx.notify();
        });
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds(close).is_none());
    assert!(visual.debug_bounds("pane-header").is_none());
    assert!(drop_on(
        &workspaces,
        Target::Draft,
        a,
        Some(Placement::Right),
        visual
    ));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(workspaces.read_with(visual, |spaces, cx| spaces.single_header(cx).is_none()));
    let caption = Box::leak(format!("pane-caption-{a:?}").into_boxed_str());
    let project = Box::leak(format!("pane-project-{a:?}").into_boxed_str());
    let caption_bounds = visual.debug_bounds(caption).unwrap();
    assert!(visual.debug_bounds(project).is_none());
    let title = Box::leak(format!("pane-title-{a:?}").into_boxed_str());
    assert_eq!(
        caption_bounds.right(),
        visual.debug_bounds(title).unwrap().right()
    );
    fn drag_title(
        visual: &mut VisualTestContext,
        title: &'static str,
        presses: &std::cell::Cell<usize>,
    ) {
        let start = visual.debug_bounds(title).unwrap().center();
        let end = start + point(px(20.), px(10.));
        let previous = presses.get();
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        assert_eq!(presses.get() - previous, 1);
        // GPUI's test platform has no native window move implementation.
        // Both single and split pane titles bubble to the native window header.
        assert!(!visual.update(|_, cx| cx.has_active_drag()));
        visual.simulate_keystrokes("escape");
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
    }
    let handle = visual.update(|window, _| window.window_handle());
    let wide = visual.debug_bounds(caption).unwrap();
    visual.simulate_window_resize(handle, size(px(760.), px(820.)));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let narrow = visual.debug_bounds(caption).unwrap();
    assert!(narrow.size.width < wide.size.width);
    assert!(narrow.right() <= visual.debug_bounds("pane-header").unwrap().right());
    assert_eq!(
        workspaces.read_with(visual, |spaces, cx| spaces.panes[&a].read(cx).title.len()),
        "A long conversation title without a repeated project name "
            .repeat(8)
            .len()
    );
    drag_title(visual, title, &presses);
    assert!(visual.debug_bounds(close).is_some());
    assert!(visual.debug_bounds("pane-close-Draft").is_some());
    drag_title(visual, title, &presses);
    let handle = Box::leak(format!("pane-drag-{a:?}").into_boxed_str());
    let handle_bounds = visual.debug_bounds(handle).unwrap();
    let close_bounds = visual.debug_bounds(close).unwrap();
    assert_eq!(handle_bounds.size, close_bounds.size);
    assert_eq!(handle_bounds.size.width, handle_bounds.size.height);
    let start = handle_bounds.center();
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(
        start + point(px(20.), px(10.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    assert!(visual.update(|_, cx| cx.has_active_drag()));
    visual.simulate_keystrokes("escape");
    visual.simulate_mouse_up(start, MouseButton::Left, Modifiers::default());
    visual.run_until_parked();
    visual.update(|window, cx| {
        let focus = workspaces.read(cx).panes[&a].read(cx).focus.clone();
        focus.focus(window, cx);
    });
    visual.dispatch_action(ToggleZoom);
    visual.run_until_parked();
    assert!(workspaces.read_with(visual, |workspaces, cx| {
        workspaces
            .active_area(cx)
            .unwrap()
            .read(cx)
            .zoomed_group()
            .is_none()
    }));
    visual.update(|window, cx| {
        workspaces.update(cx, |workspaces, cx| workspaces.close(a, window, cx))
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("pane-close-Draft").is_none());
}

#[gpui::test]
fn split_history_navigation(cx: &mut TestAppContext) {
    use crate::conversation::live::{ChatFixture, View};
    use sailry_protocol::Command;
    use std::time::{Duration, Instant};

    cx.executor().allow_parking();
    for remote in [false, true] {
        let fixture = ChatFixture::with_tools(remote, Vec::new());
        fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: 1,
            message: "Verify navigation".into(),
        });
        let (workspaces, visual) = mount(cx);
        let target = Target::Session(fixture.binding.client.target(), fixture.session.id);
        let handle = visual.update(|window, cx| {
            let view = cx.new(|cx| {
                View::new(
                    fixture.binding.clone(),
                    Some(fixture.session.clone()),
                    window,
                    cx,
                )
            });
            workspaces.update(cx, |workspaces, cx| {
                workspaces.open(
                    target,
                    "Session".into(),
                    "Project".into(),
                    view.into(),
                    window,
                    cx,
                )
            });
            window.window_handle()
        });
        visual.simulate_window_resize(handle, size(px(1100.), px(1000.)));
        fn navigation(visual: &mut VisualTestContext, visible: bool) {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                visual.run_until_parked();
                visual.update(|window, cx| window.draw(cx).clear(cx));
                if visual.debug_bounds("message-navigation").is_some() == visible {
                    return;
                }
                assert!(Instant::now() < deadline, "navigation visibility deadline");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        navigation(visual, true);
        let other = session(2);
        open(&workspaces, other, visual);
        assert!(drop_on(
            &workspaces,
            other,
            target,
            Some(Placement::Bottom),
            visual
        ));
        navigation(visual, false);
        visual.update(|window, cx| {
            workspaces.update(cx, |workspaces, cx| workspaces.close(other, window, cx))
        });
        navigation(visual, true);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn restores_mixed_groups(cx: &mut TestAppContext) {
    let (workspaces, visual) = mount(cx);
    let a = session(1);
    let b = session(2);
    let terminal = Target::Terminal(HostId([2; 32]), WorktreeId::new(), TerminalId::new());
    for target in [a, b, terminal] {
        open(&workspaces, target, visual);
    }
    let original = workspaces.read_with(visual, |workspaces, _| {
        workspaces.panes[&terminal].entity_id()
    });
    assert!(drop_on(&workspaces, b, a, Some(Placement::Right), visual));
    assert!(drop_on(
        &workspaces,
        terminal,
        b,
        Some(Placement::Bottom),
        visual
    ));
    let group = workspaces.read_with(visual, |workspaces, cx| {
        workspaces.workspace(a, cx).unwrap()
    });
    assert_eq!(
        workspaces.read_with(visual, |workspaces, cx| workspaces.members(group, cx).len()),
        3
    );
    let other = session(1);
    open(&workspaces, other, visual);
    assert_ne!(
        workspaces.read_with(visual, |workspaces, cx| workspaces.workspace(other, cx)),
        Some(group)
    );
    visual.update(|_, cx| workspaces.update(cx, |workspaces, cx| workspaces.focus(b, cx)));
    assert_eq!(
        workspaces.read_with(visual, |workspaces, cx| workspaces
            .workspace(workspaces.active.unwrap(), cx)),
        Some(group)
    );
    assert!(drop_on(&workspaces, terminal, a, None, visual));
    assert_eq!(
        workspaces.read_with(visual, |workspaces, _| workspaces.panes[&terminal]
            .entity_id()),
        original
    );
    assert!(drop_on(&workspaces, other, b, None, visual));
    assert_ne!(
        workspaces.read_with(visual, |workspaces, cx| workspaces.workspace(b, cx)),
        Some(group)
    );
    assert_eq!(
        workspaces.read_with(visual, |workspaces, cx| workspaces.members(group, cx).len()),
        3
    );
    visual.update(|window, cx| {
        workspaces.update(cx, |workspaces, cx| workspaces.close(terminal, window, cx))
    });
    assert!(!workspaces.read_with(visual, |workspaces, _| workspaces.contains(terminal)));
    assert_eq!(
        workspaces.read_with(visual, |workspaces, cx| workspaces.members(group, cx).len()),
        2
    );
}
#[gpui::test]
fn split_capacity(cx: &mut TestAppContext) {
    let (workspaces, visual) = mount(cx);
    let targets: Vec<_> = (0..10).map(session).collect();
    for target in &targets {
        open(&workspaces, *target, visual);
    }
    for target in &targets[1..9] {
        assert!(drop_on(
            &workspaces,
            *target,
            targets[0],
            Some(Placement::Right),
            visual
        ));
    }
    assert!(!drop_on(
        &workspaces,
        targets[9],
        targets[0],
        Some(Placement::Bottom),
        visual
    ));
    assert!(drop_on(
        &workspaces,
        targets[1],
        targets[0],
        Some(Placement::Bottom),
        visual
    ));
    assert!(drop_on(&workspaces, targets[9], targets[0], None, visual));
    let count = workspaces.read_with(visual, |workspaces, cx| {
        workspaces
            .members(workspaces.workspace(targets[9], cx).unwrap(), cx)
            .len()
    });
    assert_eq!(count, LIMIT);
}

#[gpui::test]
fn preserves_unreadable_layout(cx: &mut TestAppContext) {
    let (workspaces, visual) = mount(cx);
    let original = serde_json::json!({ "invalid": true });
    visual.update(|_, cx| {
        crate::preferences::update(cx, |data| data.workspaces = Some(original.clone()));
        workspaces.update(cx, |workspaces, _| workspaces.suspend_saving());
    });
    let a = session(1);
    let b = session(2);
    open(&workspaces, a, visual);
    open(&workspaces, b, visual);
    visual.update(|_, cx| {
        workspaces.update(cx, |workspaces, cx| {
            workspaces.persist(cx);
            assert!(workspaces.save.is_none());
            assert_eq!(crate::preferences::data(cx).workspaces, Some(original));
        })
    });
    assert!(drop_on(&workspaces, b, a, Some(Placement::Right), visual));
    visual.update(|_, cx| {
        workspaces.update(cx, |workspaces, cx| {
            assert!(workspaces.saving);
            workspaces.saved(cx).validate().unwrap();
        })
    });
}

#[gpui::test]
fn restores_saved_hierarchy(cx: &mut TestAppContext) {
    let (workspaces, visual) = mount(cx);
    let a = session(1);
    let b = session(2);
    let terminal = Target::Terminal(HostId([2; 32]), WorktreeId::new(), TerminalId::new());
    for target in [a, b, terminal] {
        open(&workspaces, target, visual);
    }
    assert!(drop_on(&workspaces, b, a, Some(Placement::Right), visual));
    assert!(drop_on(
        &workspaces,
        terminal,
        b,
        Some(Placement::Bottom),
        visual
    ));
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let saved = workspaces.read_with(visual, |workspaces, cx| workspaces.saved(cx));
    assert_eq!(saved.targets(), [a, b, terminal]);
    let encoded = serde_json::to_string(&saved).unwrap();
    assert!(encoded.contains("Terminal"));
    let saved: Saved = serde_json::from_str(&encoded).unwrap();
    saved.validate().unwrap();
    let mut duplicate = saved.clone();
    duplicate.groups.extend(saved.groups.clone());
    assert!(duplicate.validate().is_err());
    let mut cyclic = saved.clone();
    cyclic.parents.push((a, terminal));
    assert!(cyclic.validate().is_err());
    visual.update(|window, cx| {
        let restored = cx.new(|_| Workspaces::new());
        restored.update(cx, |restored, cx| {
            restored.restore(&saved, window, cx);
            assert_eq!(restored.workspace(a, cx), restored.workspace(b, cx));
            assert_eq!(restored.workspace(a, cx), restored.workspace(terminal, cx));
            assert_eq!(restored.parent(b), Some(a));
            assert_eq!(restored.parent(terminal), Some(b));
            assert_eq!(restored.panes.len(), 3);
            let original = restored.panes[&a].entity_id();
            let content = cx.new(|_| Content);
            restored.open(
                a,
                "Reconnected".into(),
                "Project".into(),
                content.clone().into(),
                window,
                cx,
            );
            assert_eq!(restored.panes[&a].entity_id(), original);
            assert_eq!(
                restored.panes[&a].read(cx).content.entity_id(),
                content.entity_id()
            );
            assert_eq!(restored.workspace(a, cx), restored.workspace(b, cx));
        });
    });
}

#[gpui::test]
fn restores_active_terminal(cx: &mut TestAppContext) {
    use crate::{activity::fixture::Fixture, preview::Page, shell::Shell};
    use sailry_client::Client;
    use sailry_protocol::{
        Command, Output,
        terminal::{Launch, Status, Viewport},
    };
    use std::time::{Duration, Instant};

    cx.executor().allow_parking();
    let fixture = Fixture::new();
    for index in 0..2 {
        let (workspaces, visual) = mount(cx);
        let node = fixture.nodes[index].id();
        let session = Target::Session(node, fixture.sessions[index].id);
        let tree = fixture.sessions[index].worktree;
        let client = Client::new(if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        });
        let appearance = visual.update(|_, cx| crate::theme::terminal(cx));
        let Output::Terminal(info) = fixture
            .runtime
            .block_on(
                client.execute(client.prepare(Command::CreateTerminal(Launch {
                    worktree: tree,
                    viewport: Viewport {
                        columns: 80,
                        rows: 24,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                    appearance,
                }))),
            )
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let terminal = Target::Terminal(node, tree, info.id);
        for target in [session, terminal] {
            open(&workspaces, target, visual);
        }
        assert!(drop_on(
            &workspaces,
            terminal,
            session,
            Some(Placement::Right),
            visual
        ));
        let saved = workspaces.read_with(visual, |workspaces, cx| workspaces.saved(cx));
        assert_eq!(saved.active, Some(terminal));
        visual.update(|window, _| window.remove_window());
        drop(workspaces);

        cx.update(|cx| {
            crate::shell::init(cx);
            cx.set_global(fixture.services());
            crate::preferences::update(cx, |preferences| {
                preferences.workspaces = Some(serde_json::to_value(&saved).unwrap());
            });
        });
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            entity = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = entity.unwrap();
        let placeholder = shell.read_with(visual, |shell, cx| {
            shell.splits.read(cx).panes[&terminal].entity_id()
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            visual.run_until_parked();
            let ready = visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                let shell = shell.read(cx);
                let splits = shell.splits.read(cx);
                assert_eq!(shell.page, Page::Terminal);
                assert_eq!(splits.active, Some(terminal));
                assert!(
                    !splits.contains(Target::Draft),
                    "render must not replace the restored terminal with a draft"
                );
                assert_eq!(splits.panes[&terminal].entity_id(), placeholder);
                assert_eq!(
                    splits.workspace(session, cx),
                    splits.workspace(terminal, cx)
                );
                assert_eq!(splits.parent(terminal), Some(session));
                let package_ready = splits.panes[&terminal]
                    .read(cx)
                    .content
                    .clone()
                    .downcast::<crate::plugins::Panel>()
                    .ok()
                    .is_some_and(|panel| {
                        Some(panel.entity_id()) == shell.plugin_panes.panel_id(terminal)
                    });
                let terminal_ready = package_ready
                    && shell
                        .plugin_panes
                        .terminal(terminal, cx)
                        .is_some_and(|view| {
                            view.read(cx).info().is_some_and(|current| {
                                current.id == info.id
                                    && current.worktree == Some(tree)
                                    && current.status == Status::Running
                            })
                        });
                let conversation_ready = shell
                    .chats
                    .views
                    .get(&(node, fixture.sessions[index].id))
                    .is_some_and(|view| view.read(cx).connected());
                let live = shell.live.as_ref().unwrap();
                terminal_ready && conversation_ready && live.selected == node && live.view.connected
            });
            if ready {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "active terminal hydration deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        shell.read_with(visual, |shell, cx| {
            let live = shell.live.as_ref().unwrap();
            assert_eq!(live.project, fixture.sessions[index].project);
            assert_eq!(live.selected_worktree().unwrap().id, tree);
            shell.splits.read(cx).saved(cx).validate().unwrap();
        });
        visual.update(|window, _| window.remove_window());
        drop(shell);
    }
    fixture.close();
}

#[gpui::test]
fn detach_and_replace(cx: &mut TestAppContext) {
    let (workspaces, visual) = mount(cx);
    let a = session(1);
    let b = session(2);
    let c = session(3);
    let d = session(4);
    for target in [a, b, c, d] {
        open(&workspaces, target, visual);
    }
    assert!(drop_on(&workspaces, b, a, Some(Placement::Left), visual));
    assert!(drop_on(&workspaces, c, b, Some(Placement::Bottom), visual));
    workspaces.read_with(visual, |workspaces, _| {
        assert_eq!(workspaces.parent(b), Some(a));
        assert_eq!(workspaces.parent(c), Some(b));
    });
    // Replacing a root moves its visual subtree without changing any resource.
    assert!(drop_on(&workspaces, d, a, None, visual));
    workspaces.read_with(visual, |workspaces, cx| {
        assert_eq!(workspaces.parent(b), Some(d));
        assert_eq!(workspaces.parent(c), Some(b));
        assert_ne!(workspaces.workspace(a, cx), workspaces.workspace(d, cx));
        workspaces.saved(cx).validate().unwrap();
    });
    let entity = workspaces.read_with(visual, |workspaces, _| workspaces.panes[&b].entity_id());
    visual.update(|window, cx| {
        workspaces.update(cx, |workspaces, cx| {
            workspaces.detach(b, window, cx);
            assert_eq!(workspaces.panes[&b].entity_id(), entity);
            assert_eq!(workspaces.parent(b), None);
            assert_eq!(workspaces.parent(c), Some(d));
            assert_ne!(workspaces.workspace(b, cx), workspaces.workspace(d, cx));
            workspaces.saved(cx).validate().unwrap();
        })
    });
    // Moving an ancestor next to its child never creates a sidebar cycle.
    assert!(drop_on(&workspaces, d, c, Some(Placement::Right), visual));
    workspaces.read_with(visual, |workspaces, cx| {
        assert_eq!(workspaces.parent(c), None);
        assert_eq!(workspaces.parent(d), Some(c));
        workspaces.saved(cx).validate().unwrap();
    });
}

#[gpui::test]
fn drag_and_terminal_close(cx: &mut TestAppContext) {
    use crate::{activity::fixture::Fixture, preview::Page, shell::Shell};
    use sailry_client::Client;
    use sailry_protocol::{
        Command, Output,
        terminal::{Launch, Status, Viewport},
    };
    use std::time::{Duration, Instant};
    cx.executor().allow_parking();
    let fixture = Fixture::new();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            cx.run_until_parked();
            if cx.update(|window, cx| {
                window.draw(cx).clear(cx);
                predicate(cx)
            }) {
                return;
            }
            assert!(Instant::now() < deadline, "split integration deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    wait(visual, |cx| {
        shell.read(cx).live.as_ref().unwrap().hosts.len() == 2
    });
    for index in 0..2 {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.park_session_panel(window, cx);
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.nodes[index].id(), cx);
            })
        });
        wait(visual, |cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.sessions[index].clone(), window, cx)
            })
        });
        wait(visual, |cx| {
            shell.read(cx).current_chat().unwrap().read(cx).connected()
        });
        let session = Target::Session(fixture.nodes[index].id(), fixture.sessions[index].id);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_destination(crate::resources::launcher::Destination::Files, window, cx)
            })
        });
        let files = shell.read_with(visual, |shell, cx| document_panel(shell, cx).entity_id());
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_live_terminals(window, cx);
                assert!(shell.side_resource.is_none());
                shell.activate_session(
                    crate::shell::session_scope::Key::Session(
                        fixture.nodes[index].id(),
                        fixture.sessions[index].id,
                    ),
                    window,
                    cx,
                );
                assert_eq!(document_panel(shell, cx).entity_id(), files);
            });
        });
        // Hide resources for the drag, retaining this session's own panel state.
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.layout.panel_open[0] = false;
                cx.notify();
            })
        });
        let transport = if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        };
        let client = Client::new(transport);
        let appearance = visual.update(|_, cx| crate::theme::terminal(cx));
        let Output::Terminal(info) = fixture
            .runtime
            .block_on(
                client.execute(client.prepare(Command::CreateTerminal(Launch {
                    worktree: fixture.sessions[index].worktree,
                    viewport: Viewport {
                        columns: 80,
                        rows: 24,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                    appearance,
                }))),
            )
            .unwrap()
        else {
            panic!("terminal expected")
        };
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .terminals
                .iter()
                .any(|entry| entry.id == info.id)
        });
        let terminal = Target::Terminal(
            fixture.nodes[index].id(),
            fixture.sessions[index].worktree,
            info.id,
        );
        let terminal_row = visual
            .debug_bounds(Box::leak(
                format!("live-terminal-{}", info.id).into_boxed_str(),
            ))
            .unwrap();
        visual.simulate_click(terminal_row.center(), Modifiers::default());
        wait(visual, |cx| shell.read(cx).page == Page::Terminal);
        assert!(visual.debug_bounds("pane-header").is_none());
        let header = visual.debug_bounds("shell-module-header").unwrap();
        let title = visual.debug_bounds("single-pane-title").unwrap();
        let close = visual
            .debug_bounds(Box::leak(
                format!("pane-close-{terminal:?}").into_boxed_str(),
            ))
            .unwrap();
        assert!(title.top() >= header.top() && title.bottom() <= header.bottom());
        assert!(close.top() >= header.top() && close.bottom() <= header.bottom());
        let session_row = visual
            .debug_bounds(Box::leak(
                format!("live-session-{}", fixture.sessions[index].id).into_boxed_str(),
            ))
            .unwrap();
        visual.simulate_click(session_row.center(), Modifiers::default());
        wait(visual, |cx| shell.read(cx).page == Page::Conversation);
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.layout.panel_open[0] = false;
                cx.notify();
            })
        });
        let source = visual
            .debug_bounds(Box::leak(
                format!("live-terminal-{}", info.id).into_boxed_str(),
            ))
            .unwrap()
            .center();
        let destination = shell.read_with(visual, |shell, cx| {
            shell.splits.read(cx).panes[&session].read(cx).bounds.get()
        });
        let end = point(destination.right() - px(20.), destination.center().y);
        visual.simulate_mouse_down(source, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(
            source + point(px(10.), px(10.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        wait(visual, |cx| {
            shell.read(cx).splits.read(cx).contains(terminal)
        });
        shell.read_with(visual, |shell, cx| {
            assert_eq!(
                shell.splits.read(cx).workspace(session, cx),
                shell.splits.read(cx).workspace(terminal, cx)
            );
            assert_eq!(shell.page, Page::Terminal);
            assert!(shell.side_resource.is_none());
            assert_eq!(shell.splits.read(cx).parent(terminal), Some(session));
        });
        let session_selector =
            Box::leak(format!("live-session-{}", fixture.sessions[index].id).into_boxed_str());
        let terminal_selector = Box::leak(format!("live-terminal-{}", info.id).into_boxed_str());
        let guide = Box::leak(format!("live-terminal-{}-guide", info.id).into_boxed_str());
        let session_guide = Box::leak(
            format!("live-session-{}-guide", fixture.sessions[index].id).into_boxed_str(),
        );
        assert!(
            visual.debug_bounds(terminal_selector).unwrap().top()
                > visual.debug_bounds(session_selector).unwrap().top()
        );
        let folder = Box::leak(format!("split-folder-{session:?}").into_boxed_str());
        let folder_guide = Box::leak(format!("split-folder-{session:?}-guide").into_boxed_str());
        assert_eq!(
            visual.debug_bounds(guide).unwrap().left(),
            visual.debug_bounds(session_guide).unwrap().left()
        );
        assert_eq!(
            visual.debug_bounds(session_guide).unwrap().left()
                - visual.debug_bounds(folder_guide).unwrap().left(),
            px(18.)
        );
        let disclosure = Box::leak(format!("split-disclosure-{session:?}").into_boxed_str());
        let resting_arrow = visual.debug_bounds(disclosure).unwrap();
        let position = visual.debug_bounds(folder).unwrap().center();
        visual.simulate_mouse_move(position, None, Modifiers::default());
        wait(visual, |_| true);
        let archive = Box::leak(
            format!("session-sidebar-archive-{}", fixture.sessions[index].id).into_boxed_str(),
        );
        let arrow = visual.debug_bounds(disclosure).unwrap();
        assert!(visual.debug_bounds(archive).is_none());
        assert_eq!(arrow, resting_arrow);
        let project = fixture.sessions[index].project.unwrap();
        let project_arrow = Box::leak(format!("project-disclosure-{project}").into_boxed_str());
        for selector in ["hosts-disclosure", "projects-disclosure", project_arrow] {
            let reference = visual.debug_bounds(selector).unwrap();
            assert_eq!(arrow.center().x, reference.center().x, "{selector}");
            assert_eq!(arrow.right(), reference.right(), "{selector}");
        }
        assert_eq!(
            arrow.right(),
            visual.debug_bounds(folder).unwrap().right() - px(8.)
        );
        let project_selector = Box::leak(format!("live-project-{project}").into_boxed_str());
        let position =
            visual.debug_bounds(project_selector).unwrap().origin + point(px(70.), px(10.));
        visual.simulate_click(position, Modifiers::default());
        wait(visual, |cx| {
            !shell
                .read(cx)
                .sidebar
                .project_open(fixture.nodes[index].id(), project)
        });
        assert!(visual.debug_bounds(session_selector).is_none());
        assert!(visual.debug_bounds(terminal_selector).is_none());
        visual.simulate_click(position, Modifiers::default());
        wait(visual, |cx| {
            shell
                .read(cx)
                .sidebar
                .project_open(fixture.nodes[index].id(), project)
        });
        assert!(visual.debug_bounds(terminal_selector).is_some());
        // Focus the session before opening its retained resources from the outer header.
        let pane = Box::leak(format!("pane-{session:?}").into_boxed_str());
        let content = visual.debug_bounds(pane).unwrap();
        visual.simulate_click(content.origin + point(px(8.), px(8.)), Modifiers::default());
        wait(visual, |cx| shell.read(cx).page == Page::Conversation);
        let control = visual.debug_bounds("toggle-details").unwrap();
        let header = visual.debug_bounds("shell-module-header").unwrap();
        assert!(control.top() >= header.top() && control.bottom() <= header.bottom());
        visual.simulate_click(control.center(), Modifiers::default());
        wait(visual, |cx| shell.read(cx).layout.panel_open[0]);
        assert!(shell.read_with(visual, |shell, _| shell.layout.panel_open[0]));
        assert_eq!(
            shell.read_with(visual, |shell, cx| document_panel(shell, cx).entity_id()),
            files
        );
        let close = visual
            .debug_bounds(Box::leak(
                format!("pane-close-{terminal:?}").into_boxed_str(),
            ))
            .unwrap()
            .center();
        visual.simulate_click(close, Modifiers::default());
        wait(visual, |cx| {
            let splits = shell.read(cx).splits.read(cx);
            splits.workspace(terminal, cx) != splits.workspace(session, cx)
        });
        let Output::Snapshot(detached) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected");
        };
        assert!(
            detached
                .terminals
                .iter()
                .any(|entry| entry.id == info.id && entry.status != Status::Closed)
        );
        let row = visual.debug_bounds(terminal_selector).unwrap().center();
        visual.simulate_click(row, Modifiers::default());
        wait(visual, |cx| {
            shell.read(cx).splits.read(cx).active == Some(terminal)
        });
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("pane-project-{terminal:?}").into_boxed_str()
                ))
                .is_none()
        );
        let close = visual
            .debug_bounds(Box::leak(
                format!("pane-close-{terminal:?}").into_boxed_str(),
            ))
            .unwrap()
            .center();
        visual.simulate_click(close, Modifiers::default());
        wait(visual, |cx| {
            !shell.read(cx).splits.read(cx).contains(terminal)
        });
        let Output::Snapshot(snapshot) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert!(
            snapshot
                .terminals
                .iter()
                .any(|entry| entry.id == info.id && entry.status == Status::Closed)
        );
        assert_eq!(
            shell.read_with(visual, |shell, cx| document_panel(shell, cx).entity_id()),
            files
        );
        assert!(
            snapshot
                .sessions
                .iter()
                .any(|entry| entry.id == fixture.sessions[index].id)
        );
        let position = visual.debug_bounds("toggle-details").unwrap().center();
        visual.simulate_click(position, Modifiers::default());
        wait(visual, |cx| !shell.read(cx).layout.panel_open[0]);
    }
    let a = Target::Session(fixture.nodes[0].id(), fixture.sessions[0].id);
    let b = Target::Session(fixture.nodes[1].id(), fixture.sessions[1].id);
    let splits = shell.read_with(visual, |shell, _| shell.splits.clone());
    assert!(drop_on(&splits, b, a, Some(Placement::Right), visual));
    let saved = splits.read_with(visual, |splits, cx| {
        serde_json::to_value(splits.saved(cx)).unwrap()
    });
    let restored = visual.update(|window, cx| {
        crate::preferences::update(cx, |preferences| preferences.workspaces = Some(saved));
        cx.new(|cx| Shell::new(window, cx))
    });
    wait(visual, |cx| restored.read(cx).chats.views.len() == 2);
    restored.read_with(visual, |shell, cx| {
        let splits = shell.splits.read(cx);
        assert_eq!(splits.workspace(a, cx), splits.workspace(b, cx));
        for index in 0..2 {
            let view = shell.chats.views[&(fixture.nodes[index].id(), fixture.sessions[index].id)]
                .read(cx);
            assert_eq!(view.binding().client.target(), fixture.nodes[index].id());
            assert_eq!(
                view.binding().worktree,
                Some(fixture.sessions[index].worktree)
            );
        }
    });
    drop(restored);
    visual.update(|window, cx| {
        let focus = splits.read(cx).panes[&b].read(cx).focus.clone();
        focus.focus(window, cx);
    });
    visual.simulate_modifiers_change(Keystroke::parse("secondary-w").unwrap().modifiers);
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(1));
    visual.run_until_parked();
    visual.simulate_keystrokes("secondary-w");
    wait(visual, |cx| {
        splits.read(cx).workspace(a, cx) != splits.read(cx).workspace(b, cx)
    });
    assert!(!visual.has_pending_prompt());
    assert!(splits.read_with(visual, |splits, _| splits.contains(b)));
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.splits.update(cx, |splits, cx| splits.focus(b, cx));
            shell.activate_pane(b, window, cx);
        });
    });
    visual.simulate_keystrokes("secondary-w");
    assert!(visual.has_pending_prompt());
    visual.simulate_prompt_answer(&crate::tr("settings_cancel"));
    visual.run_until_parked();
    assert!(splits.read_with(visual, |splits, _| splits.contains(b)));
    visual.simulate_keystrokes("secondary-w");
    assert!(visual.has_pending_prompt());
    visual.simulate_prompt_answer(&crate::tr("close"));
    wait(visual, |cx| !splits.read(cx).contains(b));
    assert!(splits.read_with(visual, |splits, _| splits.contains(a)));
    let client = Client::new(fixture.nodes[1].local());
    let Output::Snapshot(snapshot) = fixture
        .runtime
        .block_on(client.execute(client.prepare(Command::Snapshot)))
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|session| session.id == fixture.sessions[1].id)
    );
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
