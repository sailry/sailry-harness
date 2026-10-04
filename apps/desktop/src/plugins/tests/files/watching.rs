use super::*;

#[gpui::test]
fn refreshes_clean_buffers_and_guards_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let (shell, visual) = mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual);
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        click(visual, "resource-file-notes.txt");
        let id = document(&panel, visual, "notes.txt");
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        let input = controller.read_with(visual, |controller, _| {
            controller.editor("notes.txt").unwrap()
        });
        visual.update(|_, cx| input.update(cx, |input, cx| input.set_selected_range(1..4, cx)));
        let path = fixture.directory.path().join("project/notes.txt");
        std::fs::write(&path, "Changed by an external writer\n").unwrap();
        wait(visual, |cx| {
            input.read(cx).value().as_ref() == "Changed by an external writer\n"
        });
        assert_eq!(
            controller.read_with(visual, |controller, _| controller.editor("notes.txt")),
            Some(input.clone())
        );
        assert_eq!(
            input.read_with(visual, |input, _| input.selected_range()),
            1..4
        );
        edit(&panel, visual, &id, "A draft survives the watcher\n");
        std::fs::write(&path, "Another external value\n").unwrap();
        std::fs::write(
            fixture.directory.path().join("project/watch-ready.txt"),
            "ready",
        )
        .unwrap();
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("watch-ready.txt")
        });
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            "A draft survives the watcher\n"
        );
        visual.simulate_keystrokes("secondary-w");
        let (_, detail) = crate::prompts::tests::wait(visual);
        assert!(detail.contains("notes.txt"));
        crate::prompts::tests::answer(visual, "settings_cancel");
        assert_eq!(document(&panel, visual, "notes.txt"), id);
        visual.simulate_keystrokes("secondary-q");
        wait(visual, |_| true);
        assert!(visual.has_pending_prompt());
        crate::prompts::tests::answer(visual, "settings_cancel");
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            "A draft survives the watcher\n"
        );
        enable(&fixture, false);
        wait(visual, |cx| panel.read(cx).mounted.is_none());
        visual.simulate_keystrokes("secondary-q");
        wait(visual, |_| true);
        assert!(visual.has_pending_prompt());
        crate::prompts::tests::answer(visual, "settings_cancel");
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "Another external value\n"
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn retains_loaded_directory_pages_and_editor_selection(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let directory = fixture.directory.path().join("project/many");
        std::fs::create_dir(&directory).unwrap();
        for index in 0..1_100 {
            std::fs::write(
                directory.join(format!("file-{index:04}.txt")),
                "page data\n",
            )
            .unwrap();
        }
        let (shell, visual) = mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual);
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        click(visual, "resource-file-notes.txt");
        let id = document(&panel, visual, "notes.txt");
        edit(
            &panel,
            visual,
            &id,
            "Retained while directory pages change\n",
        );
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        let input = controller.read_with(visual, |controller, _| {
            controller.editor("notes.txt").unwrap()
        });
        visual.update(|_, cx| input.update(cx, |input, cx| input.set_selected_range(1..4, cx)));
        click(visual, "resource-file-many");
        wait(visual, |cx| snapshot(&panel, cx).contains("file-0499.txt"));
        assert!(
            !visual
                .update(|_, cx| snapshot(&panel, cx))
                .contains("file-0500.txt")
        );
        bottom(visual);
        click(visual, "resource-file-many\0more");
        wait(visual, |cx| snapshot(&panel, cx).contains("file-0999.txt"));
        assert!(
            !visual
                .update(|_, cx| snapshot(&panel, cx))
                .contains("file-1099.txt")
        );
        std::fs::write(directory.join("aa-watched.txt"), "watched").unwrap();
        wait(visual, |cx| snapshot(&panel, cx).contains("aa-watched.txt"));
        wait(visual, |cx| snapshot(&panel, cx).contains("file-0799.txt"));
        bottom(visual);
        click(visual, "resource-file-many\0more");
        wait(visual, |cx| snapshot(&panel, cx).contains("file-1099.txt"));
        let state = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(!state.contains("Load more"));
        assert_eq!(
            controller.read_with(visual, |controller, _| controller.editor("notes.txt")),
            Some(input.clone())
        );
        assert_eq!(
            input.read_with(visual, |input, _| input.selected_range()),
            1..4
        );
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            "Retained while directory pages change\n"
        );
        assert_eq!(
            visual.update(|_, cx| value(&panel, &id, cx)["dirty"].clone()),
            true
        );
        assert!(fixture.transport.requests.lock().unwrap().iter().any(|request| {
            matches!(&request.command, Command::ListDirectory { path, after: Some(_), .. } if path == "many")
                && request.plugin.as_ref().is_some_and(|context| context.worktree == fixture.binding.worktree)
        }));
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        drop(shell);
        fixture.close();
    }
}

fn bottom(visual: &mut VisualTestContext) {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = visual.debug_bounds("files-tree").unwrap();
    visual.simulate_event(ScrollWheelEvent {
        position: bounds.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-40_000.))),
        touch_phase: TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}
