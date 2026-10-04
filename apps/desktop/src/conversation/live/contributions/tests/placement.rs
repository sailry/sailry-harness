use super::*;

#[gpui::test]
fn reaches_context_and_embedded_menus(cx: &mut TestAppContext) {
    init(cx);
    cx.update(crate::plugins::init);
    for remote in [false, true] {
        let mut fixture = Fixture::with_tools(remote, vec![]);
        install_with(&mut fixture, |manifest| {
            for entry in manifest["extensions"]["dev.sailry.platform"]["ui"]
                .as_array_mut()
                .unwrap()
            {
                match entry["id"].as_str().unwrap() {
                    "focus" => {
                        entry["slot"] = json!("context");
                        entry["align"] = json!("end");
                    }
                    "refresh" => entry["slot"] = json!("context"),
                    _ => {}
                }
            }
        });
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        resize(visual, 1100.);
        wait(visual, |cx| value(&view, "focus", cx) == Some(json!(false)));
        let note = visual
            .debug_bounds("plugin-control-task-notes-note")
            .unwrap();
        let focus = visual
            .debug_bounds("plugin-control-task-notes-focus")
            .unwrap();
        assert!(focus.left() >= note.right());
        assert_eq!(focus.center().y, note.center().y);
        tap(visual, "plugin-control-task-notes-focus");
        wait(visual, |cx| value(&view, "focus", cx) == Some(json!(true)));
        assert!(
            visual
                .debug_bounds("plugin-control-task-notes-refresh")
                .is_some()
        );
        assert!(visual.debug_bounds("composer-settings").is_none());
        tap(visual, "plugin-control-task-notes-refresh");
        wait(visual, |cx| value(&view, "refresh", cx).is_some());

        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.sidebar = true;
                cx.notify();
            });
        });
        resize(visual, 900.);
        wait(visual, |cx| !view.read(cx).compact_composer);
        for selector in [
            "plugin-control-task-notes-note",
            "plugin-control-task-notes-focus",
            "plugin-control-task-notes-refresh",
            "plugin-control-task-notes-actions",
        ] {
            assert!(
                visual.debug_bounds(selector).is_some(),
                "wide resource action missing: {selector}"
            );
        }
        assert!(visual.debug_bounds("composer-settings").is_none());
        resize(visual, 520.);
        tap(visual, "composer-settings");
        assert!(
            visual
                .debug_bounds("plugin-control-task-notes-note")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("plugin-control-task-notes-actions")
                .is_some()
        );
        tap(visual, "plugin-control-task-notes-focus");
        wait(visual, |cx| value(&view, "focus", cx) == Some(json!(false)));
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
