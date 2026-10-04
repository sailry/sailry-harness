use super::*;
use crate::conversation::live::tests::{
    AttachmentControl, attachment_control,
    fixture::{self, Fixture, ImageView, init, tap},
    wait,
};
use crate::conversation::live::{Binding as ConversationBinding, View};
use core::prelude::v1::test;
use image::{ImageEncoder as _, Rgba, RgbaImage};
use std::path::PathBuf;

mod replies;
mod tools;

fn png(width: u32, height: u32) -> Vec<u8> {
    let pixels = RgbaImage::from_pixel(width, height, Rgba([220, 30, 60, 128]));
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
        .unwrap();
    bytes
}

mod decoding {
    use super::*;

    #[test]
    fn preserves_pixel_properties() {
        let image = decode::decode(&png(256, 128), image::ImageFormat::Png, 128).unwrap();
        assert_eq!(image.size(0), size(DevicePixels(128), DevicePixels(64)));
        assert_eq!(&image.as_bytes(0).unwrap()[..4], &[60, 30, 220, 128]);
        assert_eq!(image.frame_count(), 1);
    }

    #[test]
    fn rejects_invalid_images() {
        assert!(decode::decode(b"not a PNG", image::ImageFormat::Png, 128).is_err());
        assert!(decode::decode(&png(16_385, 1), image::ImageFormat::Png, 128).is_err());
        assert!(decode::decode(&png(4097, 4097), image::ImageFormat::Png, 128).is_err());
    }

    #[test]
    fn applies_jpeg_orientation() {
        let pixels = image::RgbImage::from_pixel(6, 4, image::Rgb([220, 30, 60]));
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut bytes);
        // Little-endian TIFF containing the rotate-90 orientation tag.
        encoder
            .set_exif_metadata(vec![
                73, 73, 42, 0, 8, 0, 0, 0, 1, 0, 18, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
            ])
            .unwrap();
        encoder
            .write_image(&pixels, 6, 4, image::ExtendedColorType::Rgb8)
            .unwrap();
        let image = decode::decode(&bytes, image::ImageFormat::Jpeg, 128).unwrap();
        assert_eq!(image.size(0), size(DevicePixels(4), DevicePixels(6)));
    }

    #[test]
    fn keeps_webp_alpha() {
        let pixels = RgbaImage::from_pixel(6, 4, Rgba([220, 30, 60, 128]));
        let mut bytes = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
            .write_image(&pixels, 6, 4, image::ExtendedColorType::Rgba8)
            .unwrap();
        let image = decode::decode(&bytes, image::ImageFormat::WebP, 128).unwrap();
        assert_eq!(&image.as_bytes(0).unwrap()[..4], &[60, 30, 220, 128]);
    }

    #[test]
    fn first_gif_frame() {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            encoder
                .encode_frames(
                    [[220, 30, 60, 255], [30, 60, 220, 255]]
                        .into_iter()
                        .map(|color| image::Frame::new(RgbaImage::from_pixel(6, 4, Rgba(color)))),
                )
                .unwrap();
        }
        let image = decode::decode(&bytes, image::ImageFormat::Gif, 128).unwrap();
        assert_eq!(image.frame_count(), 1);
        assert_eq!(&image.as_bytes(0).unwrap()[..4], &[60, 30, 220, 255]);
    }
}

fn attach(view: &Entity<View>, cx: &mut VisualTestContext, path: PathBuf) -> Attachment {
    cx.update(|window, cx| view.update(cx, |view, cx| view.add_images(vec![path], window, cx)));
    wait(cx, |cx| view.read(cx).image_ready());
    view.read_with(cx, |view, _| {
        view.image_attachments().last().unwrap().clone()
    })
}

fn finish_close(visual: &mut VisualTestContext) {
    let duration = visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        cx.theme().motion_tokens().duration_normal
    });
    visual.executor().advance_clock(duration);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.run_until_parked();
}

#[gpui::test]
fn previews_unbound_drafts(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::with_tools(false, vec![]);
    let path = fixture.directory.path().join("draft.png");
    std::fs::write(&path, png(256, 128)).unwrap();
    let binding = ConversationBinding {
        project: None,
        worktree: None,
        ..fixture.binding.clone()
    };
    let (view, visual) = fixture::open_session(cx, binding, None);
    visual.update(|window, cx| view.update(cx, |view, cx| view.add_images(vec![path], window, cx)));
    let images = view.read_with(visual, |view, _| view.image_cache());
    wait(visual, |cx| {
        images
            .read(cx)
            .entries
            .get(&Key::Local(0))
            .is_some_and(|entry| matches!(entry.result, Some(Ok(_))))
    });
    assert!(view.read_with(visual, |view, _| view.image_staged()));
    let first = visual.debug_bounds("attachment-media-0").unwrap();
    assert!(first.size.width > px(32.) && first.size.width <= px(64.));
    assert_eq!(first.size.width, first.size.height);
    tap(visual, "attachment-card-0");
    assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    tap(visual, "image-download");
    let destination = fixture.directory.path().join("local-download.png");
    visual.simulate_new_path_selection(|_| Some(destination.clone()));
    fixture::finish_download(visual, &destination);
    assert_eq!(std::fs::read(destination).unwrap(), png(256, 128));
    visual.simulate_keystrokes("escape");
    finish_close(visual);
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    let second = fixture.directory.path().join("portrait.png");
    std::fs::write(&second, png(128, 256)).unwrap();
    visual
        .update(|window, cx| view.update(cx, |view, cx| view.add_images(vec![second], window, cx)));
    wait(visual, |cx| {
        images
            .read(cx)
            .entries
            .get(&Key::Local(1))
            .is_some_and(|entry| matches!(entry.result, Some(Ok(_))))
    });
    let first = visual.debug_bounds("attachment-media-0").unwrap();
    let second = visual.debug_bounds("attachment-media-1").unwrap();
    assert_eq!(first.size.width, first.size.height);
    assert_eq!(second.size, first.size);
    assert!(first.right() < second.left());
    let published = fixture.node.profile().join("attachments");
    assert!(!published.exists() || std::fs::read_dir(published).unwrap().next().is_none());
    visual.update(|window, _| window.remove_window());
    drop(view);
    drop(images);
    fixture.close();
}

#[gpui::test]
fn preview_and_download(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let path = fixture.directory.path().join("图片.png");
        let bytes = png(256, 128);
        std::fs::write(&path, &bytes).unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let attachment = attach(&view, visual, path);
        let images = view.read_with(visual, |view, _| view.image_cache());
        wait(visual, |cx| {
            images
                .read(cx)
                .entries
                .get(&Key::Published(attachment.id))
                .is_some_and(|entry| matches!(entry.result, Some(Ok(_))))
        });
        // The Kit card's click layer is exercised separately from its action buttons.
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let card = visual.debug_bounds("attachment-card-0").unwrap();
        visual.simulate_click(card.center(), Modifiers::default());
        visual.run_until_parked();
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        // Kit animates dialog bounds on the wall clock before its controls settle.
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        tap(visual, "image-download");
        assert!(visual.did_prompt_for_new_path());
        let destination = fixture.directory.path().join("download.png");
        visual.simulate_new_path_selection(|_| Some(destination.clone()));
        fixture::finish_download(visual, &destination);
        assert_eq!(std::fs::read(destination).unwrap(), bytes);
        visual.simulate_keystrokes("escape");
        finish_close(visual);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(!visual.did_prompt_for_paths());
        visual.update(|window, cx| view.update(cx, |view, cx| view.focus(window, cx)));
        visual.simulate_keystrokes("shift-tab shift-tab");
        let keystroke = Keystroke::parse("space").unwrap();
        visual.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        visual.simulate_event(KeyUpEvent { keystroke });
        visual.run_until_parked();
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        visual.simulate_keystrokes("escape");
        finish_close(visual);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(view.read_with(visual, |view, _| view.image_count()), 1);
        attachment_control(visual, 0, AttachmentControl::Remove);
        wait(visual, |cx| view.read(cx).image_count() == 0);
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(images);
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn failed_and_closed_previews(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let path = fixture.directory.path().join("invalid.png");
        std::fs::write(&path, b"invalid image bytes").unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let attachment = attach(&view, visual, path);
        let images = view.read_with(visual, |view, _| view.image_cache());
        let preview =
            visual.update(|window, cx| preview::open(&images, attachment.clone(), window, cx));
        wait(visual, |cx| matches!(preview.read(cx).result, Some(Err(_))));
        tap(visual, "image-retry");
        wait(visual, |cx| matches!(preview.read(cx).result, Some(Err(_))));
        visual.simulate_keystrokes("escape");
        finish_close(visual);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        drop(preview);
        let slots = images.read_with(visual, |images, _| images.slots.clone());
        wait(visual, |_| slots.available_permits() == 2);
        let permits = fixture
            .runtime
            .block_on(slots.acquire_many_owned(2))
            .unwrap();
        let preview = visual.update(|window, cx| preview::open(&images, attachment, window, cx));
        visual.simulate_keystrokes("escape");
        finish_close(visual);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        drop(permits);
        assert!(preview.read_with(visual, |view, _| view.result.is_none()));
        drop(preview);
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(images);
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn cache_eviction(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::with_tools(false, vec![]);
    let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
    let images = visual.new(|cx| {
        Images::new(
            fixture.binding.client.clone(),
            fixture.binding.runtime.clone(),
            Arc::new(tokio::sync::Semaphore::new(2)),
            cx,
        )
    });
    let slots = images.read_with(visual, |images, _| images.slots.clone());
    let permits = fixture
        .runtime
        .block_on(slots.acquire_many_owned(2))
        .unwrap();
    let attachments: Vec<_> = (0..65)
        .map(|_| Attachment {
            id: AttachmentId::new(),
            spec: sailry_protocol::attachment::Spec {
                worktree: fixture.binding.worktree.unwrap(),
                name: "cache.png".into(),
                media_type: "image/png".into(),
                size: 0,
                revision: blake3::hash(&[]).to_hex().to_string(),
            },
        })
        .collect();
    visual.update(|window, cx| {
        images.update(cx, |images, cx| {
            for attachment in &attachments[..64] {
                images.request(&ImageSource::from(attachment), window, cx);
            }
            let evicted = images.entries[&Key::Published(attachments[1].id)]
                .cancel
                .clone();
            images.request(&ImageSource::from(&attachments[0]), window, cx);
            images.request(&ImageSource::from(&attachments[64]), window, cx);
            assert_eq!(images.entries.len(), 64);
            assert!(
                images
                    .entries
                    .contains_key(&Key::Published(attachments[0].id))
            );
            assert!(
                !images
                    .entries
                    .contains_key(&Key::Published(attachments[1].id))
            );
            assert!(evicted.is_cancelled());
        })
    });
    let cancelled = images.read_with(visual, |images, _| {
        images
            .entries
            .values()
            .map(|entry| entry.cancel.clone())
            .collect::<Vec<_>>()
    });
    visual.update(|window, _| {
        drop(images);
        window.remove_window();
    });
    drop(view);
    visual.run_until_parked();
    assert!(cancelled.iter().all(CancellationToken::is_cancelled));
    drop(permits);
    fixture.close();
}

#[gpui::test]
fn rejects_changed_bytes_and_metadata(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let path = fixture.directory.path().join("verified.png");
        std::fs::write(&path, png(32, 16)).unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let attachment = attach(&view, visual, path);
        let slots = Arc::new(tokio::sync::Semaphore::new(2));
        let load = |attachment| {
            fixture.runtime.block_on(load::load(
                fixture.binding.client.clone(),
                attachment,
                128,
                CancellationToken::new(),
                slots.clone(),
            ))
        };
        assert!(load(attachment.clone()).is_ok());
        let mut changed = attachment.clone();
        changed.spec.name = "unexpected.png".into();
        assert!(load(changed).is_err());
        let stored = fixture
            .directory
            .path()
            .join("node/attachments")
            .join(attachment.id.to_string());
        let mut bytes = std::fs::read(&stored).unwrap();
        bytes[0] ^= 1;
        std::fs::write(&stored, bytes).unwrap();
        assert!(load(attachment).is_err());
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[test]
fn scoped_file_downloads_follow_package_availability() {
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let bytes = png(32, 16);
        std::fs::write(fixture.directory.path().join("project/image.png"), &bytes).unwrap();
        let context = fixture.files_context();
        let source = ImageSource::File {
            worktree: fixture.binding.worktree.unwrap(),
            path: "image.png".into(),
            context: Some(context.clone()),
        };
        let load = || {
            fixture.runtime.block_on(load::load(
                fixture.binding.client.clone(),
                source.clone(),
                128,
                CancellationToken::new(),
                Arc::new(tokio::sync::Semaphore::new(2)),
            ))
        };
        assert!(load().is_ok());
        let download = |name: &str| {
            let target = fixture.directory.path().join(name);
            let (status, receiver) =
                tokio::sync::watch::channel(crate::downloads::transfer::Status::Preparing);
            fixture.runtime.block_on(crate::downloads::transfer::run(
                fixture.binding.client.clone(),
                crate::downloads::transfer::Source::ScopedFile {
                    context: context.clone(),
                    path: "image.png".into(),
                },
                target.clone(),
                CancellationToken::new(),
                status,
            ));
            let result = receiver.borrow().clone();
            (target, result)
        };
        let (target, status) = download("download.png");
        assert!(matches!(
            status,
            crate::downloads::transfer::Status::Finished(Ok(()))
        ));
        assert_eq!(std::fs::read(target).unwrap(), bytes);
        let sailry_protocol::Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
            name: context.package.name.clone(),
        }) else {
            panic!("Files package expected");
        };
        fixture.execute(Command::SetPluginEnabled {
            name: context.package.name.clone(),
            expected_revision: info.summary.revision,
            enabled: false,
        });
        assert!(load().is_err());
        let (target, status) = download("disabled.png");
        assert!(matches!(
            status,
            crate::downloads::transfer::Status::Finished(Err(_))
        ));
        assert!(!target.exists());
        fixture.close();
    }
}
