use super::*;
use gpui_kit::component::ThemeMode;

#[gpui::test]
fn preserves_drafts_and_sends(cx: &mut TestAppContext) {
    init(cx);
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::styled(directory.path(), "composer");
    let loaded = crate::theme::package::Loaded::read(&source, true).unwrap();
    cx.update(|cx| {
        cx.global_mut::<crate::theme::Catalog>()
            .packages
            .push(loaded.into())
    });
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep this themed draft");
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            visual.update(|window, cx| {
                crate::theme::apply(1, None, cx);
                crate::theme::select(Some(mode), window, cx);
                view.update(cx, |_, cx| cx.notify());
            });
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds("theme-icon.send").is_some());
            let surface = visual.debug_bounds("composer-surface").unwrap();
            for slot in ["theme-composer.top_left", "theme-composer.bottom_right"] {
                let corner = visual.debug_bounds(slot).unwrap();
                assert!(surface.contains(&corner.origin));
                assert!(corner.right() <= surface.right());
                assert!(corner.bottom() <= surface.bottom());
            }
            assert_eq!(
                view.read_with(visual, |view, cx| view.input.read(cx).value()),
                "Keep this themed draft"
            );
        }
        visual.update(|window, cx| crate::theme::apply(0, Some(window), cx));
        view.update(visual, |_, cx| cx.notify());
        visual.run_until_parked();
        assert!(visual.debug_bounds("theme-icon.send").is_none());
        visual.update(|window, cx| crate::theme::apply(1, Some(window), cx));
        view.update(visual, |_, cx| cx.notify());
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            let view = view.read(cx);
            view.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .page
                    .runs
                    .last()
                    .is_some_and(|run| run.status == Status::Completed)
            }) && !view.busy()
        });
        assert!(view.read_with(visual, |view, cx| view.input.read(cx).value().is_empty()));
        assert!(
            fixture
                .server
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request["messages"]
                    .to_string()
                    .contains("Keep this themed draft"))
        );
        leave(visual);
    }
}
