use super::*;

fn drop_files(cx: &mut VisualTestContext, paths: &[PathBuf], submit: bool) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let position = cx.debug_bounds("live-chat-input").unwrap().center();
    cx.simulate_event(FileDropEvent::Entered {
        position,
        paths: ExternalPaths(paths.iter().cloned().collect()),
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    if submit {
        cx.simulate_event(FileDropEvent::Submit { position });
    } else {
        cx.simulate_event(FileDropEvent::Exited);
    }
    cx.run_until_parked();
}

#[gpui::test]
fn paste_preserves_text(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let path = fixture.directory.path().join("粘贴内容.txt");
        std::fs::write(&path, "complete pasted file").unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("keep ");
        visual.update(|_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string("plain text 🙂".into()))
        });
        visual.simulate_keystrokes("secondary-v");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "keep plain text 🙂"
        );
        visual.update(|_, cx| {
            cx.write_to_clipboard(
                ClipboardEntry::ExternalPaths(ExternalPaths([path.clone()].into_iter().collect()))
                    .into(),
            )
        });
        visual.simulate_keystrokes("secondary-v");
        let files = ready(visual, &view, 1);
        assert_eq!(files[0].spec.name, "粘贴内容.txt");
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(8, 4)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let bytes = bytes.into_inner();
        visual.update(|_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
                ImageFormat::Png,
                bytes.to_vec(),
            )))
        });
        visual.simulate_keystrokes("secondary-v");
        let files = ready(visual, &view, 2);
        assert_eq!(files[1].spec.media_type, "image/png");
        assert_eq!(
            files[1].spec.revision,
            blake3::hash(&bytes).to_hex().as_str()
        );
        let Output::AttachmentDownload(download) = fixture.execute(Command::DownloadAttachment {
            worktree: fixture.binding.worktree.unwrap(),
            attachment: files[1].id,
        }) else {
            panic!("attachment download expected");
        };
        let mut result = Vec::new();
        fixture
            .runtime
            .block_on(fixture.binding.client.download_attachment(
                &download,
                &mut result,
                CancellationToken::new(),
                |_| {},
            ))
            .unwrap();
        assert_eq!(result, bytes);
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "keep plain text 🙂"
        );
        visual.update(|_, cx| {
            cx.write_to_clipboard(
                ClipboardEntry::ExternalPaths(ExternalPaths(vec![path; 7].into_iter().collect()))
                    .into(),
            )
        });
        visual.simulate_keystrokes("secondary-v");
        wait(visual, |cx| {
            view.read(cx).attachments.error == Some("chat_attachment_limit")
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.attachments.items.len()),
            2
        );
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.insert_file_references(vec![("src/参考.rs".into(), false)], window, cx);
            })
        });
        for mode in [
            gpui_kit::component::ThemeMode::Light,
            gpui_kit::component::ThemeMode::Dark,
        ] {
            visual.update(|window, cx| {
                gpui_kit::component::Theme::change(mode, Some(window), cx);
                window.draw(cx).clear(cx);
            });
            let file = visual.debug_bounds("attachment-card-0").unwrap();
            let image = visual.debug_bounds("attachment-card-1").unwrap();
            let reference = view.read_with(visual, |view, cx| {
                let input = view.input.read(cx);
                let label = "@src/参考.rs";
                let start = input.value().find(label).unwrap();
                assert_eq!(view.active_references(cx).len(), 1);
                input
                    .range_to_bounds(&(start..start + label.len()))
                    .unwrap()
            });
            let rem = visual.update(|window, _| window.rem_size());
            assert!((file.size.height - (rem * 3.5 + px(6.))).abs() < px(1.));
            assert!(file.size.width > file.size.height);
            assert!(
                reference.size.height <= px(36.),
                "inline references occupy one input row"
            );
            assert!(reference.size.width > reference.size.height);
            assert!((image.size.width - (rem * 3.5 + px(6.))).abs() < px(1.));
            assert!(
                (image.size.height - image.size.width).abs() < px(1.),
                "images use native square tiles"
            );
            assert!(file.right() <= image.left() || file.bottom() <= image.top());
            assert!(image.right() <= reference.left() || image.bottom() <= reference.top());
        }
        visual.update(|_, cx| {
            view.read(cx).input.clone().update(cx, |input, cx| {
                let start = input.value().find("@src/参考.rs").unwrap();
                input.set_selected_range(start..input.value().len(), cx);
            });
        });
        visual.simulate_keystrokes("backspace");
        assert!(view.read_with(visual, |view, cx| view.active_references(cx).is_empty()));
        assert_eq!(
            view.read_with(visual, |view, cx| view
                .input
                .read(cx)
                .value()
                .trim()
                .to_owned()),
            "keep plain text 🙂"
        );
        assert_eq!(
            view.read_with(visual, |view, _| view.attachments.items.len()),
            2
        );
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn drops_files_only_after_submission(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let path = fixture.directory.path().join("dropped.txt");
        std::fs::write(&path, "dropped file").unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_keystrokes("a");
        assert!(visual.update(|window, _| window.last_input_was_keyboard()));
        drop_files(visual, std::slice::from_ref(&path), false);
        assert!(view.read_with(visual, |view, _| view.attachments.items.is_empty()));
        visual.simulate_keystrokes("b");
        assert!(visual.update(|window, _| window.last_input_was_keyboard()));
        drop_files(visual, std::slice::from_ref(&path), true);
        let files = ready(visual, &view, 1);
        assert_eq!(files[0].spec.name, "dropped.txt");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "ab"
        );
        tap(visual, "live-chat-send");
        wait(visual, |_| {
            !fixture.server.requests.lock().unwrap().is_empty()
        });
        assert!(
            fixture.server.requests.lock().unwrap()[0]
                .to_string()
                .contains("dropped file")
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn pastes_native_bitmaps_as_png(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        for (index, (format, codec)) in [
            (ImageFormat::Tiff, image::ImageFormat::Tiff),
            (ImageFormat::Bmp, image::ImageFormat::Bmp),
        ]
        .into_iter()
        .enumerate()
        {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(8, 4)
                .write_to(&mut bytes, codec)
                .unwrap();
            visual.update(|_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
                    format,
                    bytes.into_inner(),
                )))
            });
            visual.simulate_keystrokes("secondary-v");
            let files = ready(visual, &view, index + 1);
            assert_eq!(files[index].spec.media_type, "image/png");
            assert!(files[index].spec.name.ends_with(".png"));
            let Output::AttachmentDownload(download) =
                fixture.execute(Command::DownloadAttachment {
                    worktree: fixture.binding.worktree.unwrap(),
                    attachment: files[index].id,
                })
            else {
                panic!("attachment download expected");
            };
            let mut result = Vec::new();
            fixture
                .runtime
                .block_on(fixture.binding.client.download_attachment(
                    &download,
                    &mut result,
                    CancellationToken::new(),
                    |_| {},
                ))
                .unwrap();
            let decoded =
                image::load_from_memory_with_format(&result, image::ImageFormat::Png).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (8, 4));
        }
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
