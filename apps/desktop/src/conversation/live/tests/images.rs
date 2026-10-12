use super::*;
use crate::{backend::Services, shell::Shell};

const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#c33"/></svg>"##;

#[derive(Clone, Copy, PartialEq)]
enum Shows {
    ImageCard,
    FileCard,
}

fn settle(visual: &mut VisualTestContext, selector: Option<&'static str>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    for _ in 0..30 {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        if selector.is_some_and(|selector| visual.debug_bounds(selector).is_some()) {
            return;
        }
        assert!(Instant::now() < deadline, "image update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(selector.is_none(), "missing {selector:?}");
}

fn check(cx: &mut TestAppContext, remote: bool, path: &str, bytes: Option<&[u8]>, shows: Shows) {
    fixture::init(cx);
    cx.update(crate::shell::init);
    let fixture = fixture::Fixture::with_server(remote, |runtime| {
        runtime.block_on(support::Server::markdown(format!(
            "Before\n\n![Drawing caption]({path})\n\nAfter"
        )))
    });
    if let Some(bytes) = bytes {
        let file = fixture.directory.path().join("project").join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    }
    cx.update(|cx| {
        cx.set_global(Services {
            profile: fixture.node.profile().to_path_buf(),
            runtime: fixture.runtime.clone(),
            local: fixture.node.local(),
            link: fixture.node.link(),
            relay_enabled: false,
        });
        cx.set_global(crate::preferences::Preferences::default());
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        shell.update(cx, |shell, cx| {
            let live = shell.live.as_mut().unwrap();
            live.override_transport(fixture.transport.clone());
            live.select(fixture.node.id(), cx);
            shell.reveal_session(fixture.session.clone(), window, cx);
        });
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1440.), px(900.)));
    wait(visual, |cx| {
        shell
            .read(cx)
            .current_chat()
            .is_some_and(|chat| chat.read(cx).connected())
    });
    let chat = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
    fixture.start();
    wait(visual, |cx| {
        chat.read(cx)
            .history
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| {
                snapshot
                    .page
                    .runs
                    .iter()
                    .any(|run| run.status == Status::Completed)
            })
    });
    let card = format!("image-card-file-{}-{path}", fixture.session.worktree);
    let card: &'static str = Box::leak(card.into_boxed_str());
    let first = match shows {
        Shows::ImageCard => card,
        Shows::FileCard => "artifact-file-title",
    };
    settle(visual, Some(first));
    // A failed load replaces the card with the file fallback.
    settle(visual, None);
    let link: &'static str = Box::leak(format!("image-link-{path}").into_boxed_str());
    assert!(visual.debug_bounds(link).is_none(), "no ghost button");
    let picture = visual.debug_bounds("markdown-image-picture").unwrap();
    let caption = visual.debug_bounds("markdown-image-caption").unwrap();
    assert!(
        caption.top() >= picture.bottom(),
        "caption {caption:?} overlaps picture {picture:?}"
    );
    match shows {
        Shows::ImageCard => {
            assert!(visual.debug_bounds(card).is_some());
            assert!(visual.debug_bounds("artifact-file-title").is_none());
        }
        Shows::FileCard => {
            assert!(visual.debug_bounds(card).is_none());
            assert!(visual.debug_bounds("artifact-file-title").is_some());
        }
    }
}

#[gpui::test]
fn svg_renders_image_card_locally(cx: &mut TestAppContext) {
    check(cx, false, "output/a.svg", Some(SVG), Shows::ImageCard);
}

#[gpui::test]
fn svg_renders_image_card_remotely(cx: &mut TestAppContext) {
    check(cx, true, "output/a.svg", Some(SVG), Shows::ImageCard);
}

#[gpui::test]
fn unsupported_format_shows_file_card(cx: &mut TestAppContext) {
    check(cx, false, "output/a.pdf", Some(b"%PDF"), Shows::FileCard);
}

#[gpui::test]
fn missing_file_shows_file_card(cx: &mut TestAppContext) {
    check(cx, false, "output/missing.png", None, Shows::FileCard);
}

#[gpui::test]
fn script_svg_shows_file_card(cx: &mut TestAppContext) {
    let hostile =
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><script>1</script></svg>"#;
    check(cx, false, "output/bad.svg", Some(hostile), Shows::FileCard);
}
