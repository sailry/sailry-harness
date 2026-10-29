use crate::{
    agent::{
        canonical::{BlockKind, Record, Role},
        engine::history as conversion,
    },
    store::agent::tests::fixture::message,
};
use sailry_client::Client;
use sailry_link::{CancellationToken, Identity, Link, Local, NetworkScope};
use sailry_protocol::{
    Command, Output, Session, SessionId, TurnId,
    conversation::{History, Image, Part, Status},
    tool::{OutputPage, OutputReference},
};
use std::{path::Path, sync::Arc};

struct Fixture {
    profile: crate::profile::Profile,
    store: crate::store::Store,
    network: Link,
    controller: Link,
    local: Client,
    remote: Client,
}

impl Fixture {
    async fn open(root: &Path) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let profile = crate::profile::Profile::acquire(&root.join("node")).unwrap();
        let identity = Arc::new(Identity::open(&profile.path).unwrap());
        // No supervisor runs: these synthetic events are the only history writer.
        let store = crate::store::Store::open(
            profile.path.clone(),
            identity.id(),
            crate::providers::catalog::Catalog::empty(),
            Default::default(),
            Default::default(),
            Default::default(),
            false,
        )
        .await
        .unwrap();
        let ingress = store.ingress.clone();
        let network = Link::bind(
            &identity,
            NetworkScope::default(),
            ingress.clone(),
            ingress.clone(),
        )
        .await
        .unwrap();
        store.bind_link(network.handle()).await.unwrap();
        let local = Client::new(Arc::new(Local::new(identity.id(), identity.id(), ingress)));
        let controller = Link::controller(root.join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(network.handle().invite().unwrap().ticket())
            .await
            .unwrap();
        let remote = Client::new(controller.handle().remote(address));
        Self {
            profile,
            store,
            network,
            controller,
            local,
            remote,
        }
    }

    async fn session(&self, root: &Path) -> Session {
        let path = root.join("project");
        std::fs::create_dir_all(&path).unwrap();
        let Output::Project(project) = self
            .local
            .execute(self.local.prepare(Command::RegisterProject {
                name: "History fixture".into(),
                path: path.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Session(session) = self
            .local
            .execute(self.local.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(crate::store::tests::config("history-fixture")),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        session
    }

    async fn start(&self, client: &Client, session: &Session) -> TurnId {
        let Output::QueuedTurn(queued) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Read the saved result".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        let invocation = self
            .store
            .ingress
            .claim_run(CancellationToken::new())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(invocation.turn.id, queued.id);
        let input = Record {
            turn: queued.id,
            ..message(
                "user",
                Role::User,
                vec![BlockKind::Text {
                    text: "Read the saved result".into(),
                }],
            )
        };
        self.store
            .ingress
            .sessions(queued.id)
            .append_event(&session.id.to_string(), input)
            .await
            .unwrap();
        queued.id
    }

    async fn finish(&self, turn: TurnId) {
        assert_eq!(
            self.store
                .ingress
                .finish_run(turn, Status::Completed, None, None)
                .await
                .unwrap(),
            Status::Completed
        );
        self.store.ingress.agents.finish(turn);
    }

    async fn close(self) {
        self.controller.close().await.unwrap();
        self.network.close().await.unwrap();
        self.store.shutdown().await.unwrap();
        drop(self.profile);
    }
}

async fn history(client: &Client, session: SessionId) -> History {
    let Output::Conversation(history) = client
        .execute(client.prepare(Command::ReadConversation {
            session,
            before: None,
            limit: 20,
        }))
        .await
        .unwrap()
    else {
        panic!("history expected")
    };
    history
}

async fn output(client: &Client, turn: TurnId, reference: &OutputReference) -> OutputPage {
    let Output::ToolOutput(page) = client
        .execute(client.prepare(Command::ReadToolOutput {
            turn,
            output: reference.clone(),
            stream: None,
            offset: 0,
            limit: sailry_protocol::tool::MAX_OUTPUT_PAGE_BYTES,
        }))
        .await
        .unwrap()
    else {
        panic!("tool output expected")
    };
    page
}

async fn image(client: &Client, session: SessionId, image: &Image) -> Vec<u8> {
    let Output::AttachmentDownload(download) = client
        .execute(client.prepare(Command::DownloadImage {
            session,
            image: image.clone(),
        }))
        .await
        .unwrap()
    else {
        panic!("image download expected")
    };
    assert_eq!(download.attachment, image.attachment);
    let mut bytes = Vec::new();
    client
        .download_attachment(&download, &mut bytes, CancellationToken::new(), |_| {})
        .await
        .unwrap();
    bytes
}

#[tokio::test]
async fn preserves_captured_reads() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::open(directory.path()).await;
    let session = fixture.session(directory.path()).await;
    let turn = fixture.start(&fixture.local, &session).await;
    use goose_provider_types::conversation::message::{Message, MessageContent};
    use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
    use std::collections::BTreeMap;

    let mut source = Message::assistant()
        .with_text("Before the call")
        .with_tool_request(
            "saved-call",
            Ok(CallToolRequestParams::new("read_file").with_arguments(
                serde_json::from_value(serde_json::json!({"path":"saved.txt"})).unwrap(),
            )),
        );
    source.id = Some("saved-call-entry".into());
    let MessageContent::ToolRequest(request) = &mut source.content[1] else {
        panic!("tool request expected")
    };
    request.metadata = Some(
        serde_json::from_value(serde_json::json!({
            "thoughtSignature":"saved-signature", "opaque":{"retained":[true,2]}
        }))
        .unwrap(),
    );
    request.tool_meta = Some(serde_json::json!({"title":"Read saved file"}));
    let original = serde_json::json!({
        "text":"Saved result",
        "image":{"mimeType":"image/png","data":"AAEC/f7/"},
        "extension":{"preserved":[true,2,"value"]}
    });
    let bytes = vec![0, 1, 2, 253, 254, 255];
    let db = fixture
        .store
        .ingress
        .storage
        .get()
        .unwrap()
        .handle
        .lease(false)
        .await
        .unwrap();
    let transaction = db.unchecked_transaction().await.unwrap();
    let mut call = conversion::capture(&transaction, turn, &source, &BTreeMap::new())
        .await
        .unwrap();
    call.provenance.branch = "saved-branch".into();
    let mut tool_result = CallToolResult::structured(original.clone());
    tool_result
        .content
        .push(ContentBlock::audio("BAU=", "audio/wav"));
    tool_result
        .content
        .push(ContentBlock::image("AAEC/f7/", "image/png"));
    let encoded = serde_json::to_value(&tool_result).unwrap();
    let mut result_source = Message::user()
        .with_text("Before the result")
        .with_tool_response("saved-call", Ok(tool_result));
    result_source.id = Some("saved-result-entry".into());
    let mut result = conversion::capture(
        &transaction,
        turn,
        &result_source,
        &BTreeMap::from([("saved-call".into(), "read_file".into())]),
    )
    .await
    .unwrap();
    result.provenance.branch = call.provenance.branch.clone();
    transaction.commit().await.unwrap();
    let stored: Record = serde_json::from_slice(&serde_json::to_vec(&call).unwrap()).unwrap();
    assert_eq!(
        conversion::project(&db, &stored).await.unwrap().unwrap(),
        source
    );
    assert_eq!(
        conversion::project(&db, &result).await.unwrap().unwrap(),
        result_source
    );
    drop(db);
    let sessions = fixture.store.ingress.sessions(turn);
    for event in [call, result] {
        sessions
            .append_event(&session.id.to_string(), event)
            .await
            .unwrap();
    }
    drop(sessions);
    let reference = OutputReference {
        call: "saved-call".into(),
        revision: blake3::hash(encoded.to_string().as_bytes())
            .to_hex()
            .to_string(),
    };
    let expected_output = output(&fixture.local, turn, &reference).await;
    assert_eq!(expected_output.text, encoded.to_string());
    assert_eq!(expected_output.turn, turn);
    fixture.finish(turn).await;
    let expected = history(&fixture.local, session.id).await;
    assert_eq!(
        history(&fixture.remote, session.id).await.page,
        expected.page
    );
    let entry = expected
        .page
        .entries
        .iter()
        .find(|entry| entry.id == "saved-result-entry")
        .unwrap();
    assert_eq!(entry.author, "assistant");
    assert_eq!(entry.branch, "saved-branch");
    let Part::ToolResult { id, images, .. } = &entry.parts[1] else {
        panic!("tool result expected at its original slot")
    };
    assert_eq!(id.as_deref(), Some("saved-call"));
    let saved_image = images.first().unwrap().clone();
    assert_eq!(saved_image.entry, "saved-result-entry");
    assert_eq!(saved_image.part, 1);
    assert_eq!(saved_image.index, 0);
    assert_eq!(image(&fixture.local, session.id, &saved_image).await, bytes);
    fixture.close().await;

    let fixture = Fixture::open(directory.path()).await;
    for client in [&fixture.local, &fixture.remote] {
        assert_eq!(history(client, session.id).await.page, expected.page);
        assert_eq!(image(client, session.id, &saved_image).await, bytes);
    }
    for client in [&fixture.local, &fixture.remote] {
        // Output reads belong to the active caller's invocation after restart.
        let consumer = fixture.start(client, &session).await;
        assert_eq!(output(client, consumer, &reference).await, expected_output);
        fixture.finish(consumer).await;
    }
    fixture.close().await;
}
