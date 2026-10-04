use super::*;
use crate::panes::Target;
use sailry_protocol::terminal::{Launch, Viewport};

mod empty;
mod indicators;
mod spacing;

fn selector(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

pub(super) fn hover(cx: &mut VisualTestContext, row: &'static str) -> Bounds<Pixels> {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx.debug_bounds(row).unwrap();
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    let branch = selector(format!("{row}-branch"));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if cx.debug_bounds(branch).is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "sidebar location deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("tooltip-popup").is_none());
    assert!(
        cx.debug_bounds(selector(format!("{row}-preview")))
            .is_none()
    );
    assert_eq!(cx.debug_bounds(row), Some(bounds));
    cx.debug_bounds(branch).unwrap()
}

fn drag(cx: &mut VisualTestContext, source: &str, destination: Point<Pixels>) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let start = cx.debug_bounds(selector(source.into())).unwrap().center();
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        start + point(px(10.), px(10.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    cx.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
}

#[gpui::test]
fn groups_and_headings(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1600.), px(1000.)));
    for index in 0..2 {
        let original = &fixture.sessions[index];
        let node = fixture.nodes[index].id();
        open(&shell, visual, &fixture, index, original);
        let transport = if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        };
        let client = Client::new(transport);
        let mut sessions = Vec::new();
        for _ in 0..2 {
            let Output::Session(session) = fixture
                .runtime
                .block_on(client.execute(client.prepare(Command::CreateSession {
                    project: original.project,
                    worktree: Some(original.worktree),
                    config: Some(original.config.clone()),
                })))
                .unwrap()
            else {
                panic!("session expected")
            };
            sessions.push(session);
        }
        let appearance = visual.update(|_, cx| crate::theme::terminal(cx));
        let Output::Terminal(terminal) = fixture
            .runtime
            .block_on(
                client.execute(client.prepare(Command::CreateTerminal(Launch {
                    worktree: original.worktree,
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
            let snapshot = shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap();
            snapshot
                .sessions
                .iter()
                .any(|session| session.id == sessions[1].id)
                && snapshot.terminals.iter().any(|info| info.id == terminal.id)
        });
        let root = Target::Session(node, original.id);
        let second = Target::Session(node, sessions[0].id);
        let fourth = Target::Session(node, sessions[1].id);
        let third = Target::Terminal(node, original.worktree, terminal.id);
        let root_row = selector(format!("live-session-{}", original.id));
        let second_row = selector(format!("live-session-{}", sessions[0].id));
        let fourth_row = selector(format!("live-session-{}", sessions[1].id));
        let third_row = selector(format!("live-terminal-{}", terminal.id));
        let folder = selector(format!("split-folder-{root:?}"));
        click(&shell, visual, "live-chat-input".into());
        visual.simulate_input("Keep this draft");
        let pane = visual
            .debug_bounds(selector(format!("pane-{root:?}")))
            .unwrap();
        drag(
            visual,
            second_row,
            point(pane.right() - px(20.), pane.center().y),
        );
        wait(visual, |cx| {
            shell.read(cx).splits.read(cx).ordered_members(root, cx) == [root, second]
        });
        let chat = shell.read_with(visual, |shell, _| {
            shell.chats.views[&(node, original.id)].clone()
        });
        let split_id = shell.read_with(visual, |shell, cx| {
            shell.splits.read(cx).pane(root).unwrap().entity_id()
        });

        // The folder only changes navigation visibility, leaving the active pane intact.
        let active = shell.read_with(visual, |shell, cx| shell.splits.read(cx).active);
        let page = shell.read_with(visual, |shell, _| shell.page);
        click(&shell, visual, folder.into());
        wait(visual, |cx| !shell.read(cx).sidebar.group_open(root));
        assert!(visual.debug_bounds(root_row).is_none());
        assert!(visual.debug_bounds(second_row).is_none());
        shell.read_with(visual, |shell, cx| {
            assert_eq!(shell.splits.read(cx).active, active);
            assert_eq!(shell.page, page);
        });
        assert!(
            visual
                .debug_bounds(selector(format!("pane-{second:?}")))
                .is_some()
        );
        let destination = visual.debug_bounds(folder).unwrap().center();
        drag(visual, third_row, destination);
        wait(visual, |cx| {
            shell.read(cx).splits.read(cx).ordered_members(root, cx) == [root, second, third]
        });
        assert!(shell.read_with(visual, |shell, _| shell.sidebar.group_open(root)));

        // A drop on any member appends to the whole group, with no new ancestry level.
        let destination = visual.debug_bounds(third_row).unwrap().center();
        drag(visual, fourth_row, destination);
        wait(visual, |cx| {
            shell.read(cx).splits.read(cx).ordered_members(root, cx)
                == [root, second, third, fourth]
        });
        let guides: Vec<_> = [fourth_row, second_row, third_row, root_row]
            .map(|row| {
                visual
                    .debug_bounds(selector(format!("{row}-guide")))
                    .unwrap()
            })
            .into();
        for pair in guides.windows(2) {
            assert_eq!(pair[0].left(), pair[1].left());
            assert!(pair[0].top() < pair[1].top());
        }
        let panes = [root, second, third, fourth].map(|target| {
            visual
                .debug_bounds(selector(format!("pane-{target:?}")))
                .unwrap()
        });
        assert_eq!(panes[0].top(), panes[1].top());
        assert_eq!(panes[2].top(), panes[3].top());
        assert!(panes[0].bottom() < panes[2].top());
        assert!(
            panes[0].right() <= panes[1].left(),
            "overlapping columns: {panes:?}"
        );
        shell.read_with(visual, |shell, cx| {
            let splits = shell.splits.read(cx);
            assert_eq!(splits.pane(root).unwrap().entity_id(), split_id);
            splits.saved(cx).validate().unwrap();
            assert_eq!(shell.chats.views[&(node, original.id)], chat);
        });
        assert_eq!(
            chat.read_with(visual, |chat, cx| chat.draft(cx)),
            "Keep this draft"
        );

        let first = visual.debug_bounds(fourth_row).unwrap();
        drag(
            visual,
            root_row,
            point(first.center().x, first.top() + px(2.)),
        );
        wait(visual, |cx| {
            !shell.read(cx).sidebar.order_pending
                && shell
                    .read(cx)
                    .live
                    .as_ref()
                    .unwrap()
                    .view
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .sessions[0]
                    .id
                    == original.id
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual.debug_bounds(root_row).unwrap().top()
                < visual.debug_bounds(fourth_row).unwrap().top()
        );
        assert_eq!(
            shell.read_with(visual, |shell, cx| shell
                .splits
                .read(cx)
                .ordered_members(root, cx)),
            [root, second, third, fourth]
        );

        let disclosure = selector(format!("split-disclosure-{root:?}"));
        click(&shell, visual, disclosure.into());
        wait(visual, |cx| !shell.read(cx).sidebar.group_open(root));
        click(&shell, visual, disclosure.into());
        wait(visual, |cx| shell.read(cx).sidebar.group_open(root));
        assert!(visual.debug_bounds(fourth_row).is_some());

        // Every member, including the original root, opens its pane without collapsing.
        for (row, target) in [(root_row, root), (third_row, third), (second_row, second)] {
            click(&shell, visual, row.into());
            wait(visual, |cx| {
                shell.read(cx).splits.read(cx).active == Some(target)
            });
            assert!(shell.read_with(visual, |shell, _| shell.sidebar.group_open(root)));
            assert!(visual.debug_bounds(fourth_row).is_some());
        }

        // Toggling an inactive folder must not navigate away from another page.
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.page = Page::Project;
                cx.notify();
            })
        });
        click(&shell, visual, folder.into());
        click(&shell, visual, folder.into());
        shell.read_with(visual, |shell, cx| {
            assert_eq!(shell.page, Page::Project);
            assert_eq!(shell.splits.read(cx).active, Some(second));
        });
        click(&shell, visual, root_row.into());
        wait(visual, |cx| {
            shell.read(cx).splits.read(cx).active == Some(root)
        });

        let project = selector(format!("live-project-{}", original.project.unwrap()));
        let host = selector(format!(
            "live-host-{}",
            node.0
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ));
        click(&shell, visual, "hosts-toggle".into());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(host).is_none());
        assert!(visual.debug_bounds(project).is_some());
        click(&shell, visual, "hosts-toggle".into());
        click(&shell, visual, "projects-toggle".into());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(host).is_some());
        assert!(visual.debug_bounds(project).is_none());
        assert!(visual.debug_bounds(root_row).is_none());
        assert!(
            visual
                .debug_bounds(selector(format!("pane-{root:?}")))
                .is_some()
        );
        click(&shell, visual, "projects-toggle".into());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(fourth_row).is_some());
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

#[gpui::test]
fn session_drag_order(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    for index in 0..2 {
        let original = &fixture.sessions[index];
        let node = fixture.nodes[index].id();
        open(&shell, visual, &fixture, index, original);
        let client = Client::new(if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        });
        let mut sessions = vec![original.id];
        for _ in 0..2 {
            let Output::Session(session) = fixture
                .runtime
                .block_on(client.execute(client.prepare(Command::CreateSession {
                    project: original.project,
                    worktree: Some(original.worktree),
                    config: Some(original.config.clone()),
                })))
                .unwrap()
            else {
                panic!("session expected")
            };
            sessions.push(session.id);
        }
        let ids = |cx: &App| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .sessions
                .iter()
                .map(|session| session.id)
                .collect::<Vec<_>>()
        };
        wait(visual, |cx| {
            ids(cx) == [sessions[2], sessions[1], sessions[0]]
        });
        let rows: Vec<_> = sessions
            .iter()
            .map(|id| selector(format!("live-session-{id}")))
            .collect();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let newest = visual.debug_bounds(rows[2]).unwrap();
        let oldest = visual.debug_bounds(rows[0]).unwrap();
        assert!(newest.top() < oldest.top());
        drag(
            visual,
            rows[0],
            point(newest.center().x, newest.top() + px(2.)),
        );
        wait(visual, |cx| {
            ids(cx) == [sessions[0], sessions[2], sessions[1]]
                && !shell.read(cx).sidebar.order_pending
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual.debug_bounds(rows[0]).unwrap().top()
                < visual.debug_bounds(rows[2]).unwrap().top()
        );
        let last = visual.debug_bounds(rows[1]).unwrap();
        drag(
            visual,
            rows[0],
            point(last.center().x, last.bottom() - px(2.)),
        );
        wait(visual, |cx| {
            ids(cx) == [sessions[2], sessions[1], sessions[0]]
                && !shell.read(cx).sidebar.order_pending
        });
        let Output::Snapshot(snapshot) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .map(|session| session.id)
                .collect::<Vec<_>>(),
            [sessions[2], sessions[1], sessions[0]]
        );
        assert!(
            snapshot
                .sessions
                .iter()
                .all(|session| session.revision == 1)
        );
        assert_eq!(
            shell.read_with(visual, |shell, cx| shell
                .splits
                .read(cx)
                .ordered_members(Target::Session(node, original.id), cx)),
            [Target::Session(node, original.id)]
        );
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
