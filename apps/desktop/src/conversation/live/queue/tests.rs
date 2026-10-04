use super::super::tests::{click as click_control, provider, support, wait};
use super::*;
use core::prelude::v1::test;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::conversation::Part;

mod lifecycle;

struct Fixture {
    _directory: tempfile::TempDir,
    runtime: Arc<tokio::runtime::Runtime>,
    node: Node,
    controller: Link,
    client: Arc<Client>,
    transport: Arc<dyn Transport>,
    server: support::Server,
    binding: Binding,
    session: Session,
}

impl Fixture {
    fn new(remote: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = runtime
            .block_on(Node::start(directory.path().join("node")))
            .unwrap();
        let controller = runtime
            .block_on(Link::controller(
                directory.path().join("controller"),
                NetworkScope::default(),
            ))
            .unwrap();
        let address = runtime
            .block_on(
                controller
                    .handle()
                    .pair(node.link().invite().unwrap().ticket()),
            )
            .unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Arc::new(Client::new(transport.clone()));
        let execute = |command| {
            runtime
                .block_on(client.execute(client.prepare(command)))
                .unwrap()
        };
        let server = runtime.block_on(support::Server::start(false));
        let provider = provider(&server.endpoint, "queue-model");
        let config = SessionConfig {
            assistant: None,
            resource: None,
            provider: provider.id,
            model: provider.default_model.clone(),
            credential: None,
            effort: Effort::High,
            mode: sailry_protocol::WorkMode::Code,
            permission: sailry_protocol::Permission::Ask,
        };
        execute(Command::SaveProvider {
            provider,
            expected_revision: 0,
            secret: None,
        });
        let Output::Project(project) = execute(Command::RegisterProject {
            name: "Queue fixture".into(),
            path: directory.path().to_str().unwrap().into(),
        }) else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let worktree = snapshot.worktrees[0].id;
        let Output::Session(session) = execute(Command::CreateSession {
            project: Some(project.id),
            worktree: Some(worktree),
            config: Some(config),
        }) else {
            panic!("session expected")
        };
        execute(Command::SetQueuePaused {
            session: session.id,
            expected_revision: 0,
            paused: true,
        });
        let binding = Binding {
            client: client.clone(),
            defaults: client.clone(),
            runtime: runtime.clone(),
            project: Some(project.id),
            worktree: Some(worktree),
            host: "Queue Node".into(),
            project_name: "Queue fixture".into(),
            branch: "main".into(),
        };
        Self {
            _directory: directory,
            runtime,
            node,
            controller,
            client,
            transport,
            server,
            binding,
            session,
        }
    }

    fn execute(&self, command: Command) -> Output {
        self.runtime
            .block_on(self.client.execute(self.client.prepare(command)))
            .unwrap()
    }

    fn enqueue(&self, message: impl Into<sailry_protocol::conversation::Input>) -> TurnId {
        let Output::QueuedTurn(turn) = self.execute(Command::QueueTurn {
            session: self.session.id,
            expected_revision: self.session.revision,
            message: message.into(),
        }) else {
            panic!("turn expected")
        };
        self.execute(Command::StartQueuedTurn { turn: turn.id });
        turn.id
    }

    fn close(self) {
        self.runtime.block_on(self.node.shutdown()).unwrap();
        self.runtime.block_on(self.controller.close()).unwrap();
    }

    fn attachment(&self) -> sailry_protocol::attachment::Attachment {
        let bytes = b"Complete attachment input";
        let Output::AttachmentUpload(upload) = self.execute(Command::UploadAttachment(
            sailry_protocol::attachment::Spec {
                worktree: self.session.worktree,
                name: "input.txt".into(),
                media_type: "text/plain".into(),
                size: bytes.len() as u64,
                revision: blake3::hash(bytes).to_hex().to_string(),
            },
        )) else {
            panic!("upload expected")
        };
        self.runtime
            .block_on(self.client.upload_attachment(
                &upload,
                &mut &bytes[..],
                CancellationToken::new(),
                |_| {},
            ))
            .unwrap();
        let Output::Attachment(attachment) = self.execute(Command::FinishAttachmentUpload {
            worktree: self.session.worktree,
            stream: upload.stream,
        }) else {
            panic!("attachment expected")
        };
        attachment
    }
}

fn selector(action: &str, turn: TurnId) -> &'static str {
    Box::leak(format!("queue-{action}-{turn}").into_boxed_str())
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    // Let hover and asynchronous composer layout settle before reading the click target.
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    click_control(cx, selector);
}

#[gpui::test]
fn saves_without_text(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let attachment = fixture.attachment();
        let references = vec![sailry_protocol::conversation::reference::Reference {
            target: sailry_protocol::conversation::reference::Target::Worktree,
            label: "Current worktree".into(),
        }];
        let turn = fixture.enqueue(sailry_protocol::conversation::Input {
            references: references.clone(),
            text: "remove this text @Current worktree".into(),
            attachments: vec![attachment.id],
        });
        let (view, visual) = super::super::tests::fixture::open(
            cx,
            fixture.binding.clone(),
            fixture.session.clone(),
        );
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).queue.items.len() == 1
                && panel.read(cx).connected
                && view.read(cx).configured()
                && view.read(cx).contributions.read(cx).ready(cx)
        });
        click(visual, "live-queue");
        click(visual, selector("edit", turn));
        wait(visual, |cx| panel.read(cx).editing.is_some());
        let original = panel.read_with(visual, |panel, cx| {
            let editing = panel.editing.as_ref().unwrap();
            assert_eq!(panel.session, Some(fixture.session.id));
            assert_eq!(editing.turn, turn);
            assert_eq!(editing.revision, panel.queue.items[0].revision);
            assert_eq!(editing.references, references);
            let content = editing.input.read(cx).content();
            assert_eq!(content.tokens().len(), 1);
            content
        });
        visual.simulate_keystrokes("secondary-a backspace secondary-z");
        assert_eq!(
            panel.read_with(visual, |panel, cx| panel
                .editing
                .as_ref()
                .unwrap()
                .input
                .read(cx)
                .content()),
            original
        );
        visual.simulate_keystrokes("secondary-a backspace");
        click(visual, "queue-edit-save");
        wait(visual, |cx| {
            panel.read(cx).editing.is_none() && panel.read(cx).queue.items[0].revision == 2
        });
        let Output::QueuedMessage(message) = fixture.execute(Command::ReadQueuedTurn { turn })
        else {
            panic!("queued message expected");
        };
        assert!(message.message.text.is_empty());
        assert_eq!(message.message.attachments, [attachment.id]);
        assert!(message.message.references.is_empty());
        assert_eq!(
            panel.read_with(visual, |panel, _| render::label(&panel.queue.items[0])),
            attachment.spec.name
        );
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

fn init(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
}

#[gpui::test]
fn edits_reorders_and_sends(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let original = "完整内容 🙂\n".repeat(100);
        let attachment = fixture.attachment();
        let first = fixture.enqueue(sailry_protocol::conversation::Input {
            references: Vec::new(),
            text: original.clone(),
            attachments: vec![attachment.id],
        });
        let second = fixture.enqueue("second");
        let third = fixture.enqueue("third");
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                View::new(
                    fixture.binding.clone(),
                    Some(fixture.session.clone()),
                    window,
                    cx,
                )
            });
            entity = Some(view.clone());
            Root::new(
                cx.new(|_| crate::conversation::live::tests::fixture::Harness(view)),
                window,
                cx,
            )
        });
        let view = entity.unwrap();
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).connected
                && panel.read(cx).queue.items.len() == 3
                && view.read(cx).configured()
                && view.read(cx).contributions.read(cx).ready(cx)
        });
        assert!(view.read_with(visual, |view, _| view.rows.is_empty()));
        let activity = visual.debug_bounds("composer-activity").unwrap();
        let queue = visual.debug_bounds("live-queue").unwrap();
        assert_eq!(activity.origin.y, queue.origin.y);
        assert!(queue.size.width < activity.size.width / 2.);

        click(visual, "live-queue");
        assert!(visual.debug_bounds("queue-pause").is_none());
        let drag = visual.debug_bounds(selector("drag", first)).unwrap();
        let edit = visual.debug_bounds(selector("edit", first)).unwrap();
        assert!((drag.center().y - edit.center().y).abs() < px(1.));
        click(visual, selector("edit", first));
        wait(visual, |cx| panel.read(cx).editing.is_some());
        assert!(visual.debug_bounds("queue-input").is_some());
        assert!(visual.debug_bounds("queue-pause").is_none());
        panel.read_with(visual, |panel, cx| {
            assert_eq!(
                panel
                    .editing
                    .as_ref()
                    .unwrap()
                    .input
                    .read(cx)
                    .value()
                    .as_ref(),
                original
            )
        });
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("edited 中文");
        visual.simulate_keystrokes("shift-enter");
        visual.simulate_input("second line");
        visual.simulate_keystrokes("escape");
        click(visual, "live-queue");
        panel.read_with(visual, |panel, cx| {
            assert_eq!(
                panel.editing.as_ref().unwrap().input.read(cx).value(),
                "edited 中文\nsecond line"
            )
        });
        click(visual, "queue-edit-save");
        wait(visual, |cx| {
            panel.read(cx).editing.is_none() && panel.read(cx).queue.items[0].revision == 2
        });
        let from = visual
            .debug_bounds(selector("drag", first))
            .unwrap()
            .center();
        let to = visual
            .debug_bounds(selector("drag", third))
            .unwrap()
            .center();
        visual.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        for position in [from + point(px(8.), px(8.)), to] {
            visual.simulate_mouse_move(position, MouseButton::Left, Modifiers::default());
            visual.run_until_parked();
            visual.update(|window, cx| {
                let _ = window.draw(cx);
            });
        }
        visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        wait(visual, |cx| {
            panel
                .read(cx)
                .queue
                .items
                .iter()
                .map(|item| item.turn)
                .collect::<Vec<_>>()
                == [second, third, first]
        });
        click(visual, selector("delete", second));
        wait(visual, |cx| {
            panel.read(cx).queue.items.len() == 2 && !panel.read(cx).pending
        });
        assert!(visual.debug_bounds("queue-pause").is_none());
        click(visual, selector("send", third));
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
                        .filter(|run| run.status == Status::Completed)
                        .count()
                        == 2
                })
        });
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert!(!page.queue.paused);
            let messages: Vec<_> = page
                .entries
                .iter()
                .filter(|entry| entry.author == "user")
                .map(|entry| entry.parts.clone())
                .collect();
            assert_eq!(
                messages,
                [
                    vec![Part::Text("third".into())],
                    vec![
                        Part::Text("edited 中文\nsecond line".into()),
                        Part::Attachment(attachment.clone())
                    ]
                ]
            );
            assert!(!view.rows.contains(&second));
        });
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn retains_conflicting_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let turn = fixture.enqueue("original");
        let other = Client::new(fixture.node.local());
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                View::new(
                    fixture.binding.clone(),
                    Some(fixture.session.clone()),
                    window,
                    cx,
                )
            });
            entity = Some(view.clone());
            Root::new(
                cx.new(|_| crate::conversation::live::tests::fixture::Harness(view)),
                window,
                cx,
            )
        });
        let view = entity.unwrap();
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).connected
                && panel.read(cx).queue.items.len() == 1
                && view.read(cx).configured()
                && view.read(cx).contributions.read(cx).ready(cx)
        });
        click(visual, "live-queue");
        click(visual, selector("edit", turn));
        wait(visual, |cx| panel.read(cx).editing.is_some());
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("local unsaved draft");
        fixture
            .runtime
            .block_on(other.execute(other.prepare(Command::EditQueuedTurn {
                turn,
                expected_revision: 1,
                message: "another controller".into(),
            })))
            .unwrap();
        wait(visual, |cx| panel.read(cx).queue.items[0].revision == 2);
        click(visual, "queue-edit-save");
        panel.read_with(visual, |panel, cx| {
            let editing = panel.editing.as_ref().unwrap();
            assert_eq!(editing.revision, 1);
            assert_eq!(editing.input.read(cx).value(), "local unsaved draft");
            assert!(!panel.pending);
        });
        click(visual, "queue-edit-reload");
        wait(visual, |cx| {
            panel
                .read(cx)
                .editing
                .as_ref()
                .is_some_and(|editing| editing.revision == 2)
        });
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("draft while removed");
        fixture.execute(Command::RemoveQueuedTurn {
            turn,
            expected_revision: 2,
        });
        wait(visual, |cx| panel.read(cx).queue.items.is_empty());
        visual.simulate_keystrokes("escape");
        click(visual, "live-queue");
        panel.read_with(visual, |panel, cx| {
            assert_eq!(
                panel.editing.as_ref().unwrap().input.read(cx).value(),
                "draft while removed"
            )
        });
        click(visual, "queue-edit-cancel");
        assert!(panel.read_with(visual, |panel, _| panel.editing.is_none()));
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
