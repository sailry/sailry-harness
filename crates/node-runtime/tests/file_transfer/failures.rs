use super::*;
use sailry_link::{Admission, Pending, Stream, Subscription};
use std::{sync::Mutex, time::Duration};

enum Effect {
    Observe,
    WrongPath,
    Hold(Arc<tokio::sync::Notify>),
}

struct Observed {
    inner: Arc<dyn Transport>,
    effect: Effect,
    uploads: Mutex<Vec<FileUpload>>,
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn open(&self, stream: StreamId) -> Pending<'_, Result<Stream, Fault>> {
        Box::pin(async move {
            if let Effect::Hold(entered) = &self.effect {
                entered.notify_one();
                std::future::pending::<()>().await;
            }
            self.inner.open(stream).await
        })
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let record = matches!(request.command, Command::UploadFile(_));
            let mut admission = self.inner.dispatch(request).await?;
            if record {
                let mut output = admission.completion.await.unwrap()?;
                let Output::FileUpload(upload) = &mut output else {
                    panic!("upload expected")
                };
                self.uploads.lock().unwrap().push(upload.clone());
                if matches!(self.effect, Effect::WrongPath) {
                    upload.spec.path = "wrong".into();
                }
                let (sender, receiver) = tokio::sync::oneshot::channel();
                sender.send(Ok(output)).unwrap();
                admission.completion = receiver;
            }
            Ok(admission)
        })
    }
}

impl Observed {
    fn new(inner: Arc<dyn Transport>, effect: Effect) -> Arc<Self> {
        Arc::new(Self {
            inner,
            effect,
            uploads: Mutex::new(Vec::new()),
        })
    }

    async fn assert_released(&self) {
        let client = Client::new(self.inner.clone());
        let uploads = self.uploads.lock().unwrap().clone();
        for upload in uploads {
            assert!(client.open(upload.stream).await.is_err());
            assert!(
                client
                    .execute(client.prepare(Command::FinishFileUpload {
                        worktree: upload.spec.worktree,
                        path: upload.spec.path,
                        stream: upload.stream,
                    }))
                    .await
                    .is_err()
            );
        }
    }
}

#[tokio::test]
async fn rejects_changed_inputs() {
    let fixture = Fixture::start().await;
    let data = vec![0xab; 2 * 1024 * 1024];
    for pair in PATHS {
        for mode in [
            "source", "hash", "size", "spec", "response", "cancel", "progress",
        ] {
            std::fs::write(fixture.source.root.join("file"), &data).unwrap();
            let (source, _) = fixture.clients(pair);
            let observed = Observed::new(
                fixture.destination.transports[pair.1].clone(),
                if mode == "response" {
                    Effect::WrongPath
                } else {
                    Effect::Observe
                },
            );
            let target = Client::new(observed.clone());
            let mut download = prepare(&source, fixture.source.worktree, "file").await;
            match mode {
                "source" => std::fs::write(fixture.source.root.join("file"), b"changed").unwrap(),
                "hash" => download.revision = blake3::hash(b"wrong").to_hex().to_string(),
                "size" => download.size += 1,
                _ => {}
            }
            let mut destination = spec(&download, fixture.destination.worktree, "copy");
            if mode == "spec" {
                destination.size += 1;
            }
            let cancel = CancellationToken::new();
            if mode == "cancel" {
                cancel.cancel();
            }
            let stop = cancel.clone();
            let counts = (fixture.source.count(), fixture.destination.count());
            let result = target
                .stage_copy(&source, &download, destination, cancel, |bytes| {
                    if mode == "progress" && bytes > 0 {
                        stop.cancel();
                    }
                })
                .await;
            let code = result.unwrap_err().code;
            assert_eq!(
                code,
                match mode {
                    "spec" => ErrorCode::InvalidRequest,
                    "response" => ErrorCode::Internal,
                    "cancel" | "progress" => ErrorCode::Cancelled,
                    _ => ErrorCode::RevisionConflict,
                },
                "{mode} {pair:?}"
            );
            assert!(!fixture.destination.root.join("copy").exists());
            assert_eq!(fixture.source.count(), counts.0);
            assert_eq!(fixture.destination.count(), counts.1);
            assert!(source.open(download.stream).await.is_err());
            observed.assert_released().await;
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn cancels_both_blocked_streams() {
    let fixture = Fixture::start().await;
    std::fs::write(
        fixture.source.root.join("file"),
        vec![0xf1; 10 * 1024 * 1024],
    )
    .unwrap();
    for pair in PATHS {
        let (source, _) = fixture.clients(pair);
        let entered = Arc::new(tokio::sync::Notify::new());
        let observed = Observed::new(
            fixture.destination.transports[pair.1].clone(),
            Effect::Hold(entered.clone()),
        );
        let target = Client::new(observed.clone());
        let download = prepare(&source, fixture.source.worktree, "file").await;
        let cancel = CancellationToken::new();
        let copying = target.stage_copy(
            &source,
            &download,
            spec(&download, fixture.destination.worktree, "copy"),
            cancel.clone(),
            |_| {},
        );
        let control = async {
            entered.notified().await;
            for client in [&source, &target] {
                tokio::time::timeout(
                    Duration::from_secs(2),
                    client.execute(client.prepare(Command::Snapshot)),
                )
                .await
                .unwrap()
                .unwrap();
            }
            assert!(!fixture.destination.root.join("copy").exists());
            cancel.cancel();
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(copying, control)
        })
        .await
        .unwrap();
        assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
        assert!(source.open(download.stream).await.is_err());
        observed.assert_released().await;
    }
    fixture.close().await;
}

#[tokio::test]
async fn preserves_conflicting_sources() {
    let fixture = Fixture::start().await;
    std::fs::write(fixture.source.root.join("file"), "source").unwrap();
    std::fs::write(fixture.destination.root.join("file"), "target").unwrap();
    for pair in PATHS {
        let (source, target) = fixture.clients(pair);
        for mode in ["scope", "path", "existing"] {
            let download = prepare(&source, fixture.source.worktree, "file").await;
            let mut destination = spec(&download, fixture.destination.worktree, "file");
            match mode {
                "scope" => destination.worktree = fixture.source.worktree,
                "path" => destination.path = "../escape".into(),
                _ => {}
            }
            assert!(
                target
                    .stage_copy(
                        &source,
                        &download,
                        destination,
                        CancellationToken::new(),
                        |_| {}
                    )
                    .await
                    .is_err()
            );
            assert!(source.open(download.stream).await.is_err());
        }
        let download = prepare(&source, fixture.source.worktree, "file").await;
        let mut destination = spec(&download, fixture.destination.worktree, "file");
        destination.expected_revision = Some(blake3::hash(b"target").to_hex().to_string());
        let request = target
            .stage_copy(
                &source,
                &download,
                destination,
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(fixture.destination.root.join("file")).unwrap(),
            b"target"
        );
        std::fs::write(fixture.destination.root.join("file"), "external").unwrap();
        assert_eq!(
            target.execute(request).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            std::fs::read(fixture.destination.root.join("file")).unwrap(),
            b"external"
        );
        assert_eq!(
            std::fs::read(fixture.source.root.join("file")).unwrap(),
            b"source"
        );
        std::fs::write(fixture.destination.root.join("file"), "target").unwrap();
    }
    fixture.close().await;
}
