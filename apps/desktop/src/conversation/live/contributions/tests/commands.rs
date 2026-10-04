use super::*;

#[gpui::test]
fn command_aliases_reuse_toggle_and_picker_state(cx: &mut TestAppContext) {
    init(cx);
    cx.update(crate::plugins::init);
    for remote in [false, true] {
        let mut fixture = Fixture::with_tools(remote, vec![]);
        let info = install_with(&mut fixture, |manifest| {
            for entry in manifest["extensions"]["dev.sailry.platform"]["ui"]
                .as_array_mut()
                .unwrap()
            {
                if matches!(entry["id"].as_str(), Some("focus" | "note")) {
                    entry["command"] = json!({"name":entry["id"],"kind":"invoke"});
                }
            }
        });
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| value(&view, "focus", cx) == Some(json!(false)));
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep /focus");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| value(&view, "focus", cx) == Some(json!(true)));
        assert_eq!(view.read_with(visual, |view, cx| view.draft(cx)), "Keep ");
        tap(visual, "plugin-control-task-notes-focus");
        wait(visual, |cx| value(&view, "focus", cx) == Some(json!(false)));
        tap(visual, "live-chat-input");
        visual.simulate_input("/note");
        visual.simulate_keystrokes("enter");
        assert!(visual.debug_bounds("plugin-contribution-picker").is_some());
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            value(&view, "note", cx) == Some(json!("build"))
        });
        assert_eq!(view.read_with(visual, |view, cx| view.draft(cx)), "Keep ");
        fixture.execute(Command::SetPluginEnabled {
            name: info.summary.name,
            expected_revision: info.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| {
            view.read(cx)
                .contributions
                .read(cx)
                .commands(cx)
                .iter()
                .all(|entry| entry.key.package.name != "task-notes")
        });
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
