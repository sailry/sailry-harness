use super::*;
use crate::conversation::live::tests::{
    fixture::{self, Fixture, init, tap},
    wait,
};
use core::prelude::v1::test;
use sailry_protocol::conversation::Part;
use std::path::Path;

mod input;
mod layout;
mod lifecycle;
mod media;

pub(crate) enum Control {
    Remove,
    Retry,
}

fn send(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx.debug_bounds("live-chat-send").expect("send button");
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx.debug_bounds("live-chat-send").expect("send button");
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

pub(crate) fn control(cx: &mut VisualTestContext, key: usize, control: Control) {
    let position = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let selector = Box::leak(format!("attachment-card-{key}").into_boxed_str());
        let bounds = cx.debug_bounds(selector).expect("attachment card");
        match &control {
            Control::Remove => point(bounds.right() - px(10.), bounds.top() + px(10.)),
            Control::Retry => cx
                .debug_bounds(Box::leak(
                    format!("attachment-media-{key}").into_boxed_str(),
                ))
                .expect("native attachment media")
                .center(),
        }
    };
    cx.run_until_parked();
    let before = position(cx);
    cx.simulate_mouse_move(before, None, Modifiers::default());
    cx.run_until_parked();
    // Hover and queued toolbar renders can change the native control's geometry.
    let current = position(cx);
    cx.simulate_click(current, Modifiers::default());
    cx.run_until_parked();
}

fn choose(cx: &mut VisualTestContext, paths: &[PathBuf]) {
    tap(cx, "live-attach");
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && !options.directories && options.multiple);
        Some(paths.to_vec())
    });
}

fn ready(cx: &mut VisualTestContext, view: &Entity<View>, count: usize) -> Vec<Attachment> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let ready = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let view = view.read(cx);
            view.attachments.ready() && view.attachments.items.len() == count
        });
        if ready {
            break;
        }
        view.read_with(cx, |view, _| {
            assert!(
                std::time::Instant::now() < deadline,
                "attachment readiness deadline: expected {count}, actual {}, blocked={}, connected={}, configured={}, pending={}, retry={}, statuses={:?}",
                view.attachments.items.len(),
                view.attachments_blocked(),
                view.connected(),
                view.configured(),
                view.pending,
                view.retry.is_some(),
                view.attachments.items.iter().map(|item| (item.key, std::mem::discriminant(&item.status), item.pending)).collect::<Vec<_>>()
            );
        });
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    view.read_with(cx, |view, _| {
        view.attachments
            .items
            .iter()
            .map(|item| {
                let Status::Ready(attachment) = &item.status else {
                    panic!("ready attachment expected");
                };
                attachment.clone()
            })
            .collect()
    })
}

fn download(cx: &mut VisualTestContext, attachment: &Attachment, destination: &Path) {
    tap(cx, &format!("attachment-download-{}", attachment.id));
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|_| Some(destination.to_path_buf()));
    finish_download(cx, destination);
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
}

pub(super) fn finish_download(cx: &mut VisualTestContext, destination: &Path) {
    // Publishing the file precedes delivery of the finished state to the view.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        if cx.debug_bounds("file-download-finished").is_some() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "download completion deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(destination.exists());
    assert_eq!(
        cx.update(crate::feedback::tests::summary),
        tr("files_download_done")
    );
    assert!(cx.debug_bounds("files_download_done").is_none());
    // The download can finish while Kit is still animating the dialog bounds.
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    tap(cx, "file-download-close");
}

#[gpui::test]
fn selects_sends_and_downloads(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
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
        let text = fixture.directory.path().join("完整内容与 Emoji.txt");
        let empty = fixture.directory.path().join("empty.json");
        let document = fixture.directory.path().join("报告.pdf");
        let pdf = include_bytes!("../../../../../../tests/fixtures/document.pdf");
        std::fs::write(&document, pdf).unwrap();
        let body = "完整附件内容 🙂\nA full UTF-8 file\n";
        std::fs::write(&text, body).unwrap();
        std::fs::write(&empty, []).unwrap();
        let binding = Binding {
            project: None,
            worktree: None,
            ..fixture.binding.clone()
        };
        let (view, visual) = fixture::open_session(cx, binding, None);
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        tap(visual, "live-attach");
        visual.simulate_path_prompt_response(|_| None);
        assert!(view.read_with(visual, |view, _| view.attachments.items.is_empty()));
        choose(visual, &[text.clone(), empty, document]);
        wait(visual, |cx| view.read(cx).attachments.items.len() == 3);
        assert!(view.read_with(visual, |view, _| {
            view.attachments.items.len() == 3
                && view
                    .attachments
                    .items
                    .iter()
                    .all(|item| matches!(item.status, Status::Staged))
        }));
        let published = fixture.node.profile().join("attachments");
        assert!(!published.exists() || std::fs::read_dir(&published).unwrap().next().is_none());
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.retarget(fixture.binding.clone(), window, cx)
            })
        });
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        assert!(
            view.read_with(visual, |view, _| view.attachments.items.len() == 3
                && view.session.is_none())
        );
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == sailry_protocol::conversation::Status::Completed)
                })
        });
        let attachments = view.read_with(visual, |view, _| {
            view.history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .entries
                .iter()
                .filter(|entry| entry.author == "user")
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::Attachment(attachment) => Some(attachment.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(attachments.len(), 3);
        assert_eq!(Some(attachments[0].spec.worktree), fixture.binding.worktree);
        assert_eq!(attachments[0].spec.media_type, "text/plain");
        view.read_with(visual, |view, cx| {
            assert!(view.attachments.items.is_empty());
            assert!(view.input.read(cx).value().is_empty());
            assert_eq!(view.error, None);
            let parts = &view
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .entries
                .iter()
                .find(|entry| entry.author == "user")
                .unwrap()
                .parts;
            assert_eq!(
                parts,
                &attachments
                    .iter()
                    .cloned()
                    .map(Part::Attachment)
                    .collect::<Vec<_>>()
            );
        });
        let request = fixture.server.requests.lock().unwrap()[0].to_string();
        assert!(request.contains("完整附件内容 🙂"));
        assert!(request.contains("data:application/pdf;base64,"));
        assert!(!request.contains("sailry-attachment://"));
        let destination = fixture.directory.path().join("download.txt");
        download(visual, &attachments[0], &destination);
        assert_eq!(std::fs::read_to_string(&destination).unwrap(), body);
        let empty = fixture.directory.path().join("download-empty.json");
        download(visual, &attachments[1], &empty);
        let document = fixture.directory.path().join("download.pdf");
        download(visual, &attachments[2], &document);
        assert_eq!(std::fs::read(document).unwrap(), pdf);
        assert_eq!(std::fs::metadata(&empty).unwrap().len(), 0);
        let resume = view.read_with(visual, |view, _| view.session.clone().unwrap());
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), resume);
        wait(visual, |cx| {
            view.read(cx).connected() && !view.read(cx).rows.is_empty()
        });
        let destination = fixture.directory.path().join("download-resumed.txt");
        download(visual, &attachments[0], &destination);
        assert_eq!(std::fs::read_to_string(destination).unwrap(), body);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn sends_non_native_files(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let path = fixture.directory.path().join("archive.zip");
        let bytes = b"opaque\0\xffattachment";
        std::fs::write(&path, bytes).unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        choose(visual, &vec![path.clone(); 9]);
        wait(visual, |cx| view.read(cx).attachments.error.is_some());
        assert!(view.read_with(visual, |view, _| view.attachments.items.is_empty()));
        choose(visual, &[path]);
        let attachment = ready(visual, &view, 1).remove(0);
        tap(visual, "live-chat-input");
        visual.simulate_input("Inspect this file");
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == sailry_protocol::conversation::Status::Completed)
                })
        });
        view.read_with(visual, |view, cx| {
            assert!(view.input.read(cx).value().is_empty());
            assert!(view.attachments.items.is_empty());
            assert_eq!(view.error, None);
            assert!(view.retry.is_none());
        });
        let request = fixture.server.requests.lock().unwrap()[0].to_string();
        assert!(request.contains("Attachment file:"));
        assert!(request.contains(&attachment.id.to_string()));
        assert!(!request.contains("opaque"));
        let destination = fixture.directory.path().join("download.zip");
        download(visual, &attachment, &destination);
        assert_eq!(std::fs::read(destination).unwrap(), bytes);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn uploads_a_full_selection(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let paths: Vec<_> = (0..8)
            .map(|index| {
                let path = fixture.directory.path().join(format!("batch-{index}.txt"));
                std::fs::write(&path, vec![b'a'; 512 * 1024]).unwrap();
                path
            })
            .collect();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        choose(visual, &paths);
        wait(visual, |cx| {
            view.read(cx)
                .attachments
                .items
                .iter()
                .all(|item| !item.pending)
        });
        assert!(
            view.read_with(visual, |view, _| view.attachments.ready()),
            "all selected attachments must upload"
        );
        assert_eq!(ready(visual, &view, 8).len(), 8);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
