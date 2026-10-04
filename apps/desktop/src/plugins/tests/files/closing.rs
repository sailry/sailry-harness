use super::*;

#[gpui::test]
fn confirms_the_captured_tab_and_rejects_a_released_view(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let other = fixture.directory.path().join("project/other.txt");
        std::fs::write(&other, "Other content\n").unwrap();
        let notes = fixture.directory.path().join("project/notes.txt");
        let original = std::fs::read_to_string(&notes).unwrap();
        install(&fixture);
        let (shell, visual) = mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual);
        click(visual, "resource-file-notes.txt");
        let id = document(&panel, visual, "notes.txt");
        edit(&panel, visual, &id, "A retained draft\n");
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        let input = controller.read_with(visual, |controller, _| {
            controller.editor("notes.txt").unwrap()
        });
        click(visual, "resource-file-other.txt");
        let other_id = document(&panel, visual, "other.txt");
        let close = Box::leak(format!("file-tab-close-{id}").into_boxed_str());
        click(visual, close);
        let (title, detail) = crate::prompts::tests::wait(visual);
        assert_eq!(title, crate::tr("files_discard_title").as_ref());
        assert!(detail.starts_with("notes.txt\n\n"));
        assert!(detail.contains(crate::tr("files_discard_description").as_ref()));
        assert!(!detail.contains("other.txt"));
        assert!(
            !visual
                .update(|_, cx| snapshot(&panel, cx))
                .contains("files-discard-dialog")
        );
        crate::prompts::tests::answer(visual, "settings_cancel");
        assert_eq!(document(&panel, visual, "notes.txt"), id);
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            "A retained draft\n"
        );

        click(visual, close);
        crate::prompts::tests::wait(visual);
        enable(&fixture, false);
        wait(visual, |cx| panel.read(cx).mounted.is_none());
        crate::prompts::tests::answer(visual, "files_discard");
        enable(&fixture, true);
        ready(&panel, visual);
        assert_eq!(document(&panel, visual, "notes.txt"), id);
        assert_eq!(
            controller.read_with(visual, |controller, _| controller.editor("notes.txt")),
            Some(input.clone())
        );
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            "A retained draft\n"
        );

        click(
            visual,
            Box::leak(format!("file-tab-{other_id}").into_boxed_str()),
        );
        click(visual, close);
        crate::prompts::tests::answer(visual, "files_discard");
        wait(visual, |cx| {
            !documents(&panel, cx)["documents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|document| document["id"] == id)
        });
        assert_eq!(document(&panel, visual, "other.txt"), other_id);
        assert!(
            visual
                .update(|_, cx| snapshot(&panel, cx))
                .contains(&format!("file-editor-{other_id}"))
        );
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "Other content\n");
        assert_eq!(std::fs::read_to_string(&notes).unwrap(), original);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        drop(shell);
        fixture.close();
    }
}
