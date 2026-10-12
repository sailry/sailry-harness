use super::*;

fn image() -> Arc<[u8]> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode_image(&image::DynamicImage::new_rgb8(12, 8))
        .unwrap();
    bytes.into()
}

#[gpui::test]
fn publishes_selected_context_on_each_path(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let bytes = image();
        let reference = r##"{"page_element":{"url":"https://example.com/","selector":"#heading","text":"Visible heading"},"annotation":"Replace this heading"}"##;
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.input
                    .update(cx, |input, cx| input.set_value("Keep draft", window, cx));
                view.insert_browser_reference(reference, bytes.clone(), window, cx)
                    .unwrap();
            })
        });
        let files = ready(visual, &view, 1);
        assert_eq!(files[0].spec.name, "browser-element-0.jpg");
        assert_eq!(files[0].spec.media_type, "image/jpeg");
        assert_eq!(
            files[0].spec.revision,
            blake3::hash(&bytes).to_hex().as_str()
        );
        assert_eq!(files[0].spec.worktree, fixture.binding.worktree.unwrap());
        let Output::AttachmentDownload(download) = fixture.execute(Command::DownloadAttachment {
            worktree: fixture.binding.worktree.unwrap(),
            attachment: files[0].id,
        }) else {
            panic!("Attachment download expected");
        };
        let mut actual = Vec::new();
        fixture
            .runtime
            .block_on(fixture.binding.client.download_attachment(
                &download,
                &mut actual,
                CancellationToken::new(),
                |_| {},
            ))
            .unwrap();
        assert_eq!(actual.as_slice(), bytes.as_ref());
        view.read_with(visual, |view, cx| {
            assert_eq!(
                view.input.read(cx).value().as_ref(),
                format!("Keep draft\n\n> {reference}\n\n")
            );
            let content = view.input.read(cx).content();
            assert_eq!(content.tokens().len(), 1);
            assert_eq!(content.tokens()[0].token().text(), ">");
            assert_eq!(content.tokens()[0].token().label(), " ");
            assert!(view.active_references(cx).is_empty());
            assert_eq!(view.attachments.items.len(), 1);
        });
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn blocked_composers_retain_drafts_and_attachments(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::with_tools(false, vec![]);
    let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
    wait(visual, |cx| view.read(cx).connected());
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.input
                .update(cx, |input, cx| input.set_value("Keep draft", window, cx));
            view.pending = true;
            assert_eq!(
                view.insert_browser_reference("Selected element", image(), window, cx),
                Err("browser_annotation_stale")
            );
            assert_eq!(view.input.read(cx).value().as_ref(), "Keep draft");
            assert!(view.attachments.items.is_empty());
            view.pending = false;
            view.restore_attachment_sources(
                (0..8)
                    .map(|_| Source::Image {
                        name: "kept.jpg".into(),
                        format: ImageFormat::Jpeg,
                        bytes: image(),
                    })
                    .collect(),
                cx,
            );
            assert_eq!(
                view.insert_browser_reference("Selected element", image(), window, cx),
                Err("chat_attachment_limit")
            );
            assert_eq!(view.input.read(cx).value().as_ref(), "Keep draft");
            assert_eq!(view.attachments.items.len(), 8);
        })
    });
    assert_eq!(fixture.task_requests(), 0);
    visual.update(|window, _| window.remove_window());
    drop(view);
    fixture.close();
}
