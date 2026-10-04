use super::*;
use sailry_protocol::conversation::Part;

#[gpui::test]
fn mixed_content(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let bytes = png(256, 64);
        let fixture = tools::fixture(remote, &bytes);
        let project = fixture
            .directory
            .path()
            .join("project")
            .canonicalize()
            .unwrap();
        for name in ["first.png", "second.png"] {
            std::fs::write(project.join(name), &bytes).unwrap();
        }
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1100.), px(1600.)));
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).image_history().calls.len() == 1
        });
        let image = view.read_with(visual, |view, _| {
            let history = view.image_history();
            history.calls[0].images(&history.snapshot.as_ref().unwrap().page)[0].clone()
        });
        let native = image.attachment.id;
        view.update(visual, |view, cx| {
            view.set_reply(
                vec![
                    Part::Text("Before native image".into()),
                    Part::Image(image),
                    Part::Text(format!(
                        "Before ![First](first.png) between [Download]({}) after",
                        project.join("second.png").display()
                    )),
                ],
                cx,
            )
        });
        let images = view.read_with(visual, |view, _| view.image_cache());
        let worktree = fixture.session.worktree;
        wait(visual, |cx| {
            [
                Key::Published(native),
                Key::File(worktree, "first.png".into()),
                Key::File(worktree, "second.png".into()),
            ]
            .iter()
            .all(|key| {
                let entry = images.read(cx).entries.get(key);
                if let Some(Entry {
                    result: Some(Err(error)),
                    ..
                }) = entry
                {
                    panic!("reply image failed: {error}");
                }
                entry.is_some_and(|entry| matches!(entry.result, Some(Ok(_))))
            })
        });
        let first = format!("image-card-file-{worktree}-first.png");
        let second = format!("image-media-file-{worktree}-second.png");
        let bounds = |visual: &mut VisualTestContext, selector: String| {
            visual
                .debug_bounds(Box::leak(selector.into_boxed_str()))
                .unwrap()
        };
        assert!(
            bounds(visual, format!("image-card-{native}")).bottom() <= bounds(visual, first).top()
        );
        tap(visual, &second);
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        tap(visual, "image-download");
        let destination = fixture.directory.path().join("original.png");
        visual.simulate_new_path_selection(|_| Some(destination.clone()));
        fixture::finish_download(visual, &destination);
        assert_eq!(std::fs::read(destination).unwrap(), bytes);
        visual.simulate_keystrokes("escape");
        finish_close(visual);
        visual.update(|window, _| window.remove_window());
        drop(images);
        drop(view);
        fixture.close();
    }
}
