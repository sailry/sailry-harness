use super::*;
use sailry_link::{Admission, Pending, Stream, Subscription};
use std::sync::Mutex;

enum Effect {
    Corrupt,
    Oversized,
    WrongPath,
    Cancel(CancellationToken),
}

struct Altered {
    inner: Arc<dyn Transport>,
    effect: Effect,
    streams: Mutex<Vec<StreamId>>,
}

impl Transport for Altered {
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
            let alter = matches!(
                request.command,
                Command::DownloadFile { .. } | Command::UploadFile(_)
            );
            let mut admission = self.inner.dispatch(request).await?;
            if !alter {
                return Ok(admission);
            }
            let mut output = admission.completion.await.unwrap()?;
            match &mut output {
                Output::FileDownload(download) => {
                    self.streams.lock().unwrap().push(download.stream);
                    match &self.effect {
                        Effect::Corrupt => {
                            download.revision = blake3::hash(b"incorrect").to_hex().to_string()
                        }
                        Effect::Oversized => download.size = MAX_DOCUMENT_BYTES as u64 + 1,
                        Effect::WrongPath => download.path = "wrong.txt".into(),
                        Effect::Cancel(cancel) => cancel.cancel(),
                    }
                }
                Output::FileUpload(upload) => {
                    self.streams.lock().unwrap().push(upload.stream);
                    match &self.effect {
                        Effect::Cancel(cancel) => cancel.cancel(),
                        _ => upload.spec.path = "wrong.txt".into(),
                    }
                }
                _ => panic!("file transfer expected"),
            }
            let (sender, receiver) = tokio::sync::oneshot::channel();
            sender.send(Ok(output)).unwrap();
            admission.completion = receiver;
            Ok(admission)
        })
    }
}

#[tokio::test]
async fn failures_release_staging() {
    let fixture = Fixture::start().await;
    let text = "large document\n".repeat(20_000);
    std::fs::write(fixture.root.join("file.txt"), &text).unwrap();
    let expected = blake3::hash(text.as_bytes()).to_hex().to_string();
    for inner in &fixture.transports {
        for effect in [
            Effect::Corrupt,
            Effect::Oversized,
            Effect::WrongPath,
            Effect::Cancel(CancellationToken::new()),
        ] {
            let cancel = match &effect {
                Effect::Cancel(cancel) => cancel.clone(),
                _ => CancellationToken::new(),
            };
            let altered = Arc::new(Altered {
                inner: inner.clone(),
                effect,
                streams: Mutex::new(Vec::new()),
            });
            let client = Client::new(altered.clone());
            let result = client
                .read_document(fixture.worktree, "file.txt", cancel)
                .await;
            match &altered.effect {
                Effect::Oversized => {
                    let preview = result.unwrap();
                    assert!(preview.truncated);
                    assert!(preview.revision.is_none());
                    assert!(preview.text.len() <= MAX_FILE_BYTES);
                }
                Effect::Corrupt => {
                    assert_eq!(result.unwrap_err().code, ErrorCode::RevisionConflict)
                }
                Effect::WrongPath => assert_eq!(result.unwrap_err().code, ErrorCode::Internal),
                Effect::Cancel(_) => assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled),
            }
            let streams = altered.streams.lock().unwrap().clone();
            assert_eq!(streams.len(), 1);
            assert!(inner.open(streams[0]).await.is_err());
        }
        for effect in [Effect::WrongPath, Effect::Cancel(CancellationToken::new())] {
            let cancel = match &effect {
                Effect::Cancel(cancel) => cancel.clone(),
                _ => CancellationToken::new(),
            };
            let altered = Arc::new(Altered {
                inner: inner.clone(),
                effect,
                streams: Mutex::new(Vec::new()),
            });
            let client = Client::new(altered.clone());
            let before = fixture.count();
            assert!(
                client
                    .stage_document(fixture.worktree, "file.txt", &text, &expected, cancel)
                    .await
                    .is_err()
            );
            assert_eq!(fixture.count(), before);
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("file.txt")).unwrap(),
                text
            );
            let streams = altered.streams.lock().unwrap().clone();
            assert_eq!(streams.len(), 1);
            assert!(inner.open(streams[0]).await.is_err());
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn preserves_uncertain_publication() {
    let fixture = Fixture::start().await;
    let database =
        rusqlite::Connection::open(fixture.profile.join("storage/node.sqlite3")).unwrap();
    database.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let mut pending = Vec::new();
    let text = "verified text\n".repeat(20_000);
    for (index, transport) in fixture.transports.iter().enumerate() {
        let path = format!("{index}.txt");
        std::fs::write(fixture.root.join(&path), "original").unwrap();
        let client = Client::new(transport.clone());
        let expected = blake3::hash(b"original").to_hex().to_string();
        let staged = client
            .stage_document(
                fixture.worktree,
                &path,
                &text,
                &expected,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(
            client
                .execute(staged.request.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(&path)).unwrap(),
            text
        );
        std::fs::write(fixture.root.join(path), "external").unwrap();
        assert_eq!(
            client
                .execute(staged.request.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        pending.push(staged);
    }
    database.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(database);
    fixture.node.shutdown().await.unwrap();
    let node = Node::start(&fixture.profile).await.unwrap();
    for (index, transport) in [
        node.local(),
        fixture.controller.handle().remote(node.link().address()),
    ]
    .into_iter()
    .enumerate()
    {
        let client = Client::new(transport);
        assert_eq!(
            client
                .execute(pending[index].request.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(format!("{index}.txt"))).unwrap(),
            "external"
        );
    }
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
