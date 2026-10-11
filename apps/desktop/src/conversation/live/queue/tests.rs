use super::super::tests::{click as click_control, provider, support, wait};
use super::*;
use core::prelude::v1::test;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;

mod guidance;
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
            .block_on(Node::start_isolated(directory.path().join("node")))
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
        for name in ["context7", "github"] {
            execute(Command::SetPluginEnabled {
                name: name.into(),
                expected_revision: 1,
                enabled: false,
            });
        }
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
            .block_on(self.client.upload(
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
fn restores_attachments_and_references(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let server = fixture.runtime.block_on(support::Server::tools(vec![(
            "ask_user".into(),
            serde_json::json!({"prompt":"Describe the task","input":{"kind":"text","multiline":false,"max_bytes":256}}),
        )]));
        guidance::running(&mut fixture, server, true);
        let attachment = fixture.attachment();
        let references = vec![sailry_protocol::conversation::reference::Reference {
            target: sailry_protocol::conversation::reference::Target::Worktree,
            label: "Current worktree".into(),
        }];
        let message = sailry_protocol::conversation::Input {
            references: references.clone(),
            text: "Original @Current worktree".into(),
            attachments: vec![attachment.id],
        };
        let turn = fixture.enqueue(message.clone());
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
        click(visual, selector("edit", turn));
        wait(visual, |cx| {
            panel.read(cx).queue.items.is_empty() && !panel.read(cx).pending
        });
        view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), message.text);
            assert_eq!(view.input.read(cx).content().tokens().len(), 1);
            assert_eq!(view.active_references(cx), references);
            assert_eq!(view.attachments.ids(), [attachment.id]);
        });
        assert!(visual.debug_bounds("queue-input").is_none());
        let snapshot = visual.update(|_, cx| {
            let mut snapshot = view.read(cx).snapshot_draft(cx).unwrap().unwrap();
            snapshot.draft =
                serde_json::from_slice(&serde_json::to_vec(&snapshot.draft).unwrap()).unwrap();
            assert_eq!(
                snapshot.draft.attachments,
                [super::super::recovery::Attachment {
                    index: 0,
                    value: attachment.clone(),
                }]
            );
            assert!(snapshot.sources.is_empty());
            snapshot
        });
        super::super::tests::remove_attachment(visual, 0);
        let restored_key = visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                let key = view.release_draft_attachments();
                view.references.selected.clear();
                view.input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                assert!(view.recover_draft(snapshot, window, cx));
                assert_eq!(view.attachments.ids(), [attachment.id]);
                assert_eq!(view.active_references(cx), references);
                key
            })
        });
        // Removing a restored attachment only edits the draft; historical bytes remain.
        super::super::tests::remove_attachment(visual, restored_key);
        assert!(view.read_with(visual, |view, _| !view.has_attachments()));
        assert!(matches!(
            fixture.execute(Command::DownloadAttachment {
                worktree: fixture.session.worktree,
                attachment: attachment.id
            }),
            Output::AttachmentDownload(_)
        ));
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            panel.read(cx).queue.items.len() == 1 && !view.read(cx).pending
        });
        let requeued = panel.read_with(visual, |panel, _| panel.queue.items[0].turn);
        assert_ne!(requeued, turn);
        let Output::QueuedMessage(requeued) =
            fixture.execute(Command::ReadQueuedTurn { turn: requeued })
        else {
            panic!("queued input expected")
        };
        assert_eq!(requeued.message.text, message.text);
        assert_eq!(requeued.message.references, references);
        assert!(requeued.message.attachments.is_empty());
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
fn preserves_existing_draft(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let turn = fixture.enqueue("Queued input");
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
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Unsent draft");
        click(visual, selector("edit", turn));
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Unsent draft"
        );
        panel.read_with(visual, |panel, _| {
            assert!(!panel.pending);
            assert!(panel.retry.is_none());
            assert_eq!(panel.error, Some("queue_draft_present"));
            assert_eq!(panel.queue.items[0].turn, turn);
        });
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn requeues_captured_input(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        let server = fixture.runtime.block_on(support::Server::tools(vec![(
            "ask_user".into(),
            serde_json::json!({"prompt":"Describe the task","input":{"kind":"text","multiline":false,"max_bytes":256}}),
        )]));
        guidance::running(&mut fixture, server, true);
        let attachment = fixture.attachment();
        let message = sailry_protocol::conversation::Input {
            text: "Queued input".into(),
            references: Vec::new(),
            attachments: vec![attachment.id],
        };
        let turn = fixture.enqueue(message.clone());
        let (view, visual) = super::super::tests::fixture::open(
            cx,
            fixture.binding.clone(),
            fixture.session.clone(),
        );
        let panel = view.read_with(visual, |view, _| view.queue.clone());
        wait(visual, |cx| {
            panel.read(cx).connected
                && panel.read(cx).queue.items.len() == 1
                && view.read(cx).configured()
                && view.read(cx).contributions.read(cx).ready(cx)
        });
        click(visual, selector("edit", turn));
        wait(visual, |cx| {
            !panel.read(cx).pending && panel.read(cx).queue.items.is_empty()
        });
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            !view.read(cx).pending && panel.read(cx).queue.items.len() == 1
        });
        let requeued = panel.read_with(visual, |panel, _| panel.queue.items[0].turn);
        let Output::QueuedMessage(captured) =
            fixture.execute(Command::ReadQueuedTurn { turn: requeued })
        else {
            panic!("queued input expected")
        };
        assert_eq!(captured.message, message);
        click(visual, selector("edit", requeued));
        wait(visual, |cx| {
            !panel.read(cx).pending && panel.read(cx).queue.items.is_empty()
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            message.text
        );
        assert_eq!(
            view.read_with(visual, |view, _| view.attachments.ids()),
            [attachment.id]
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
