use super::*;
use crate::{activity::fixture::Fixture, conversation::live::tests::wait};
use core::prelude::v1::test;

fn click(visual: &mut VisualTestContext, selector: &str) {
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn live_selection_preserves_the_captured_session(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fixture = Fixture::new();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    wait(visual, |cx| {
        let live = shell.read(cx).live.as_ref().unwrap();
        fixture
            .nodes
            .iter()
            .all(|node| live.hosts.contains_key(&node.id()))
            && live.view.connected
            && live.info.is_some()
    });
    assert!(visual.debug_bounds("sidebar-host-icon").is_some());
    let button = visual.debug_bounds("sidebar-host").unwrap();
    assert!(button.bottom() <= visual.debug_bounds("sidebar-search").unwrap().top());
    click(visual, &format!("live-session-{}", fixture.sessions[0].id));
    wait(visual, |cx| {
        shell
            .read(cx)
            .current_chat()
            .is_some_and(|chat| chat.read(cx).connected())
    });
    let captured = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
    for index in [1, 0, 1] {
        click(visual, "sidebar-host");
        let selector = format!(
            "sidebar-host-option-{}",
            crate::live::node_key(fixture.nodes[index].id())
        );
        click(visual, &selector);
        wait(visual, |cx| {
            let shell = shell.read(cx);
            let live = shell.live.as_ref().unwrap();
            shell.page == Page::Host
                && live.selected == fixture.nodes[index].id()
                && live.view.connected
                && live.info.is_some()
                && live.view.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.node == fixture.nodes[index].id()
                        && snapshot
                            .sessions
                            .iter()
                            .any(|session| session.id == fixture.sessions[index].id)
                })
        });
        assert!(visual.debug_bounds("sidebar-host-icon").is_some());
        captured.read_with(visual, |view, _| {
            assert_eq!(view.session(), Some(fixture.sessions[0].id));
            assert_eq!(view.binding().client.target(), fixture.nodes[0].id());
        });
    }
    click(visual, "sidebar-host");
    click(
        visual,
        &format!(
            "sidebar-host-option-{}",
            crate::live::node_key(fixture.nodes[0].id())
        ),
    );
    wait(visual, |cx| {
        shell.read(cx).live.as_ref().unwrap().selected == fixture.nodes[0].id()
    });
    // A menu's captured peer cannot select a Node after it leaves the known set.
    click(visual, "sidebar-host");
    visual.update(|_, cx| {
        shell.update(cx, |shell, _| {
            shell
                .live
                .as_mut()
                .unwrap()
                .hosts
                .remove(&fixture.nodes[1].id());
        })
    });
    click(
        visual,
        &format!(
            "sidebar-host-option-{}",
            crate::live::node_key(fixture.nodes[1].id())
        ),
    );
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().selected),
        fixture.nodes[0].id()
    );
    visual.update(|window, _| window.remove_window());
    drop(captured);
    drop(shell);
    fixture.close();
}

#[gpui::test]
fn preview_selection_stays_passive(cx: &mut TestAppContext) {
    let (shell, mut visual) = crate::shell::tests::setup(cx);
    click(&mut visual, "sidebar-host");
    click(&mut visual, "sidebar-host-option-1");
    shell.read_with(&visual, |shell, _| {
        assert!(shell.live.is_none());
        assert_eq!(shell.host, 1);
        assert_eq!(shell.page, Page::Host);
    });
    click(&mut visual, "sidebar-host");
    click(&mut visual, "sidebar-host-option-0");
    assert_eq!(shell.read_with(&visual, |shell, _| shell.host), 0);
}
