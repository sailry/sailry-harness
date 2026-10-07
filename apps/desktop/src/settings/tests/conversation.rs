use super::*;
use crate::preferences::{MessageDisplay, Preferences};

#[gpui::test]
fn drags_and_restores_text_size(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_global(Preferences::open(path.clone()));
    });
    let mut settings = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        view.update(cx, |view, cx| view.select(Section::Conversation, cx));
        settings = Some(view.clone());
        Root::new(view, window, cx)
    });
    let settings = settings.unwrap();
    assert_eq!(
        settings.read_with(visual, |view, cx| {
            let slider = view.conversation.text_size.read(cx);
            assert_eq!(
                (slider.min_value(), slider.max_value(), slider.step_value()),
                (12., 18., 2.)
            );
            slider.value().start()
        }),
        14.
    );
    for width in [420., 960.] {
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(width), px(820.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let slider = visual.debug_bounds("message-text-size-slider").unwrap();
        assert!(slider.left() >= px(0.) && slider.right() <= px(width));
        for size in crate::preferences::Data::MESSAGE_TEXT_SIZES {
            let label = Box::leak(format!("message-text-size-label-{size}").into_boxed_str());
            assert!(visual.debug_bounds(label).is_none());
            let selector = Box::leak(format!("message-text-size-tick-{size}").into_boxed_str());
            let tick = visual.debug_bounds(selector).unwrap();
            assert!(tick.left() >= px(0.) && tick.right() <= px(width));
            let inset = px(8.);
            let fraction = f32::from(size - 12) / 6.;
            let position = point(
                slider.left() + inset + (slider.size.width - inset * 2.) * fraction,
                slider.center().y,
            );
            assert!((tick.center().x - position.x).abs() <= px(1.));
            let current = visual.update(|_, cx| crate::preferences::data(cx).message_text_size());
            let thumb = point(
                slider.left()
                    + inset
                    + (slider.size.width - inset * 2.) * f32::from(current - 12) / 6.,
                slider.center().y,
            );
            visual.simulate_mouse_down(thumb, MouseButton::Left, Modifiers::none());
            visual.simulate_mouse_move(position, MouseButton::Left, Modifiers::none());
            visual.simulate_mouse_move(position, MouseButton::Left, Modifiers::none());
            visual.simulate_mouse_up(position, MouseButton::Left, Modifiers::none());
            visual.run_until_parked();
            assert_eq!(
                visual.update(|_, cx| crate::preferences::data(cx).message_text_size()),
                size,
            );
            assert_eq!(
                Preferences::open(path.clone()).data.message_text_size(),
                size
            );
            visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::none());
            visual.simulate_mouse_move(position, None, Modifiers::none());
            visual
                .executor()
                .advance_clock(std::time::Duration::from_secs(1));
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds("message-text-size-tooltip").is_some());
        }
    }
    // Kit accessibility increment/decrement uses this state path without a
    // SliderEvent, so persistence must not depend on pointer release alone.
    visual.update(|window, cx| {
        settings.update(cx, |view, cx| {
            view.conversation.text_size.update(cx, |slider, cx| {
                slider.set_value(16., window, cx);
            });
        });
    });
    visual.run_until_parked();
    assert_eq!(Preferences::open(path.clone()).data.message_text_size(), 16);
    visual.update(|window, _| window.remove_window());
    cx.update(|cx| cx.set_global(Preferences::open(path.clone())));
    let mut restored = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        restored = Some(view.clone());
        Root::new(view, window, cx)
    });
    assert_eq!(
        restored.unwrap().read_with(visual, |view, cx| {
            view.conversation.text_size.read(cx).value().start()
        }),
        16.
    );
    assert_eq!(Preferences::open(path).data.terminal.font_size, 14);
}

#[gpui::test]
fn persists_message_display(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_global(Preferences::open(path.clone()));
    });
    let mut settings = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        view.update(cx, |view, cx| view.select(Section::Conversation, cx));
        settings = Some(view.clone());
        Root::new(view, window, cx)
    });
    let settings = settings.unwrap();
    assert_eq!(
        visual.update(|_, cx| crate::preferences::data(cx)
            .message_display
            .unwrap_or_default()),
        MessageDisplay::Detailed,
    );
    for (width, key, mode) in [
        (420., "message_display_compact", MessageDisplay::Compact),
        (960., "message_display_detailed", MessageDisplay::Detailed),
    ] {
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(width), px(820.)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let tab = visual.debug_bounds(key).unwrap();
        assert!(tab.left() >= px(0.) && tab.right() <= px(width));
        visual.simulate_click(tab.center(), Modifiers::default());
        visual.run_until_parked();
        assert_eq!(
            Preferences::open(path.clone()).data.message_display,
            Some(mode)
        );
        visual.update(|_, cx| {
            cx.set_global(Preferences::open(path.clone()));
            settings.update(cx, |settings, cx| {
                settings.select(Section::General, cx);
                settings.select(Section::Conversation, cx);
            });
        });
        visual.run_until_parked();
        assert_eq!(
            visual.update(|_, cx| crate::preferences::data(cx).message_display),
            Some(mode),
        );
    }
}
