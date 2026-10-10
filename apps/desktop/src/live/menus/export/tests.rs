use super::{Format, choose};
use crate::{
    activity::fixture::Fixture,
    live::menus::{Command as Action, Dispatch, Target},
    shell::Shell,
};
use core::prelude::v1::test;
use gpui_kit::{
    component::{Root, WindowExt},
    *,
};
use sailry_client::Client;
use sailry_link::{Admission, CancellationToken, Pending, Stream, Subscription, Transport};
use sailry_protocol::{
    Command, Fault, NodeId, Output, Request, Session, StreamId, Topic,
    attachment::{Attachment, Spec},
    conversation::{Input, Status},
};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const BODY: &[u8] = b"Published attachment bytes\n";

fn client(fixture: &Fixture, index: usize) -> Arc<Client> {
    Arc::new(Client::new(transport(fixture, index)))
}

fn transport(fixture: &Fixture, index: usize) -> Arc<dyn Transport> {
    if index == 0 {
        fixture.nodes[0].local()
    } else {
        fixture.nodes[0]
            .link()
            .remote(fixture.nodes[1].link().address())
    }
}

fn seed(fixture: &Fixture, index: usize) -> Attachment {
    let client = client(fixture, index);
    let session = &fixture.sessions[index];
    let attachment = fixture.runtime.block_on(async {
        let spec = Spec {
            worktree: session.worktree,
            name: "report.txt".into(),
            media_type: "text/plain".into(),
            size: BODY.len() as u64,
            revision: blake3::hash(BODY).to_hex().to_string(),
        };
        let Output::AttachmentUpload(upload) = client
            .execute(client.prepare(Command::UploadAttachment(spec)))
            .await
            .unwrap()
        else {
            panic!("attachment upload expected");
        };
        let mut bytes = BODY;
        client
            .upload(&upload, &mut bytes, CancellationToken::new(), |_| {})
            .await
            .unwrap();
        let Output::Attachment(attachment) = client
            .execute(client.prepare(Command::FinishAttachmentUpload {
                worktree: session.worktree,
                stream: upload.stream,
            }))
            .await
            .unwrap()
        else {
            panic!("published attachment expected");
        };
        client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: Input {
                    text: format!("Captured export from Node {index}"),
                    attachments: vec![attachment.id],
                    references: vec![],
                },
            }))
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let history = client
                .read_conversation(session.id, None, 20)
                .await
                .unwrap();
            if history.page.questions.iter().any(|question| {
                question.state == sailry_protocol::conversation::question::State::Pending
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "export fixture question deadline"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        attachment
    });
    fixture.answer(index);
    fixture.runtime.block_on(async {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let history = client
                .read_conversation(session.id, None, 20)
                .await
                .unwrap();
            if history
                .page
                .runs
                .iter()
                .any(|run| run.status == Status::Completed)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "export fixture completion deadline: {:?}",
                history.page.runs
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    attachment
}

fn mount<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Shell>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("en");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
        cx.set_global(crate::preferences::Preferences::default());
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1200.), px(800.)));
    (owner.unwrap(), visual)
}

#[track_caller]
fn wait(visual: &mut VisualTestContext, ready: impl Fn(&mut Window, &mut App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        if visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            ready(window, cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "conversation export UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn select(shell: &Entity<Shell>, visual: &mut VisualTestContext, node: NodeId, session: &Session) {
    wait(visual, |_, cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .unwrap()
            .hosts
            .contains_key(&node)
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.select_live_host(node, window, cx))
    });
    wait(visual, |_, cx| {
        let live = shell.read(cx).live.as_ref().unwrap();
        live.selected == node
            && live.view.connected
            && live.view.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.node == node
                    && snapshot.sessions.iter().any(|value| value.id == session.id)
            })
    });
}

fn entries(path: &Path) -> Vec<std::path::PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    entries
}

#[gpui_kit::test]
fn captures_nodes_and_publishes_attachment_links(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let attachments = [seed(&fixture, 0), seed(&fixture, 1)];
    let output = tempfile::tempdir().unwrap();
    let (shell, visual) = mount(&fixture, cx);
    for (index, attachment) in attachments.iter().enumerate() {
        for (format, action) in [
            (Format::Markdown, Action::ExportMarkdown),
            (Format::Html, Action::ExportHtml),
        ] {
            let destination = output
                .path()
                .join(format!("node-{index}.{}", format.extension()));
            select(
                &shell,
                visual,
                fixture.nodes[index].id(),
                &fixture.sessions[index],
            );
            let before = visual.update(|window, cx| {
                crate::feedback::tests::count(window, &crate::tr("conversation_exported"), cx)
            });
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.live_resource_action(
                        &Dispatch {
                            node: fixture.nodes[index].id(),
                            target: Target::Session(fixture.sessions[index].id),
                            command: action,
                        },
                        window,
                        cx,
                    )
                })
            });
            assert!(visual.did_prompt_for_new_path());
            select(
                &shell,
                visual,
                fixture.nodes[1 - index].id(),
                &fixture.sessions[1 - index],
            );
            visual.simulate_new_path_selection(|suggested| {
                assert!(suggested.is_absolute());
                Some(destination.clone())
            });
            wait(visual, |window, cx| {
                crate::feedback::tests::count(window, &crate::tr("conversation_exported"), cx)
                    == before + 1
                    && !window.has_active_dialog(cx)
            });
            let document = std::fs::read_to_string(&destination).unwrap();
            assert!(document.contains(&format!("Captured export from Node {index}")));
            assert!(!document.contains(&format!("Captured export from Node {}", 1 - index)));
            assert!(document.contains("report.txt"));
            match format {
                Format::Image => unreachable!("Portable export fixture"),
                Format::Markdown => assert!(
                    document.starts_with("## ")
                        && document.contains("[report.txt](<sailry-assets-")
                ),
                Format::Html => assert!(
                    document.starts_with("<!doctype html>")
                        && document.contains("href=\"sailry-assets-")
                ),
            }
            let asset = sailry_client::conversation::export::attachment_name(attachment);
            let published = entries(output.path())
                .into_iter()
                .find(|path| {
                    path.is_dir() && document.contains(path.file_name().unwrap().to_str().unwrap())
                })
                .unwrap();
            assert_eq!(std::fs::read(published.join(&asset)).unwrap(), BODY);
            assert!(document.contains(&asset));
            assert_eq!(
                shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().selected),
                fixture.nodes[1 - index].id()
            );
        }
    }
    assert_eq!(entries(output.path()).len(), 8);
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}

// Delay delivery of real canonical history so cancellation and conflict boundaries
// are exercised after destination preparation, without modifying Node responses.
struct Held {
    inner: Arc<dyn Transport>,
    entered: CancellationToken,
    release: CancellationToken,
    completed: Arc<AtomicUsize>,
    requests: Mutex<Vec<Request>>,
}

impl Held {
    fn new(inner: Arc<dyn Transport>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            entered: CancellationToken::new(),
            release: CancellationToken::new(),
            completed: Arc::default(),
            requests: Mutex::default(),
        })
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.release.cancel();
    }
}

impl Transport for Held {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn open(&self, stream: StreamId) -> Pending<'_, Result<Stream, Fault>> {
        self.inner.open(stream)
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let hold = matches!(&request.command, Command::ReadConversation { .. });
            self.requests.lock().unwrap().push(request.clone());
            let mut admission = self.inner.dispatch(request).await?;
            if hold {
                let entered = self.entered.clone();
                let release = self.release.clone();
                let completed = self.completed.clone();
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let response = std::mem::replace(&mut admission.completion, receiver);
                tokio::spawn(async move {
                    let response = response.await;
                    entered.cancel();
                    release.cancelled().await;
                    if let Ok(response) = response {
                        let _ = sender.send(response);
                    }
                    completed.fetch_add(1, Ordering::SeqCst);
                });
            }
            Ok(admission)
        })
    }
}

#[gpui_kit::test]
fn cancels_without_publishing_files(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(&fixture, cx);
    for index in 0..2 {
        let output = tempfile::tempdir().unwrap();
        let destination = output.path().join("cancelled.md");
        let held = Held::new(transport(&fixture, index));
        let client = Arc::new(Client::new(held.clone()));
        let choose_export = |visual: &mut VisualTestContext| {
            visual.update(|window, cx| {
                choose(
                    client.clone(),
                    fixture.runtime.clone(),
                    fixture.sessions[index].clone(),
                    Format::Markdown,
                    window,
                    cx,
                )
            })
        };
        choose_export(visual);
        assert!(visual.did_prompt_for_new_path());
        visual.simulate_new_path_selection(|_| None);
        visual.run_until_parked();
        assert!(entries(output.path()).is_empty());
        assert!(held.requests.lock().unwrap().is_empty());
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));

        choose_export(visual);
        visual.simulate_new_path_selection(|_| Some(destination.clone()));
        wait(visual, |window, cx| {
            held.entered.is_cancelled() && window.has_active_dialog(cx)
        });
        assert!(
            !entries(output.path()).is_empty(),
            "prepared export staging expected"
        );
        crate::live::file_tests::click(visual, "conversation-export-cancel");
        wait(visual, |window, cx| !window.has_active_dialog(cx));
        held.release.cancel();
        wait(visual, |_, _| {
            held.completed.load(Ordering::SeqCst) == 1 && entries(output.path()).is_empty()
        });
        assert!(!destination.exists());
        assert_eq!(
            visual.update(|window, cx| crate::feedback::tests::count(
                window,
                &crate::tr("conversation_exported"),
                cx
            )),
            0
        );
        assert_eq!(held.requests.lock().unwrap().len(), 1);
    }
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}

#[gpui_kit::test]
fn preserves_changed_destinations_and_cleans_assets(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(&fixture, cx);
    for index in 0..2 {
        seed(&fixture, index);
        let output = tempfile::tempdir().unwrap();
        let destination = output.path().join("retained.html");
        std::fs::write(&destination, "Original destination").unwrap();
        let held = Held::new(transport(&fixture, index));
        let before = visual.update(|window, cx| {
            crate::feedback::tests::count(window, &crate::tr("files_download_conflict"), cx)
        });
        visual.update(|window, cx| {
            choose(
                Arc::new(Client::new(held.clone())),
                fixture.runtime.clone(),
                fixture.sessions[index].clone(),
                Format::Html,
                window,
                cx,
            )
        });
        visual.simulate_new_path_selection(|_| Some(destination.clone()));
        wait(visual, |_, _| held.entered.is_cancelled());
        std::fs::write(&destination, "External destination change").unwrap();
        held.release.cancel();
        wait(visual, |window, cx| {
            crate::feedback::tests::count(window, &crate::tr("files_download_conflict"), cx)
                == before + 1
                && !window.has_active_dialog(cx)
        });
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "External destination change"
        );
        assert_eq!(entries(output.path()), vec![destination]);
        assert!(held.requests.lock().unwrap().iter().any(|request| matches!(
            &request.command,
            Command::DownloadAttachment { .. }
        ) && request.target
            == fixture.nodes[index].id()));
    }
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}

#[gpui_kit::test]
fn footer_exports_only_captured_reply(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    for index in 0..2 {
        seed(&fixture, index);
    }
    let output = tempfile::tempdir().unwrap();
    let (shell, visual) = mount(&fixture, cx);
    for index in 0..2 {
        let session = fixture.sessions[index].clone();
        let history = fixture
            .runtime
            .block_on(client(&fixture, index).read_conversation(session.id, None, 50))
            .unwrap();
        let turn = history.page.runs.last().unwrap().turn;
        let expected = fixture
            .runtime
            .block_on(client(&fixture, index).export_reply(
                session.id,
                turn,
                "Reply",
                sailry_client::conversation::export::Format::Markdown,
                "assets",
                &CancellationToken::new(),
            ))
            .unwrap()
            .content;
        assert!(!expected.is_empty());
        assert!(!expected.contains("Captured export from Node"));
        assert!(!expected.contains("ask_user"));
        select(&shell, visual, fixture.nodes[index].id(), &session);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(session.clone(), window, cx)
            })
        });
        let selector: &'static str = Box::leak(format!("reply-export-{turn}").into_boxed_str());
        wait(visual, |window, _| {
            use gpui_kit::test::TestWindowExt as _;
            window.try_find(selector).is_some()
        });
        crate::live::file_tests::click(visual, selector);
        visual.simulate_mouse_move(point(px(-100.), px(-100.)), None, Modifiers::default());
        visual.simulate_keystrokes("down enter");
        wait(visual, |_, _| true);
        assert!(visual.did_prompt_for_new_path());
        select(
            &shell,
            visual,
            fixture.nodes[1 - index].id(),
            &fixture.sessions[1 - index],
        );
        let path = output.path().join(format!("reply-{index}.md"));
        visual.simulate_new_path_selection(|_| Some(path.clone()));
        wait(visual, |window, cx| {
            path.exists() && !window.has_active_dialog(cx)
        });
        let actual = std::fs::read_to_string(path).unwrap();
        assert!(actual.starts_with(&expected));
        assert!(!actual.contains("Captured export from Node"));
        assert!(!actual.contains("ask_user"));
        assert!(actual.contains("[Sailry](https://sailry.dev)"));
    }
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}
