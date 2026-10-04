use super::workspace::click;
use super::*;
use crate::{
    conversation::{subagent::Key, transcript::Location, turn::Status},
    resources::SideResource,
};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
}

fn show(shell: &Entity<Shell>, cx: &mut VisualTestContext) -> Location {
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_session((0, 2), window, cx);
            shell.show_subagent_preview((0, 2), cx);
        })
    });
    draw(cx);
    Location {
        child: Some(Key {
            id: 0,
            generation: 0,
        }),
        ..Location::main((0, 2), 0)
    }
}

#[gpui::test]
fn selection_and_detail(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let location = show(&shell, &mut cx);
    click(&mut cx, "composer-subagents");
    let popup = cx.debug_bounds("subagent-picker").unwrap();
    let trigger = cx.debug_bounds("composer-subagents").unwrap();
    assert!(popup.bottom() < trigger.top());
    for selector in [
        "subagent-row-0",
        "subagent-row-1",
        "subagent-row-2",
        "subagent-row-3",
    ] {
        assert!(cx.debug_bounds(selector).is_some());
    }
    click(&mut cx, "subagent-row-1");
    assert!(cx.debug_bounds("subagent-row-2").is_none());
    assert!(cx.debug_bounds("subagent-picker").unwrap().size.height < popup.size.height);
    click(&mut cx, "subagent-row-1");
    assert!(cx.debug_bounds("subagent-row-2").is_some());
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    assert!(cx.debug_bounds("subagent-picker").is_none());
    assert!(cx.debug_bounds("subagent-panel").is_some());
    assert!(cx.debug_bounds("child-1-0-phase").is_some());
    click(&mut cx, "subagent-switch");
    let popup = cx.debug_bounds("subagent-picker").unwrap();
    let header = cx.debug_bounds("subagent-header").unwrap();
    assert!(popup.top() >= header.bottom());
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    let old = cx.update(|_, cx| {
        let Some(SideResource::Subagent(panel)) = &shell.read(cx).side_resource else {
            panic!("missing panel")
        };
        assert_eq!(panel.location.child.unwrap().id, 1);
        panel.scroller.downgrade()
    });
    click(&mut cx, "child-1-0-group-0");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        let Some(SideResource::Subagent(panel)) = &shell.side_resource else {
            panic!("missing panel")
        };
        assert_eq!(panel.expanded.get(&0), Some(&true));
        assert!(
            shell
                .transcript_turn(panel.location)
                .unwrap()
                .expanded
                .is_empty()
        );
    });
    click(&mut cx, "toggle-details");
    assert!(old.upgrade().is_none());
    click(&mut cx, "composer-subagents");
    click(&mut cx, "subagent-row-0");
    assert!(cx.debug_bounds("child-0-0-footer").is_some());
    assert!(cx.debug_bounds("child-0-0-copy").is_some());
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        let Some(SideResource::Subagent(panel)) = &shell.side_resource else {
            panic!("missing panel")
        };
        assert_eq!(panel.location, location);
        assert!(panel.expanded.is_empty());
        assert_eq!(shell.conversations[&(0, 2)].turns.len(), 1);
        assert!(shell.conversations[&(0, 2)].preview_task.is_none());
    });
}

#[gpui::test]
fn isolates_owner_actions(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let location = show(&shell, &mut cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(!shell.open_subagent(
                Location {
                    child: Some(Key {
                        id: 0,
                        generation: 1
                    }),
                    ..location
                },
                cx
            ));
            shell.conversations[&(0, 2)]
                .input
                .update(cx, |input, cx| input.set_value("Next draft", window, cx));
            shell.files.tabs.open(1);
            shell.git.tabs.open(1);
            assert!(shell.open_subagent(location, cx));
            shell.select_session((0, 0), window, cx);
            assert!(shell.side_resource.is_none());
            assert!(!shell.open_subagent(location, cx));
            shell.select_session((0, 2), window, cx);
            assert!(shell.open_subagent(location, cx));
        })
    });
    draw(&mut cx);
    click(&mut cx, "composer-send");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        let thread = &shell.conversations[&(0, 2)];
        assert_eq!(thread.input.read(cx).value(), "Next draft");
        assert_eq!(thread.turns[0].status, Status::Cancelled);
        assert_eq!(
            thread.turns[0]
                .subagents()
                .map(|agent| agent.turn.status)
                .collect::<Vec<_>>(),
            [
                Status::Completed,
                Status::Cancelled,
                Status::Cancelled,
                Status::Failed
            ]
        );
        assert_eq!(shell.files.tabs.open, [0, 1]);
        assert_eq!(shell.git.tabs.open, [0, 1]);
        assert_eq!(shell.page, Page::Conversation);
    });
    click(&mut cx, "toggle-details");
    click(&mut cx, "composer-subagents");
    click(&mut cx, "subagent-row-2");
    assert!(cx.debug_bounds("child-2-0-footer").is_some());
    assert!(cx.debug_bounds("child-2-0-phase").is_none());
}

#[gpui::test]
fn keyboard_tree_and_child_links(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    show(&shell, &mut cx);
    click(&mut cx, "composer-subagents");
    cx.simulate_keystrokes("down down");
    draw(&mut cx);
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    cx.update(|_, cx| {
        let Some(SideResource::Subagent(panel)) = &shell.read(cx).side_resource else {
            panic!("missing panel")
        };
        assert_eq!(panel.location.child.unwrap().id, 2);
    });
    let link = cx.debug_bounds("child-2-0-tool-link-0").unwrap();
    cx.simulate_click(
        point(link.left() + px(24.), link.center().y),
        Modifiers::default(),
    );
    draw(&mut cx);
    assert!(cx.debug_bounds("subagent-panel").is_none());
    assert!(cx.debug_bounds("file-preview").is_some());
    cx.update(|_, cx| assert_eq!(shell.read(cx).page, Page::Conversation));
}

#[gpui::test]
fn panel_geometry_and_visibility(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let location = show(&shell, &mut cx);
    cx.update(|_, cx| shell.update(cx, |shell, cx| assert!(shell.open_subagent(location, cx))));
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [1280., 1600., 760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            draw(&mut cx);
            cx.update(|_, cx| assert!(shell.read(cx).layout.panel_open[0]));
            if width == 760. {
                assert!(cx.debug_bounds("subagent-panel").is_none());
                continue;
            }
            let panel = cx.debug_bounds("subagent-panel").unwrap();
            let header = cx.debug_bounds("subagent-header").unwrap();
            assert!(cx.debug_bounds("close-details").is_none());
            let toggle = cx.debug_bounds("toggle-details").unwrap();
            let conversation = cx.debug_bounds("conversation-page").unwrap();
            assert_eq!(header.top(), panel.top());
            assert_eq!(header.size.height, px(HEADER_HEIGHT));
            assert!(toggle.right() <= conversation.right());
            assert!(conversation.right() <= panel.left());
            assert!(cx.debug_bounds("composer-subagents").unwrap().right() <= conversation.right());
            assert!(cx.debug_bounds("child-0-0-header").unwrap().top() >= header.bottom());
        }
    }
}

#[gpui::test]
fn revokes_open_detail(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let location = show(&shell, &mut cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_host(1, window, cx);
            shell.select_session((1, 0), window, cx);
            shell.show_subagent_preview((1, 0), cx);
            assert!(!shell.open_subagent(location, cx));
            shell.select_host(0, window, cx);
            shell.select_session((0, 2), window, cx);
            assert!(shell.open_subagent(
                Location {
                    child: Some(Key {
                        id: 1,
                        generation: 0
                    }),
                    ..location
                },
                cx
            ));
            shell.project_trust(0, None, window, cx);
        })
    });
    crate::prompts::tests::answer(&mut cx, "project_revoke");
    assert!(cx.debug_bounds("child-1-0-footer").is_some());
    assert!(cx.debug_bounds("child-1-0-phase").is_none());
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert!(
            shell.conversations[&(1, 0)]
                .turns
                .last()
                .unwrap()
                .status
                .active()
        );
        assert!(!shell.conversations[&(0, 2)].turns[0].status.active());
    });
}

#[gpui::test]
fn preserves_selection(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    show(&shell, &mut cx);
    click(&mut cx, "composer-subagents");
    click(&mut cx, "subagent-row-1");
    assert!(cx.debug_bounds("subagent-row-2").is_none());
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.stop_preview((0, 2), cx)));
    draw(&mut cx);
    assert!(cx.debug_bounds("subagent-row-2").is_none());
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        let Some(SideResource::Subagent(panel)) = &shell.side_resource else {
            panic!("missing panel")
        };
        assert_eq!(panel.location.child.unwrap().id, 1);
        assert_eq!(
            shell.transcript_turn(panel.location).unwrap().status,
            Status::Cancelled
        );
    });
}
