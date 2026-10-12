use super::*;
use core::prelude::v1::test;

#[test]
fn rejects_sequences_and_unmodified_keys() {
    assert!(parse("").unwrap().is_none());
    assert!(parse("secondary-alt-space").unwrap().is_some());
    assert!(parse("secondary-alt-s").unwrap().is_some());
    assert!(parse("s").is_err());
    assert!(parse("ctrl-k ctrl-s").is_err());
}

#[gpui::test]
fn unavailable_native_registration_preserves_preferences(cx: &mut TestAppContext) {
    use crate::conversation::live::tests::fixture;
    fixture::init(cx);
    let fixture = fixture::Fixture::with_tools(false, vec![]);
    cx.update(|cx| {
        cx.set_global(crate::backend::Services {
            profile: fixture.node.profile().to_path_buf(),
            runtime: fixture.runtime.clone(),
            link: fixture.node.link().clone(),
            local: fixture.node.local(),
            relay_enabled: false,
        });
        crate::shell::shortcuts::init(cx);
        crate::shortcuts::init(cx);
        let previous = crate::shortcuts::key("system.activate", cx);
        assert_eq!(
            crate::shortcuts::save("system.activate", Some("secondary-alt-z"), cx),
            Err("shortcut_unavailable")
        );
        assert_eq!(crate::shortcuts::key("system.activate", cx), previous);
        assert!(
            !crate::preferences::data(cx)
                .shortcuts
                .unwrap_or_default()
                .contains_key("system.activate")
        );
    });
    fixture.close();
}

#[gpui::test]
fn selection_without_a_composer_stays_visible(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        gpui_kit::component::Root::new(cx.new(|cx| Shell::new(window, cx)), window, cx)
    });
    visual.update(|window, cx| show("Captured selection".into(), None, window, cx));
    assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn quotes_into_the_captured_composer(cx: &mut TestAppContext) {
    use crate::conversation::live::tests::{fixture, wait};
    use gpui_kit::test::TestWindowExt;
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                assert!(view.insert_quote("Keep draft", window, cx))
            });
            show(
                "Selected text\n第二行".into(),
                Some(Composer::capture(view.clone(), cx)),
                window,
                cx,
            );
            window.render_frame(cx);
            let target = window.find("selection-insert");
            assert!(target.visible() && target.disabled() != Some(true));
            window.click("selection-insert", cx);
        });
        visual.run_until_parked();
        visual.update(|window, cx| {
            assert!(!window.has_active_dialog(cx));
            assert_eq!(
                view.read(cx).draft(cx).as_ref(),
                "> Keep draft\n\n\n\n> Selected text\n> 第二行\n\n"
            );
        });
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn stale_targets_preserve_the_selection_and_draft(cx: &mut TestAppContext) {
    use crate::conversation::live::tests::{fixture, wait};
    use gpui_kit::test::TestWindowExt;
    fixture::init(cx);
    let fixture = fixture::Fixture::with_tools(false, vec![]);
    let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
    wait(visual, |cx| view.read(cx).connected());
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.insert_quote("Keep draft", window, cx))
        });
        let mut captured = Composer::capture(view.clone(), cx);
        captured.worktree = Some(sailry_protocol::WorktreeId::new());
        show("Selected text".into(), Some(captured), window, cx);
        window.render_frame(cx);
        window.click("selection-insert", cx);
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        assert_eq!(view.read(cx).draft(cx).as_ref(), "> Keep draft\n\n");
        window.close_dialog(cx);
        window.remove_window();
    });
    assert_eq!(fixture.task_requests(), 0);
    drop(view);
    fixture.close();
}
