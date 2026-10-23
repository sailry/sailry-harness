use super::*;

#[tokio::test]
async fn admits_mixed_streams() {
    let fixture = Fixture::start().await;
    let data = b"Independent transfer bytes";
    for index in 0..6 {
        std::fs::write(fixture.root.join(format!("source-{index}")), data).unwrap();
    }
    for (index, transport) in fixture.transports.iter().enumerate() {
        let client = Client::new(transport.clone());
        let other = Client::new(fixture.transports[1 - index].clone());
        let mut uploads = Vec::new();
        let mut downloads = Vec::new();
        for item in 0..6 {
            uploads.push(
                prepare(
                    &client,
                    spec(fixture.worktree, &format!("{index}-{item}"), data, None),
                )
                .await,
            );
            let Output::FileDownload(download) = client
                .execute(client.prepare(Command::DownloadFile {
                    worktree: fixture.worktree,
                    path: format!("source-{item}"),
                }))
                .await
                .unwrap()
            else {
                panic!("download expected");
            };
            downloads.push(download);
        }
        // Every handle remains unopened until all twelve independent reads and
        // writes have been admitted for the same authenticated caller.
        assert_eq!(uploads.len() + downloads.len(), 12);
        assert!(matches!(
            other.open(downloads[0].stream).await,
            Err(Fault {
                code: ErrorCode::NotFound,
                ..
            })
        ));
        let writing = futures::future::join_all(uploads.iter().map(|upload| {
            let client = &client;
            let root = &fixture.root;
            async move {
                stage(client, upload, data).await;
                assert!(!root.join(&upload.spec.path).exists());
                let request = finish(client, upload);
                let admitted = client.dispatch(request.clone()).await.unwrap();
                assert!(admitted.receipt.durable);
                let output = admitted.completion.await.unwrap().unwrap();
                assert_eq!(
                    output,
                    Output::FileWritten(FileWritten {
                        path: upload.spec.path.clone(),
                        size: data.len() as u64,
                        revision: upload.spec.revision.clone(),
                    })
                );
                assert_eq!(std::fs::read(root.join(&upload.spec.path)).unwrap(), data);
                (request, output)
            }
        }));
        let reading = futures::future::join_all(downloads.iter().map(|download| {
            let client = &client;
            async move {
                let mut bytes = Vec::new();
                client
                    .download(download, &mut bytes, CancellationToken::new(), |_| {})
                    .await
                    .unwrap();
                assert_eq!(bytes, data);
                assert!(client.open(download.stream).await.is_err());
            }
        }));
        let (completed, _) = tokio::join!(writing, reading);
        let requests = completed
            .iter()
            .map(|(request, _)| request.id)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(requests.len(), uploads.len());
        for (request, output) in completed {
            let Command::FinishFileUpload { path, .. } = &request.command else {
                panic!("upload completion expected");
            };
            let path = fixture.root.join(path);
            std::fs::write(&path, b"external contents").unwrap();
            assert_eq!(client.execute(request).await.unwrap(), output);
            assert_eq!(std::fs::read(path).unwrap(), b"external contents");
        }
    }
    fixture.close().await;
}
