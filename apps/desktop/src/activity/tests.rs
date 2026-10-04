use super::*;
use crate::preview::Page;
use core::prelude::v1::test;
use sailry_client::activity::Kind;
use std::time::{Duration, Instant};

mod center;
mod delivery;
mod navigation;
mod preferences;

#[track_caller]
fn wait(cx: &mut VisualTestContext, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            ready(cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "activity UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

#[test]
fn status_icons_use_embedded_catalog() {
    use gpui_kit::AssetSource;
    use sailry_client::activity::Lane;
    for lane in [
        Lane::Waiting,
        Lane::Running,
        Lane::Completed,
        Lane::Failed,
        Lane::Idle,
    ] {
        assert!(
            crate::assets::Assets
                .load(super::icons::path(lane))
                .unwrap()
                .is_some()
        );
    }
}

#[gpui::test]
fn background_host_navigation(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("zh-CN");
    let fixture = fixture::Fixture::new();
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
    // TestPlatform does not activate newly opened windows; this case exercises
    // acknowledgement while the conversation is actually visible.
    visual.update(|window, _| window.activate_window());
    wait(visual, |cx| {
        shell.read(cx).activity.observers.len() == 2
            && shell
                .read(cx)
                .activity
                .observers
                .values()
                .all(|observer| observer.view.connected)
    });
    for index in 0..2 {
        fixture.submit(index);
    }
    wait(visual, |cx| {
        shell
            .read(cx)
            .activity
            .inbox
            .notices()
            .iter()
            .filter(|notice| notice.kind == Kind::Input)
            .count()
            == 2
    });
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.nodes[1 - index].id(), cx);
                shell.navigate(Page::Settings, window, cx);
            });
            window.clear_notifications(cx);
        });
        let row = shell.read_with(visual, |shell, _| {
            shell
                .activity
                .inbox
                .notices()
                .iter()
                .position(|notice| {
                    notice.id.node == node
                        && notice.id.target == Target::Session(fixture.sessions[index].id)
                })
                .unwrap()
        });
        click(visual, "notifications-open");
        let selector: &'static str = Box::leak(format!("notification-{row}").into_boxed_str());
        let label = visual
            .debug_bounds(Box::leak(format!("{selector}-label").into_boxed_str()))
            .unwrap();
        let icon = visual
            .debug_bounds(Box::leak(format!("{selector}-icon").into_boxed_str()))
            .unwrap();
        let bounds = visual.debug_bounds(selector).unwrap();
        assert_eq!(bounds.size.height, px(36.));
        assert!(label.size.height <= px(24.));
        assert!(icon.right() < label.left());
        assert_eq!(label.right(), bounds.right() - px(8.));
        assert!((label.center().y - icon.center().y).abs() < px(1.));
        click(visual, selector);
        wait(visual, |cx| {
            shell.read(cx).page == Page::Conversation
                && shell
                    .read(cx)
                    .current_chat()
                    .is_some_and(|chat| chat.read(cx).session() == Some(fixture.sessions[index].id))
        });
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().selected),
            node
        );
        assert!(!shell.read_with(visual, |shell, _| shell.activity.open));
    }
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.activity.inbox.unread()),
        0
    );
    fixture.answer(1);
    wait(visual, |cx| {
        shell
            .read(cx)
            .activity
            .inbox
            .notices()
            .iter()
            .any(|notice| notice.id.node == fixture.nodes[1].id() && notice.kind == Kind::Completed)
    });
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.activity.inbox.unread()),
        0
    );
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}
