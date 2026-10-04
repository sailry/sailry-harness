use super::*;

#[gpui::test]
fn removal_does_not_open_gallery(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for staged in [false, true] {
            let fixture = Fixture::with_tools(remote, vec![]);
            let image = fixture.directory.path().join("diagram.png");
            image::DynamicImage::new_rgba8(8, 4).save(&image).unwrap();
            let (view, visual) = if staged {
                fixture::open_session(cx, fixture.binding.clone(), None)
            } else {
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone())
            };
            wait(visual, |cx| {
                view.read(cx).connected() && view.read(cx).configured()
            });
            choose(visual, &[image]);
            if staged {
                wait(visual, |cx| view.read(cx).attachments.sendable());
            } else {
                ready(visual, &view, 1);
            }
            control(visual, 0, Control::Remove);
            wait(visual, |cx| view.read(cx).attachments.items.is_empty());
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds("attachment-image-preview").is_none());
            assert!(fixture.server.requests.lock().unwrap().is_empty());
            visual.update(|window, _| window.remove_window());
            drop(view);
            fixture.close();
        }
    }
}

#[gpui::test]
fn separates_files_images_and_text(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for text in ["", "Review these attachments"] {
            let fixture = Fixture::with_tools(remote, vec![]);
            let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
                panic!("snapshot expected");
            };
            let mut provider = snapshot.providers[0].clone();
            provider.models[0].vision = true;
            fixture.execute(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            });
            let image = fixture.directory.path().join("diagram.png");
            image::DynamicImage::new_rgba8(8, 4).save(&image).unwrap();
            let file = fixture.directory.path().join("notes.txt");
            std::fs::write(&file, "Attachment contents").unwrap();
            let (view, visual) =
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
            visual.simulate_resize(size(px(1100.), px(1100.)));
            wait(visual, |cx| {
                view.read(cx).connected() && view.read(cx).configured()
            });
            // Input order stays intact; presentation groups each media kind.
            choose(visual, &[image, file]);
            let attachments = ready(visual, &view, 2);
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let surface = visual.debug_bounds("composer-surface").unwrap();
            for key in [0, 1] {
                let card = visual
                    .debug_bounds(Box::leak(format!("attachment-card-{key}").into_boxed_str()))
                    .unwrap();
                assert!(
                    surface.contains(&card.origin) && surface.contains(&card.bottom_right()),
                    "attachments belong inside the composer"
                );
            }
            let image_card = visual.debug_bounds("attachment-card-0").unwrap();
            let rem = visual.update(|window, _| window.rem_size());
            assert!((image_card.size.width - (rem * 3.5 + px(6.))).abs() < px(1.));
            assert!((image_card.size.height - image_card.size.width).abs() < px(1.));
            if !text.is_empty() {
                tap(visual, "live-chat-input");
                visual.simulate_input(text);
            }
            send(visual);
            view.read_with(visual, |view, _| {
                assert!(
                    view.pending || view.attachments.items.is_empty(),
                    "send did not start: remote={remote}, text={text:?}, busy={}, connected={}, configured={}, sendable={}",
                    view.busy(),
                    view.connected(),
                    view.configured(),
                    view.attachments.sendable()
                );
            });
            wait(visual, |cx| {
                let view = view.read(cx);
                assert!(view.error.is_none(), "send failed: {:?}", view.error);
                view.history.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.page.runs.iter().any(|run| {
                        assert!(run.error.is_none(), "turn failed: {:?}", run.error);
                        run.status == sailry_protocol::conversation::Status::Completed
                    })
                })
            });
            let turn = view.read_with(visual, |view, _| {
                let entry = view
                    .history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .entries
                    .iter()
                    .find(|entry| entry.author == "user")
                    .unwrap();
                let ids: Vec<_> = entry
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        Part::Attachment(attachment) => Some(attachment.id),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    ids,
                    attachments
                        .iter()
                        .map(|attachment| attachment.id)
                        .collect::<Vec<_>>()
                );
                entry.turn
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let file = visual
                .debug_bounds(Box::leak(
                    format!("file-card-{}", attachments[1].id).into_boxed_str(),
                ))
                .unwrap();
            let image = visual
                .debug_bounds(Box::leak(
                    format!("image-card-{}", attachments[0].id).into_boxed_str(),
                ))
                .unwrap();
            assert!(
                file.bottom() <= image.top(),
                "files and images occupy separate groups"
            );
            let bubble = visual.debug_bounds(Box::leak(
                format!("live-user-bubble-{turn}").into_boxed_str(),
            ));
            if text.is_empty() {
                assert!(
                    bubble.is_none(),
                    "attachments alone do not create an empty text bubble"
                );
            } else {
                assert!(
                    image.bottom() <= bubble.unwrap().top(),
                    "attachments are outside the text bubble"
                );
            }
            visual.update(|window, _| window.remove_window());
            drop(view);
            fixture.close();
        }
    }
}
