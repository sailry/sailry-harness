use super::*;

#[tokio::test]
async fn revalidates_queued_recycling() {
    let (fixture, mut gate) = blocked().await;
    let root = fixture.directory.path().join("project");
    std::fs::write(root.join("file"), "original").unwrap();
    let Output::FileDownload(download) = fixture
        .client
        .execute(fixture.client.prepare(Command::DownloadFile {
            worktree: fixture.worktree,
            path: "file".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("download expected")
    };
    fixture
        .client
        .execute(fixture.client.prepare(Command::CancelFileTransfer {
            stream: download.stream,
        }))
        .await
        .unwrap();
    let blocked = fixture
        .client
        .dispatch(fixture.write("blocked", "gate"))
        .await
        .unwrap();
    gate.entered.recv().await.unwrap();
    let request = fixture.client.prepare(Command::TrashFile {
        worktree: fixture.worktree,
        path: "file".into(),
        expected_revision: download.revision,
        expected_stamp: download.stamp,
    });
    let queued = fixture.client.dispatch(request.clone()).await.unwrap();
    assert!(queued.receipt.durable);
    // Receipt precedes dispatch validation; synchronize before changing the queued source.
    fixture
        .client
        .execute(fixture.client.prepare(Command::Snapshot))
        .await
        .unwrap();
    std::fs::rename(root.join("file"), root.join("retained")).unwrap();
    std::fs::write(root.join("file"), "original").unwrap();
    gate.release.send(()).unwrap();
    blocked.completion.await.unwrap().unwrap();
    assert_eq!(
        queued.completion.await.unwrap().unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        fixture.client.execute(request).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(std::fs::read(root.join("file")).unwrap(), b"original");
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn reserves_paths_until_completion() {
    let (entered, mut waiting) = mpsc::unbounded_channel();
    let (release, released) = std::sync::mpsc::channel();
    let fixture = Fixture::start(move |roots, request| {
        if let Command::TrashEntry { path, .. } = &request.command {
            entered.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(10)).unwrap();
            // Test admission ordering without touching the system Trash.
            let root = &roots.target;
            std::fs::rename(root.join(path), root.parent().unwrap().join("recovered")).unwrap();
            Ok(Output::EntryTrashed { path: path.clone() })
        } else {
            mutations::execute(roots, request)
        }
    })
    .await;
    let root = fixture.directory.path().join("project");
    std::fs::create_dir(root.join("folder")).unwrap();
    let path = if root.join("FOLDER").exists() {
        "FOLDER"
    } else {
        "folder"
    };
    let request = fixture.client.prepare(Command::TrashEntry {
        worktree: fixture.worktree,
        path: path.into(),
    });
    let pending = fixture.client.dispatch(request.clone()).await.unwrap();
    waiting.recv().await.unwrap();
    let registration = || Command::RegisterProject {
        name: "Nested".into(),
        path: root.join("folder").to_str().unwrap().into(),
    };
    assert_eq!(
        fixture
            .client
            .execute(fixture.client.prepare(registration()))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(
        fixture.client.execute(request).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    fixture
        .client
        .execute(fixture.client.prepare(Command::Snapshot))
        .await
        .unwrap();
    release.send(()).unwrap();
    pending.completion.await.unwrap().unwrap();
    std::fs::create_dir(root.join("folder")).unwrap();
    fixture
        .client
        .execute(fixture.client.prepare(registration()))
        .await
        .unwrap();
    fixture.store.shutdown().await.unwrap();
}
