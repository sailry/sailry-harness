use super::*;
use crate::store::Store;
use sailry_client::Client;
use sailry_link::{Identity, Local};
use sailry_protocol::{Command, Output, Permission, ProviderId, SessionConfig, WorkMode};

const BYTES: &[u8] = b"Cached attachment evidence";

struct Fixture {
    _directory: tempfile::TempDir,
    profile: crate::Profile,
    store: Store,
    client: Client,
    invocation: Arc<Invocation>,
    attachment: Attachment,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let profile = crate::Profile::acquire(&directory.path().join("node")).unwrap();
        let identity = Identity::open(&profile.path).unwrap();
        let store = Store::open(
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
        let client = Client::new(Arc::new(Local::new(
            identity.id(),
            identity.id(),
            store.ingress.clone(),
        )));
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: None,
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Default::default(),
                    features: Default::default(),
                    mode: WorkMode::Code,
                    options: Default::default(),
                    permission: Permission::Ask,
                    credential: None,
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::AttachmentUpload(upload) = client
            .execute(client.prepare(Command::UploadAttachment(Spec {
                worktree: session.worktree,
                name: "input.txt".into(),
                media_type: "text/plain".into(),
                size: BYTES.len() as u64,
                revision: blake3::hash(BYTES).to_hex().to_string(),
            })))
            .await
            .unwrap()
        else {
            panic!("upload expected")
        };
        client
            .upload(&upload, &mut &BYTES[..], CancellationToken::new(), |_| {})
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
            panic!("attachment expected")
        };
        client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: Input {
                    text: "Read the attachment".into(),
                    attachments: vec![attachment.id],
                    ..Input::default()
                },
            }))
            .await
            .unwrap();
        let invocation = Arc::new(
            store
                .ingress
                .claim_run(CancellationToken::new())
                .await
                .unwrap()
                .unwrap(),
        );
        let record = input(&store.ingress, &invocation).await.unwrap();
        store
            .ingress
            .sessions(invocation.turn.id)
            .append_event(&session.id.to_string(), record)
            .await
            .unwrap();
        Self {
            _directory: directory,
            profile,
            store,
            client,
            invocation,
            attachment,
        }
    }

    fn messages(&self) -> Vec<ModelMessage> {
        vec![ModelMessage::user().with_document(
            format!("{PREFIX}{}", self.attachment.id),
            self.attachment.spec.media_type.clone(),
            None,
        )]
    }

    async fn prepare(&self, cache: &Cache, messages: &mut [ModelMessage]) -> Result<(), Fault> {
        hydrate(
            &self.store.ingress,
            &self.invocation,
            cache,
            messages,
            true,
            ModelApi::Responses,
            &CancellationToken::new(),
        )
        .await
    }

    async fn historical(&mut self) {
        self.store
            .ingress
            .finish_run(
                self.invocation.turn.id,
                sailry_protocol::conversation::Status::Completed,
                None,
                None,
            )
            .await
            .unwrap();
        self.store.ingress.agents.finish(self.invocation.turn.id);
        self.client
            .execute(self.client.prepare(Command::SubmitTurn {
                session: self.invocation.turn.session,
                expected_revision: 1,
                message: "Continue".into(),
            }))
            .await
            .unwrap();
        self.invocation = Arc::new(
            self.store
                .ingress
                .claim_run(CancellationToken::new())
                .await
                .unwrap()
                .unwrap(),
        );
    }

    fn remove_file(&self) {
        std::fs::remove_file(self.path()).unwrap();
    }

    fn path(&self) -> std::path::PathBuf {
        self.profile
            .path
            .join("attachments")
            .join(self.attachment.id.to_string())
    }

    async fn guide(&self, cache: &Cache) {
        self.client
            .execute(self.client.prepare(Command::GuideTurn {
                recipient: self.invocation.turn.id,
                expected_revision: self.invocation.turn.revision,
                message: Input {
                    text: "Read this attachment now".into(),
                    attachments: vec![self.attachment.id],
                    ..Input::default()
                },
            }))
            .await
            .unwrap();
        let inputs = self
            .store
            .ingress
            .take_guidance(self.invocation.turn.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(inputs.len(), 1);
        cache
            .promote(
                inputs
                    .iter()
                    .flat_map(|input| input.message.attachments.iter().copied()),
            )
            .await;
    }

    async fn close(self) {
        self.store.ingress.agents.finish(self.invocation.turn.id);
        self.store.stop_admission();
        self.store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn resolves_each_attachment_once() {
    let fixture = Fixture::new().await;
    let cache = Cache::default();
    let canonical = fixture.messages();
    let mut expected = canonical.clone();
    fixture.prepare(&cache, &mut expected).await.unwrap();
    assert!(
        expected[0]
            .as_concat_text()
            .contains("Cached attachment evidence")
    );
    let ingress = fixture.store.ingress.clone();
    let invocation = fixture.invocation.clone();
    fixture.close().await;
    let mut repeated = canonical;
    hydrate(
        &ingress,
        &invocation,
        &cache,
        &mut repeated,
        true,
        ModelApi::Responses,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(repeated, expected);
}

#[tokio::test]
async fn distinguishes_current_and_historical_failures() {
    for historical in [false, true] {
        let mut fixture = Fixture::new().await;
        if historical {
            fixture.historical().await;
        }
        fixture.remove_file();
        let cache = Cache::default();
        let mut messages = fixture.messages();
        let result = fixture.prepare(&cache, &mut messages).await;
        if historical {
            result.unwrap();
            assert!(messages[0].as_concat_text().contains(UNAVAILABLE_NOTE));
            let mut repeated = fixture.messages();
            fixture.prepare(&cache, &mut repeated).await.unwrap();
            assert_eq!(messages, repeated);
            assert!(cache.entries.lock().await[&fixture.attachment.id].reported);
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::NotFound);
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn fails_when_history_joins_guidance() {
    for changed in [false, true] {
        let mut fixture = Fixture::new().await;
        fixture.historical().await;
        let expected = if changed {
            std::fs::write(fixture.path(), b"Changed attachment evidence").unwrap();
            ErrorCode::RevisionConflict
        } else {
            fixture.remove_file();
            ErrorCode::NotFound
        };
        let cache = Cache::default();
        let mut historical = fixture.messages();
        fixture.prepare(&cache, &mut historical).await.unwrap();
        assert!(historical[0].as_concat_text().contains(UNAVAILABLE_NOTE));
        assert!(
            !cache.entries.lock().await[&fixture.attachment.id]
                .source
                .current
        );
        fixture.guide(&cache.clone()).await;
        assert!(
            cache.entries.lock().await[&fixture.attachment.id]
                .source
                .current
        );
        std::fs::write(fixture.path(), BYTES).unwrap();
        let mut current = fixture.messages();
        assert_eq!(
            fixture
                .prepare(&cache, &mut current)
                .await
                .unwrap_err()
                .code,
            expected
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn resets_budget_in_request_order() {
    let fixture = Fixture::new().await;
    let cache = Cache::default();
    let mut initial = fixture.messages();
    fixture.prepare(&cache, &mut initial).await.unwrap();
    cache
        .entries
        .lock()
        .await
        .get_mut(&fixture.attachment.id)
        .unwrap()
        .source
        .attachment
        .as_mut()
        .unwrap()
        .spec
        .size = REQUEST_BYTES / 2 + 1;
    let mut canonical = fixture.messages();
    canonical.extend(fixture.messages());
    for _ in 0..2 {
        let mut messages = canonical.clone();
        fixture.prepare(&cache, &mut messages).await.unwrap();
        assert!(
            messages[0]
                .as_concat_text()
                .contains("Cached attachment evidence")
        );
        assert!(
            !messages[1]
                .as_concat_text()
                .contains("Cached attachment evidence")
        );
        assert!(messages[1].as_concat_text().contains(READ_NOTE));
    }
    fixture.close().await;
}
