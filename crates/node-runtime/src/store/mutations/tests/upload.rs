use super::*;

#[tokio::test]
async fn cancels_durable_publication() {
    let (fixture, mut gate) = blocked().await;
    let network = Link::bind(
        &fixture.identity,
        NetworkScope::default(),
        fixture.store.ingress.clone(),
        fixture.store.ingress.clone(),
    )
    .await
    .unwrap();
    let controller = Link::controller(
        fixture.directory.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let address = controller
        .handle()
        .pair(network.handle().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (index, client) in [&fixture.client, &remote].into_iter().enumerate() {
        let first = fixture
            .client
            .dispatch(fixture.write("blocked", "first"))
            .await
            .unwrap();
        gate.entered.recv().await.unwrap();
        let mut queued = Vec::new();
        for cancel in [false, true] {
            let path = format!("upload-{index}-{cancel}");
            let Output::FileUpload(upload) = client
                .execute(client.prepare(Command::UploadFile(FileUploadSpec {
                    worktree: fixture.worktree,
                    path: path.clone(),
                    size: 4,
                    revision: blake3::hash(b"ours").to_hex().to_string(),
                    expected_revision: None,
                })))
                .await
                .unwrap()
            else {
                panic!("upload expected")
            };
            client
                .upload(
                    &upload,
                    &mut &b"ours"[..],
                    sailry_link::CancellationToken::new(),
                    |_| {},
                )
                .await
                .unwrap();
            let request = client.prepare(Command::FinishFileUpload {
                worktree: fixture.worktree,
                path: path.clone(),
                stream: upload.stream,
            });
            let admission =
                tokio::time::timeout(Duration::from_secs(2), client.dispatch(request.clone()))
                    .await
                    .unwrap()
                    .unwrap();
            assert!(admission.receipt.durable);
            let db =
                rusqlite::Connection::open(fixture.directory.path().join("node.sqlite3")).unwrap();
            assert_eq!(
                db.query_row(
                    "SELECT status FROM requests WHERE id=?1",
                    [request.id.to_string()],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
                "admitted"
            );
            assert_eq!(
                client.execute(request.clone()).await.unwrap_err().code,
                ErrorCode::OutcomeUnknown
            );
            client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap();
            assert_eq!(
                client
                    .execute(
                        client.prepare(Command::RegisterProject {
                            name: "Racing registration".into(),
                            path: fixture
                                .directory
                                .path()
                                .join("project")
                                .join(&path)
                                .to_str()
                                .unwrap()
                                .into()
                        })
                    )
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Busy
            );
            if cancel {
                client
                    .execute(client.prepare(Command::CancelFileTransfer {
                        stream: upload.stream,
                    }))
                    .await
                    .unwrap();
            }
            queued.push((cancel, path, request, admission));
        }
        gate.release.send(()).unwrap();
        let _ = first.completion.await.unwrap();
        for (cancel, path, request, admission) in queued {
            let result = admission.completion.await.unwrap();
            if cancel {
                assert_eq!(result.unwrap_err().code, ErrorCode::NotFound);
                assert!(
                    !fixture
                        .directory
                        .path()
                        .join("project")
                        .join(&path)
                        .exists()
                );
            } else {
                assert!(matches!(result.unwrap(), Output::FileWritten(_)));
                std::fs::write(
                    fixture.directory.path().join("project").join(&path),
                    "external",
                )
                .unwrap();
                client.execute(request).await.unwrap();
                assert_eq!(
                    std::fs::read(fixture.directory.path().join("project").join(&path)).unwrap(),
                    b"external"
                );
            }
        }
    }
    controller.close().await.unwrap();
    network.close().await.unwrap();
    fixture.store.shutdown().await.unwrap();
}
