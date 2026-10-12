use super::*;

#[test]
fn distinguishes_confirmed_and_unknown_failures() {
    let error = Fault::new(ErrorCode::Unavailable, "plugin service is unavailable");
    assert_eq!(completion(&Err(error.clone()), false), Err(error.clone()));
    assert_eq!(error_key(&error, false), "plugins_failed");
    assert_eq!(
        completion(&Err(error.clone()), true).unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(error_key(&error, true), "plugins_unknown");
}

#[gpui::test]
fn preserves_an_overlay_above_the_prompt(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        install(&fixture, "package", "example");
        fixture.package("2.0.0");
        fixture.transport.mode.store(2, Ordering::SeqCst);
        let (owner, visual) = fixture.mount(cx);
        start(&fixture, &owner, visual);
        let state = owner.read_with(visual, |owner, _| {
            owner.plugin_catalog.updates.clone().unwrap()
        });
        visual.update(|window, cx| {
            window.open_dialog(cx, |dialog, _, _| {
                dialog.child(
                    div()
                        .debug_selector(|| "update-confirmation".into())
                        .child("Confirmation"),
                )
            });
            state.update(cx, |state, cx| state.apply(None, window, cx));
        });
        shown(visual, "update-confirmation", true);
        assert!(visual.debug_bounds("installation-list").is_none());
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        assert!(state.read_with(visual, |state, _| {
            state.batch.is_none()
                && state.dialog.as_ref().unwrap().rows["example"].selected
                && state.entries.contains_key("example")
                && state.binding.client.target() == fixture.node.id()
        }));
        visual.update(|window, cx| window.close_dialog(cx));
        tap(visual, "plugins-updates-apply");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        shown(visual, "plugins-update-dialog", false);
        shown(visual, "installation-list", true);
        assert!(visual.debug_bounds("dialog-1").is_none());
        fixture.transport.release.cancel();
        wait(visual, |cx| !state.read(cx).busy());
        fixture.close(visual);
    }
}

#[gpui::test]
fn restores_a_hidden_queue(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        install(&fixture, "package", "example");
        fixture.package("2.0.0");
        fixture.transport.mode.store(2, Ordering::SeqCst);
        let (owner, visual) = fixture.mount(cx);
        start(&fixture, &owner, visual);
        let state = owner.read_with(visual, |owner, _| {
            owner.plugin_catalog.updates.clone().unwrap()
        });
        let retained = fixture.client.prepare(Command::InstallBundledPlugin {
            name: "retained".into(),
            expected_revision: 0,
        });
        visual.update(|window, cx| {
            crate::installations::track(
                state.read(cx).binding.client.clone(),
                fixture.runtime.clone(),
                &retained,
                true,
                |_, _, _| {},
                window,
                cx,
            );
            crate::installations::open(window, cx);
        });
        shown(visual, "installation-list", true);
        tap(visual, "installation-hide");
        tap(visual, "plugins-updates-apply");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        shown(visual, "plugins-update-dialog", false);
        shown(visual, "installation-list", true);
        assert!(visual.debug_bounds("dialog-1").is_none());
        let captured = state.read_with(visual, |state, _| {
            state.batch.as_ref().unwrap().request.clone().unwrap()
        });
        for id in [retained.id, captured.id] {
            assert!(
                visual
                    .debug_bounds(Box::leak(format!("installation-{id}").into_boxed_str()))
                    .is_some()
            );
        }
        tap(visual, "installation-hide");
        visual.update(|window, cx| crate::installations::open(window, cx));
        shown(visual, "installation-list", true);
        assert!(visual.debug_bounds("plugins-update-dialog").is_none());
        assert!(visual.debug_bounds("dialog-1").is_none());
        fixture.transport.release.cancel();
        wait(visual, |cx| !state.read(cx).busy());
        fixture.close(visual);
    }
}
