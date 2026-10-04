use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, Theme};

#[derive(Default)]
struct Presented(std::collections::HashMap<WindowId, Vec<SharedString>>);
impl Global for Presented {}

// Test-only observation of the toast boundary, not a second notification inbox.
pub(super) fn record(window: &Window, summary: SharedString, cx: &mut App) {
    if !cx.has_global::<Presented>() {
        cx.set_global(Presented::default());
    }
    cx.update_global(|presented: &mut Presented, _| {
        presented
            .0
            .entry(window.window_handle().window_id())
            .or_default()
            .push(summary);
    });
}

pub(crate) fn summary(window: &mut Window, cx: &mut App) -> SharedString {
    assert!(!window.notifications(cx).is_empty(), "expected a toast");
    cx.global::<Presented>().0[&window.window_handle().window_id()]
        .last()
        .unwrap()
        .clone()
}

pub(crate) fn count(window: &Window, summary: &str, cx: &App) -> usize {
    if !cx.has_global::<Presented>() {
        return 0;
    }
    cx.global::<Presented>()
        .0
        .get(&window.window_handle().window_id())
        .map_or(0, |messages| {
            messages
                .iter()
                .filter(|message| message.as_ref() == summary)
                .count()
        })
}

struct Form {
    error: Option<&'static str>,
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn mount(cx: &mut TestAppContext) -> (Entity<Form>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut form = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            observe(window, cx, |form: &Form, _| {
                form.error.into_iter().collect()
            });
            Form { error: None }
        });
        form = Some(view.clone());
        Root::new(view, window, cx)
    });
    (form.unwrap(), visual)
}

#[gpui::test]
fn deduplicates_and_rearms(cx: &mut TestAppContext) {
    let (form, visual) = mount(cx);
    let set = |visual: &mut VisualTestContext, error| {
        form.update(visual, |form, cx| {
            form.error = error;
            cx.notify();
        });
        visual.run_until_parked();
    };
    set(visual, Some("files_read_failed"));
    let original = visual.update(|window, cx| window.notifications(cx));
    assert_eq!(original.len(), 1);
    for _ in 0..3 {
        set(visual, Some("files_read_failed"));
    }
    assert_eq!(
        visual.update(|window, cx| window.notifications(cx)),
        original
    );
    set(visual, None);
    set(visual, Some("files_read_failed"));
    let retried = visual.update(|window, cx| window.notifications(cx));
    assert_eq!(retried.len(), 1);
    assert_ne!(retried[0].entity_id(), original[0].entity_id());
}

#[gpui::test]
fn in_app_defaults(cx: &mut TestAppContext) {
    let (_, visual) = mount(cx);
    visual.update(|window, cx| {
        Theme::global_mut(cx).notification.delivery = NotificationDelivery::System;
        toast(
            window,
            tr("files_read_failed"),
            Notification::error(tr("files_read_failed")).system(),
            cx,
        );
        assert_eq!(window.notifications(cx).len(), 1);
    });
    assert!(visual.shown_system_notifications().is_empty());
}

/// Kit's stack animation uses wall time rather than the GPUI test executor clock.
pub(crate) fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    if cx.update(|window, cx| !window.notifications(cx).is_empty()) {
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(500));
        std::thread::sleep(std::time::Duration::from_millis(450));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

pub(crate) fn shown(cx: &mut VisualTestContext) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| !window.notifications(cx).is_empty()) {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "expected a toast");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[gpui::test]
fn card_sizing(cx: &mut TestAppContext) {
    let (_, visual) = mount(cx);
    visual.update(|window, cx| info("", "Saved", window, cx));
    settle(visual);
    let short = visual.debug_bounds("notification-card").unwrap();
    assert!(short.size.width >= px(240.));
    visual.update(|window, cx| {
        window.clear_notifications(cx);
        info(
            "",
            &"A longer notification that should wrap. ".repeat(12),
            window,
            cx,
        );
    });
    settle(visual);
    let long = visual.debug_bounds("notification-card").unwrap();
    assert!(short.size.width < long.size.width);
    assert!(long.size.height > short.size.height);
    assert!(long.size.width <= visual.update(|_, cx| Theme::global(cx).notification.width));
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(260.), px(600.)));
    settle(visual);
    let narrow = visual.debug_bounds("notification-card").unwrap();
    assert!(narrow.left() >= px(0.));
    assert!(narrow.right() <= px(260.));
    assert!(narrow.size.height > long.size.height);
}

#[gpui::test]
fn surface_interactions(cx: &mut TestAppContext) {
    use gpui_kit::component::{
        ActiveTheme as _,
        button::{Button, ButtonVariants as _},
    };
    use std::{cell::Cell, rc::Rc};

    let (_, visual) = mount(cx);
    let rendered = Rc::new(Cell::new(0));
    visual.update({
        let rendered = rendered.clone();
        move |_, cx| {
            gpui_kit::component::surface::set_renderer(
                move |child, corners, cx| {
                    rendered.set(rendered.get() + 1);
                    assert_eq!(
                        corners,
                        Corners::all(cx.theme().surface_radius()).map(|radius| (*radius).into()),
                    );
                    div().child(child).into_any_element()
                },
                cx,
            );
        }
    });
    let actions = Rc::new(Cell::new(0));
    let clicks = Rc::new(Cell::new(0));
    let closes = Rc::new(Cell::new(0));
    visual.update({
        let actions = actions.clone();
        let clicks = clicks.clone();
        let closes = closes.clone();
        move |window, cx| {
            deliver(
                window,
                Notification::success("Project")
                    .title("Completed")
                    .autohide(false)
                    .action(move |_, _, _| {
                        let actions = actions.clone();
                        Button::new("notice-action")
                            .debug_selector(|| "notice-action".into())
                            .ghost()
                            .icon(gpui_kit::component::IconName::ArrowRight)
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                actions.set(actions.get() + 1);
                            })
                    })
                    .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
                    .on_close(move |_, _| closes.set(closes.get() + 1)),
                cx,
            );
        }
    });
    settle(visual);
    assert!(rendered.get() > 0);
    let card = visual.debug_bounds("notification-card").unwrap();
    let icon = visual.debug_bounds("notification-icon").unwrap();
    let title = visual.debug_bounds("notification-title").unwrap();
    let message = visual.debug_bounds("notification-message").unwrap();
    let close = visual.debug_bounds("notification-close").unwrap();
    let action = visual.debug_bounds("notice-action").unwrap();
    assert_eq!(icon.left() - card.left(), px(12.));
    assert_eq!(card.right() - close.right(), px(12.));
    assert_eq!(title.top() - card.top(), px(12.));
    assert_eq!(title.left() - icon.right(), px(8.));
    assert_eq!(message.top() - title.bottom(), px(2.));
    assert!(action.left() >= title.right() + px(8.));
    assert!(close.left() >= action.right() + px(4.));

    visual.simulate_click(action.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(actions.get(), 1);
    assert_eq!(clicks.get(), 0);
    assert_eq!(closes.get(), 0);
    visual.simulate_click(close.center(), Default::default());
    settle(visual);
    assert_eq!(clicks.get(), 0);
    assert_eq!(closes.get(), 1);
    assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
}

#[gpui::test]
fn empty_titles(cx: &mut TestAppContext) {
    let (_, visual) = mount(cx);
    visual.update(|window, cx| info("", "No sound detected", window, cx));
    settle(visual);
    let untitled = visual.debug_bounds("notification-card").unwrap();
    visual.update(|window, cx| {
        window.clear_notifications(cx);
        deliver(window, Notification::info("No sound detected"), cx);
    });
    settle(visual);
    let message_only = visual.debug_bounds("notification-card").unwrap();
    assert_eq!(untitled.size, message_only.size);
    visual.update(|window, cx| {
        window.clear_notifications(cx);
        info("Microphone", "No sound detected", window, cx);
    });
    settle(visual);
    let titled = visual.debug_bounds("notification-card").unwrap();
    assert!(titled.size.height > untitled.size.height);
}

#[gpui::test]
fn replaces_repeated_information(cx: &mut TestAppContext) {
    let (_, visual) = mount(cx);
    visual.update(|window, cx| {
        for _ in 0..6 {
            info("", "Allow microphone access", window, cx);
        }
        assert_eq!(window.notifications(cx).len(), 1);
        info("", "Allow microphone access", window, cx);
        assert_eq!(window.notifications(cx).len(), 1);
        assert_eq!(summary(window, cx), "Allow microphone access");
        info("", "No sound detected", window, cx);
        assert_eq!(window.notifications(cx).len(), 2);
    });
}
