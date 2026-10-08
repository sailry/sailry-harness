use super::*;
use sailry_protocol::conversation::Part;

#[gpui::test]
fn mixed_content(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let bytes = png(256, 64);
        let fixture = tools::fixture(remote, &[&bytes]);
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

#[gpui::test]
fn groups_native_and_markdown(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let bytes = [png(256, 64), png(64, 256), png(128, 128)];
        let fixture = tools::fixture(remote, &bytes.iter().map(Vec::as_slice).collect::<Vec<_>>());
        let project = fixture.directory.path().join("project");
        for (index, data) in bytes.iter().enumerate() {
            std::fs::write(project.join(format!("image-{index}.png")), data).unwrap();
        }
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).image_history().calls.len() == 1
        });
        let native = view.read_with(visual, |view, _| {
            let history = view.image_history();
            history.calls[0]
                .images(&history.snapshot.as_ref().unwrap().page)
                .to_vec()
        });
        assert_eq!(native.len(), 3);
        let images = view.read_with(visual, |view, _| view.image_cache());
        let handle = visual.update(|window, _| window.window_handle());
        for markdown in [false, true] {
            visual.simulate_window_resize(handle, size(px(1200.), px(1600.)));
            let source = "![First](image-0.png) ![Second](image-1.png) ![Third](image-2.png)";
            view.update(visual, |view, cx| {
                view.set_reply(
                    if markdown {
                        vec![Part::Text(source.into())]
                    } else {
                        std::iter::once(Part::Text("Images".into()))
                            .chain(native.iter().cloned().map(Part::Image))
                            .collect()
                    },
                    cx,
                )
            });
            let keys: Vec<_> = (0..3)
                .map(|index| {
                    if markdown {
                        Key::File(fixture.session.worktree, format!("image-{index}.png"))
                    } else {
                        Key::Published(native[index].attachment.id)
                    }
                })
                .collect();
            wait(visual, |cx| {
                keys.iter().all(|key| {
                    images
                        .read(cx)
                        .entries
                        .get(key)
                        .is_some_and(|entry| matches!(entry.result, Some(Ok(_))))
                })
            });
            let ids: Vec<_> = (0..3)
                .map(|index| {
                    if markdown {
                        format!("file-{}-image-{index}.png", fixture.session.worktree)
                    } else {
                        native[index].attachment.id.to_string()
                    }
                })
                .collect();
            let bounds = |visual: &mut VisualTestContext| {
                ids.iter()
                    .map(|id| {
                        visual
                            .debug_bounds(Box::leak(format!("image-card-{id}").into_boxed_str()))
                            .unwrap()
                    })
                    .collect::<Vec<_>>()
            };
            let cards = bounds(visual);
            assert!(
                cards
                    .iter()
                    .all(|card| card.top() == cards[0].top() && card.size.width == px(GROUP_WIDTH))
            );
            assert!(
                cards
                    .windows(2)
                    .all(|pair| pair[0].right() < pair[1].left())
            );
            visual.simulate_window_resize(handle, size(px(420.), px(1600.)));
            wait(visual, |_| true);
            let cards = bounds(visual);
            assert!(cards[2].top() > cards[0].top());
            assert!(cards.iter().all(|card| card.right() <= px(420.)));
            tap(visual, &format!("image-card-{}", ids[2]));
            assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
            visual.simulate_window_resize(handle, size(px(1200.), px(1600.)));
            std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
            visual.update(|window, cx| window.draw(cx).clear(cx));
            for (control, index) in [("image-next", 2), ("image-previous", 1)] {
                tap(visual, control);
                tap(visual, "image-download");
                let destination = fixture.directory.path().join(format!(
                    "{}-{index}-original.png",
                    if markdown { "markdown" } else { "native" }
                ));
                visual.simulate_new_path_selection(|_| Some(destination.clone()));
                fixture::finish_download(visual, &destination);
                assert_eq!(std::fs::read(destination).unwrap(), bytes[index]);
            }
            visual.simulate_keystrokes("left");
            wait(visual, |_| true);
            tap(visual, "image-download");
            let destination = fixture.directory.path().join(if markdown {
                "markdown-original.png"
            } else {
                "native-original.png"
            });
            visual.simulate_new_path_selection(|_| Some(destination.clone()));
            fixture::finish_download(visual, &destination);
            assert_eq!(std::fs::read(destination).unwrap(), bytes[0]);
            visual.simulate_keystrokes("escape");
            finish_close(visual);
            // The authored source remains unchanged when picture blocks share a row.
            if markdown {
                view.read_with(visual, |view, _| {
                    assert!(
                        view.image_history()
                            .snapshot
                            .as_ref()
                            .unwrap()
                            .page
                            .entries
                            .iter()
                            .flat_map(|entry| &entry.parts)
                            .any(|part| matches!(part, Part::Text(text) if text == source))
                    )
                });
            }
            visual.simulate_window_resize(handle, size(px(1200.), px(1600.)));
            view.update(visual, |view, cx| {
                view.set_reply(
                    if markdown {
                        vec![Part::Text("![First](image-0.png)".into())]
                    } else {
                        vec![Part::Text("Image".into()), Part::Image(native[0].clone())]
                    },
                    cx,
                )
            });
            wait(visual, |_| true);
            let single = visual
                .debug_bounds(Box::leak(
                    format!("image-media-{}", ids[0]).into_boxed_str(),
                ))
                .unwrap();
            assert_eq!(single.size.width, px(320.));
            assert_eq!(single.size.width, single.size.height * 4.);
        }
        visual.update(|window, _| window.remove_window());
        drop(images);
        drop(view);
        fixture.close();
    }
}
