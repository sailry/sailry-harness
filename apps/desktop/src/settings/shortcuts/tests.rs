use super::*;
use crate::{preview::Page, settings::Section, shell::Shell};
use core::prelude::v1::test;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    draw(cx);
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn recording_conflicts(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_global(crate::preferences::Preferences::open(path.clone()));
        crate::shell::init(cx);
    });
    let mut state = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Settings, window, cx);
            shell
                .settings
                .update(cx, |settings, cx| settings.select(Section::Shortcuts, cx));
        });
        let panel = shell.read(cx).settings.read(cx).shortcuts.clone();
        state = Some((shell.clone(), panel));
        Root::new(shell, window, cx)
    });
    let (shell, panel) = state.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
    draw(visual);
    let search: &'static str = Box::leak(
        format!("kbd:{}", Keystroke::parse("secondary-k").unwrap().unparse()).into_boxed_str(),
    );
    let key = visual.debug_bounds(search).unwrap();
    let button = visual.debug_bounds("shortcut-app.search").unwrap();
    assert!(key.left() >= button.left() && key.right() <= button.right());
    assert!(key.top() >= button.top() && key.bottom() <= button.bottom());
    for id in [
        "app.quit",
        "app.hide",
        "browser.open",
        "file.save",
        "file.copy",
        "git.commit",
        "terminal.copy",
    ] {
        assert!(
            visual
                .debug_bounds(Box::leak(format!("shortcut-{id}").into_boxed_str()))
                .is_none()
        );
    }
    assert!(visual.debug_bounds("shortcut-reset-app.search").is_none());
    assert!(visual.debug_bounds("shortcut-clear-app.search").is_none());
    click(visual, "shortcut-app.search");
    assert!(visual.debug_bounds(search).is_none());
    assert!(visual.debug_bounds("shortcut-reset-app.search").is_some());
    assert!(visual.debug_bounds("shortcut-clear-app.search").is_some());
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    assert!(visual.debug_bounds("shortcut-save").is_none());
    visual.simulate_modifiers_change(Keystroke::parse("secondary-w").unwrap().modifiers);
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(1));
    visual.run_until_parked();
    visual.simulate_keystrokes("secondary-w");
    assert_eq!(
        panel.read_with(visual, |panel, _| panel.error),
        Some("shortcut_conflict")
    );
    crate::feedback::tests::shown(visual);
    assert_eq!(
        visual.update(crate::feedback::tests::summary),
        tr("shortcut_conflict")
    );
    assert!(panel.read_with(visual, |panel, _| panel.active.is_some()));
    assert_eq!(
        visual
            .update(|_, cx| shortcuts::key("app.search", cx))
            .as_deref(),
        Some("secondary-k")
    );
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.page),
        Page::Settings
    );
    // A corrected combination replaces the conflicting draft without opening an overlay.
    visual.simulate_keystrokes("secondary-j");
    draw(visual);
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("kbd:{}", Keystroke::parse("secondary-j").unwrap().unparse())
                    .into_boxed_str()
            ))
            .is_some()
    );
    assert!(panel.read_with(visual, |panel, _| panel.active.is_none()));
    let saved = visual.update(|_, cx| shortcuts::key("app.search", cx).unwrap());
    assert_eq!(
        Keystroke::parse(&saved).unwrap(),
        Keystroke::parse("secondary-j").unwrap()
    );
    assert_eq!(
        crate::preferences::Preferences::open(path)
            .data
            .shortcuts
            .unwrap()["app.search"],
        saved
    );
    click(visual, "shortcut-app.search");
    visual.simulate_keystrokes("escape");
    draw(visual);
    assert!(panel.read_with(visual, |panel, _| panel.active.is_none()));
    assert_eq!(
        visual.update(|_, cx| shortcuts::key("app.search", cx)),
        Some(saved)
    );
    click(visual, "shortcut-app.search");
    click(visual, "shortcut-clear-app.search");
    assert_eq!(
        visual
            .update(|_, cx| shortcuts::key("app.search", cx))
            .as_deref(),
        Some("")
    );
    assert!(visual.debug_bounds("shortcut-reset-app.search").is_none());
    click(visual, "shortcut-app.search");
    click(visual, "shortcut-reset-app.search");
    assert_eq!(
        visual
            .update(|_, cx| shortcuts::key("app.search", cx))
            .as_deref(),
        Some("secondary-k")
    );
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    visual.update(|_, cx| {
        shortcuts::save("app.search", Some("secondary-j secondary-u"), cx).unwrap();
    });
    panel.update(visual, |_, cx| cx.notify());
    draw(visual);
    let label = visual.debug_bounds("shortcut-label-app.search").unwrap();
    for stroke in ["secondary-j", "secondary-u"] {
        let key = visual
            .debug_bounds(Box::leak(
                format!("kbd:{}", Keystroke::parse(stroke).unwrap().unparse()).into_boxed_str(),
            ))
            .unwrap();
        assert!(key.left() >= label.left() && key.right() <= label.right());
        assert!(key.top() >= label.top() && key.bottom() <= label.bottom());
    }
}
