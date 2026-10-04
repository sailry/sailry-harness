use super::*;

#[gpui::test]
fn opens_captured_lines_without_replacing_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let root = fixture.directory.path().join("project");
        std::fs::write(root.join("build notes.md"), "first\nsecond\nthird\n").unwrap();
        let root = root.canonicalize().unwrap();
        let (shell, visual) = mount(&fixture, remote, cx);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.session.clone(), window, cx);
                shell.open_conversation_link("build%20notes.md#L3".into(), window, cx);
            })
        });
        let panel = shell.read_with(visual, |shell, _| match &shell.side_resource {
            Some(SideResource::Plugin(panel)) => panel.clone(),
            _ => panic!("ordinary document renderer expected"),
        });
        ready(&panel, visual);
        let id = document(&panel, visual, "build notes.md");
        let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
        let editor = controller.read_with(visual, |controller, _| {
            controller.editor("build notes.md").unwrap()
        });
        wait(visual, |cx| editor.read(cx).cursor_position().line == 2);
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_value("draft first\ndraft second\ndraft third\n", window, cx)
            });
        });
        wait(visual, |cx| value(&panel, &id, cx)["dirty"] == true);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_conversation_link(
                    format!("{}/build notes.md:2", root.display()).into(),
                    window,
                    cx,
                );
            })
        });
        wait(visual, |cx| editor.read(cx).cursor_position().line == 1);
        assert_eq!(document(&panel, visual, "build notes.md"), id);
        assert_eq!(
            controller.read_with(visual, |controller, _| controller.editor("build notes.md")),
            Some(editor.clone())
        );
        assert_eq!(
            visual.update(|_, cx| documents(&panel, cx)["documents"].as_array().unwrap().len()),
            1
        );
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.value().to_string()),
            "draft first\ndraft second\ndraft third\n"
        );
        assert_eq!(
            visual.update(|_, cx| value(&panel, &id, cx)["dirty"].clone()),
            true
        );
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.session),
            Some(fixture.session.id)
        );
        if remote {
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.open_conversation_link("file:///outside/image.png".into(), window, cx);
                });
                assert_eq!(
                    crate::feedback::tests::summary(window, cx).as_ref(),
                    crate::tr("files_remote_path").as_ref()
                );
            });
        }
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(editor);
        drop(shell);
        fixture.close();
    }
}
