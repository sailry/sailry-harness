use super::*;
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt as _;

struct Surface;

impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn press(visual: &mut VisualTestContext, key: &str) {
    let keystroke = Keystroke::parse(key).unwrap();
    visual.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    visual.simulate_event(KeyUpEvent { keystroke });
}

#[gpui::test]
fn preserves_keyboard_and_vendor_defaults(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|window, cx| {
        let surface = cx.new(|_| Surface);
        Root::new(surface, window, cx)
    });
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(1000.)));
    let editor = visual.update(|window, cx| {
        let owner = cx.new(|cx| Workspace::new(window, cx));
        let editor = super::super::open(owner, None, window, cx);
        editor.update(cx, |editor, cx| {
            editor.select_preset(Preset::ChatGpt, window, cx)
        });
        editor
    });
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(visual);
    assert!(visual.debug_bounds("provider-oauth-advanced").is_some());
    let body = visual.debug_bounds("provider-editor").unwrap();
    let trigger = visual.debug_bounds("provider-oauth-advanced").unwrap();
    let label = visual
        .debug_bounds("provider-oauth-advanced-label")
        .unwrap();
    assert_eq!(trigger.left(), body.left());
    assert!(label.left() - trigger.left() < trigger.size.width / 4.);
    visual.update(|window, _| {
        assert_eq!(
            window.find("provider-oauth-advanced").expanded(),
            Some(false)
        );
    });
    assert!(visual.debug_bounds("provider-oauth-user-agent").is_none());
    editor.read_with(visual, |editor, cx| {
        assert_eq!(
            editor.oauth.options(editor.preset, cx).unwrap(),
            Options::defaults(editor.preset.authentication())
        );
    });
    visual.update(|window, cx| {
        editor
            .read(cx)
            .name
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx)
    });
    draw(visual);
    for control in ["provider-type", "provider-oauth-advanced"] {
        visual.update(|window, cx| window.press("tab", cx));
        draw(visual);
        visual.update(|window, _| {
            assert_eq!(window.find(control).focused(), Some(true), "{control}");
        });
    }
    visual.update(|window, cx| window.press("enter", cx));
    draw(visual);
    assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
    assert!(!editor.read_with(visual, |editor, _| editor.closed));
    assert!(
        visual
            .debug_bounds("provider-oauth-catalog-version")
            .is_some()
    );
    assert!(visual.debug_bounds("provider-oauth-user-agent").is_some());
    visual.update(|window, cx| {
        assert_eq!(
            window.find("provider-oauth-advanced").expanded(),
            Some(true)
        );
        assert_eq!(window.find("provider-oauth-advanced").focused(), Some(true));
        window.press("enter", cx);
    });
    draw(visual);
    assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
    assert!(!editor.read_with(visual, |editor, _| editor.closed));
    assert!(visual.debug_bounds("provider-oauth-user-agent").is_none());

    for expanded in [true, false] {
        visual.update(|window, cx| {
            assert_eq!(window.find("provider-oauth-advanced").focused(), Some(true));
            window.press("space", cx);
        });
        draw(visual);
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        editor.read_with(visual, |editor, _| {
            assert!(!editor.closed);
            assert_eq!(editor.oauth.expanded, expanded);
        });
        assert_eq!(
            visual.debug_bounds("provider-oauth-user-agent").is_some(),
            expanded
        );
    }
    for expanded in [true, false] {
        visual.update(|window, cx| window.click("provider-oauth-advanced", cx));
        draw(visual);
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        editor.read_with(visual, |editor, _| {
            assert!(!editor.closed);
            assert_eq!(editor.oauth.expanded, expanded);
        });
        assert_eq!(
            visual.debug_bounds("provider-oauth-user-agent").is_some(),
            expanded
        );
    }

    // Admission can become pending before the next paint disables the focused control.
    editor.update(visual, |editor, _| editor.pending = true);
    press(visual, "enter");
    draw(visual);
    assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
    editor.read_with(visual, |editor, _| {
        assert!(!editor.closed && !editor.oauth.expanded);
    });
    press(visual, "space");
    draw(visual);
    assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
    editor.read_with(visual, |editor, _| {
        assert!(!editor.closed && !editor.oauth.expanded);
    });
    visual.update(|window, cx| window.click("provider-oauth-advanced", cx));
    draw(visual);
    assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
    editor.read_with(visual, |editor, _| {
        assert!(!editor.closed && !editor.oauth.expanded);
    });
    editor.update(visual, |editor, cx| {
        editor.pending = false;
        cx.notify();
    });
    draw(visual);
    for expanded in [true, false] {
        visual.update(|window, cx| window.press("enter", cx));
        draw(visual);
        editor.read_with(visual, |editor, _| {
            assert!(!editor.closed);
            assert_eq!(editor.oauth.expanded, expanded);
        });
    }

    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.select_preset(Preset::Copilot, window, cx);
            assert_eq!(
                editor.oauth.options(editor.preset, cx).unwrap(),
                Options::defaults(editor.preset.authentication())
            );
            editor
                .oauth
                .user_agent
                .update(cx, |input, cx| input.set_value("Fixture/9.3", window, cx));
            let draft = editor.oauth.options(editor.preset, cx).unwrap();
            editor.select_preset(Preset::CopilotResponses, window, cx);
            assert_eq!(editor.oauth.options(editor.preset, cx).unwrap(), draft);
            editor.select_preset(Preset::ChatGpt, window, cx);
            assert_eq!(
                editor.oauth.options(editor.preset, cx).unwrap(),
                Options::defaults(editor.preset.authentication())
            );
            editor.select_preset(Preset::OpenAi, window, cx);
            assert_eq!(editor.oauth.options(editor.preset, cx).unwrap(), None);
        });
    });
    draw(visual);
    assert!(visual.debug_bounds("provider-oauth-advanced").is_none());
    visual.update(|window, _| window.remove_window());
}
