use super::*;
use crate::preferences::{MessageDisplay, Preferences};

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
