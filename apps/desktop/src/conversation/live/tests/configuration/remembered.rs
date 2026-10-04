use super::*;

#[gpui::test]
fn restores_defaults(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, Vec::new());
        let path = fixture.directory.path().join("preferences.json");
        cx.update(|cx| cx.set_global(crate::preferences::Preferences::open(path.clone())));
        let selected = provider(&fixture.server.endpoint, "remembered-model");
        fixture.execute(Command::SaveProvider {
            provider: selected.clone(),
            expected_revision: 0,
            secret: None,
        });
        let (view, visual) = fixture::open_session(cx, fixture.binding.clone(), None);
        wait(visual, |cx| {
            view.read(cx).configured()
                && view
                    .read(cx)
                    .node
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.providers.len() == 2)
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                let choice = crate::conversation::models::Selection {
                    channel: view.provider_ids[&(fixture.node.id(), selected.id)],
                    model: selected.default_model.clone(),
                };
                view.select_model(&choice, Effort::High, window, cx);
                view.select_effort(Effort::Low, window, cx);
            })
        });
        click(visual, "live-chat-mode");
        click(visual, "composer_mode_plan-option");
        click(visual, "live-chat-permission");
        click(visual, "composer_permission_project-option");
        let expected = view.read_with(visual, |view, _| {
            assert_eq!(view.error, None);
            view.config.clone().unwrap()
        });
        assert_eq!(expected.model, "remembered-model");
        assert_eq!(expected.effort, Effort::Low);
        assert_eq!(expected.mode, sailry_protocol::WorkMode::Plan);
        assert_eq!(expected.permission, sailry_protocol::Permission::Project);
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(saved["composer"]["model"].get("credential").is_none());
        visual.update(|window, _| window.remove_window());
        cx.update(|cx| cx.set_global(crate::preferences::Preferences::open(path.clone())));
        let (fresh, visual) = fixture::open_session(cx, fixture.binding.clone(), None);
        wait(visual, |cx| fresh.read(cx).configured());
        assert_eq!(
            fresh.read_with(visual, |view, _| view.config.clone().unwrap()),
            expected
        );
        visual.update(|window, _| window.remove_window());
        let (existing, visual) =
            fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            existing.read(cx).configured() && existing.read(cx).connected()
        });
        assert_eq!(
            existing.read_with(visual, |view, _| view.config.clone().unwrap()),
            fixture.session.config
        );
        visual.update(|window, cx| {
            existing.update(cx, |view, cx| {
                view.select_effort(Effort::Low, window, cx);
            })
        });
        wait(visual, |cx| {
            !existing.read(cx).pending
                && existing.read(cx).config.as_ref().unwrap().effort == Effort::Low
        });
        let saved = crate::preferences::Preferences::open(path.clone())
            .data
            .composer
            .unwrap();
        assert_eq!(
            saved.model.unwrap().provider,
            fixture.session.config.provider
        );
        assert_eq!(saved.mode, fixture.session.config.mode);
        assert_eq!(saved.permission, fixture.session.config.permission);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
