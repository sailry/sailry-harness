use super::*;

#[gpui::test]
fn composes_native_editors_find_and_image_previews(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let root = fixture.directory.path().join("project");
        std::fs::write(root.join("notes.txt"), "needle\nneedle\nneedle\n").unwrap();
        let markdown = "# Original\r\n\r\nPlain **bold** text\r\n";
        std::fs::write(root.join("note.md"), markdown).unwrap();
        std::fs::create_dir_all(root.join("assets/generated")).unwrap();
        image::RgbaImage::from_pixel(32, 24, image::Rgba([255, 100, 10, 255]))
            .save(root.join("assets/generated/picture.PNG"))
            .unwrap();
        let (shell, visual) = mount(&fixture, remote, cx);
        let panel = main(&shell, &fixture, visual);
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        click(visual, "resource-file-notes.txt");
        let id = document(&panel, visual, "notes.txt");
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        let input = controller.read_with(visual, |controller, _| {
            controller.editor("notes.txt").unwrap()
        });
        assert!(
            !panel.read_with(visual, |panel, _| panel.loading),
            "Files panel is still loading"
        );
        click(visual, "files_find");
        wait(visual, |cx| input.read(cx).search_session().open);
        visual.simulate_input("needle");
        wait(visual, |cx| {
            input.read(cx).search_session().query == "needle"
        });
        let first = input.read_with(visual, |input, _| {
            input.search_session().matcher.current_match_index()
        });
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            input
                .read(cx)
                .search_session()
                .matcher
                .current_match_index()
                != first
        });
        visual.simulate_keystrokes("shift-enter");
        wait(visual, |cx| {
            input
                .read(cx)
                .search_session()
                .matcher
                .current_match_index()
                == first
        });
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| !input.read(cx).search_session().open);
        assert_eq!(
            visual.update(|_, cx| value(&panel, &id, cx)["dirty"].clone()),
            false
        );

        click(visual, "resource-file-note.md");
        let markdown_id = document(&panel, visual, "note.md");
        wait(visual, |cx| {
            value(&panel, &markdown_id, cx)["mode"] == "document"
        });
        let source = controller.read_with(visual, |controller, _| {
            controller.editor("note.md").unwrap()
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let heading = visual.debug_bounds("markdown-editor-block-0").unwrap();
        visual.simulate_click(
            point(heading.right() - px(1.), heading.center().y),
            Modifiers::default(),
        );
        visual.simulate_input(" Revised 中文");
        wait(visual, |cx| {
            source.read(cx).value().as_ref()
                == markdown.replacen("Original", "Original Revised 中文", 1)
        });
        visual.simulate_keystrokes("secondary-s");
        wait(visual, |cx| {
            value(&panel, &markdown_id, cx)["dirty"] == false
        });
        assert_eq!(
            std::fs::read(root.join("note.md")).unwrap(),
            markdown
                .replacen("Original", "Original Revised 中文", 1)
                .as_bytes()
        );
        click(visual, "files_find");
        wait(visual, |cx| {
            value(&panel, &markdown_id, cx)["mode"] == "source"
                && source.read(cx).search_session().open
        });
        visual.simulate_input("Revised");
        wait(visual, |cx| {
            source.read(cx).search_session().query == "Revised"
        });
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| !source.read(cx).search_session().open);

        for path in ["assets", "assets/generated", "assets/generated/picture.PNG"] {
            wait(visual, |cx| {
                snapshot(&panel, cx).contains(path.rsplit('/').next().unwrap())
            });
            click(
                visual,
                Box::leak(format!("resource-file-{path}").into_boxed_str()),
            );
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while visual.debug_bounds("image-lightbox-image").is_none() {
            wait(visual, |_| true);
            assert!(
                std::time::Instant::now() < deadline,
                "file tree image deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(visual.debug_bounds("image-zoom-in").is_some());
        assert!(visual.debug_bounds("image-download").is_some());
        assert_eq!(
            visual.update(|_, cx| documents(&panel, cx)["documents"].as_array().unwrap().len()),
            2
        );
        visual.update(|window, cx| window.close_dialog(cx));
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        drop(source);
        drop(shell);
        fixture.close();
    }
}
