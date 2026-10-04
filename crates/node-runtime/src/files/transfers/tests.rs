use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod upload;

async fn prepared(
    transfers: &Arc<Transfers>,
    root: &std::path::Path,
    caller: NodeId,
) -> FileDownload {
    transfers
        .download(
            caller,
            WorktreeId::new(),
            root.to_owned(),
            "file".into(),
            CancellationToken::new(),
        )
        .await
        .unwrap()
}

async fn released(transfers: &Transfers) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while transfers.capacity.available_permits() != CAPACITY {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(transfers.entries.lock().unwrap().entries.is_empty());
}

#[tokio::test]
async fn confines_single_use_handles() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    std::fs::write(
        root.path().join("file"),
        vec![42; FILE_TRANSFER_CHUNK_BYTES * 4],
    )
    .unwrap();
    let transfers = Transfers::new(None);
    let first = NodeId([1; 32]);
    let second = NodeId([2; 32]);
    let mut files = Vec::new();
    for caller in [first, second] {
        for _ in 0..PER_CALLER {
            files.push((caller, prepared(&transfers, root.path(), caller).await));
        }
        assert_eq!(
            transfers
                .download(
                    caller,
                    WorktreeId::new(),
                    root.path().into(),
                    "file".into(),
                    CancellationToken::new()
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
    }
    let id = files[0].1.stream;
    assert!(matches!(
        transfers.open(second, id),
        Err(Fault {
            code: ErrorCode::NotFound,
            ..
        })
    ));
    assert_eq!(
        transfers.cancel(second, id).unwrap_err().code,
        ErrorCode::NotFound
    );
    let mut stream = transfers.open(first, id).unwrap();
    stream.shutdown().await.unwrap();
    assert!(matches!(
        transfers.open(first, id),
        Err(Fault {
            code: ErrorCode::Conflict,
            ..
        })
    ));
    // An active stream is not subject to the unopened-handle deadline.
    transfers.expire(id);
    assert!(transfers.entries.lock().unwrap().entries.contains_key(&id));
    for (caller, file) in files {
        transfers.cancel(caller, file.stream).unwrap();
    }
    transfers.cancel(first, id).unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    assert!(bytes.len() < FILE_TRANSFER_CHUNK_BYTES * 4);
    released(&transfers).await;
}

#[tokio::test]
async fn releases_closed_transfers() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    std::fs::write(
        root.path().join("file"),
        vec![0; FILE_TRANSFER_CHUNK_BYTES * 4],
    )
    .unwrap();
    let transfers = Transfers::new(None);
    let caller = NodeId([1; 32]);
    let expired = prepared(&transfers, root.path(), caller).await;
    transfers
        .entries
        .lock()
        .unwrap()
        .entries
        .get_mut(&expired.stream)
        .unwrap()
        .expires = Instant::now() - Duration::from_secs(1);
    assert!(transfers.open(caller, expired.stream).is_err());
    released(&transfers).await;
    let expired = prepared(&transfers, root.path(), caller).await;
    transfers
        .entries
        .lock()
        .unwrap()
        .entries
        .get_mut(&expired.stream)
        .unwrap()
        .expires = Instant::now() - Duration::from_secs(1);
    transfers.expire(expired.stream);
    assert!(transfers.open(caller, expired.stream).is_err());
    released(&transfers).await;
    let removed = prepared(&transfers, root.path(), caller).await;
    let active = transfers.open(caller, removed.stream).unwrap();
    transfers.remove(root.path());
    released(&transfers).await;
    drop(active);
    let dropped = prepared(&transfers, root.path(), caller).await;
    drop(transfers.open(caller, dropped.stream).unwrap());
    released(&transfers).await;
    let _pending = prepared(&transfers, root.path(), caller).await;
    transfers.shutdown();
    released(&transfers).await;
    assert_eq!(
        transfers
            .download(
                caller,
                WorktreeId::new(),
                root.path().into(),
                "file".into(),
                CancellationToken::new()
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
}

#[tokio::test]
async fn releases_unused_capacity() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    std::fs::write(root.path().join("file"), []).unwrap();
    let transfers = Transfers::new(None);
    let caller = NodeId([1; 32]);
    for path in ["", "../outside", "missing", "file/child"] {
        assert!(
            transfers
                .download(
                    caller,
                    WorktreeId::new(),
                    root.path().into(),
                    path.into(),
                    CancellationToken::new()
                )
                .await
                .is_err()
        );
        released(&transfers).await;
    }
    let file = prepared(&transfers, root.path(), caller).await;
    assert_eq!(file.size, 0);
    assert_eq!(file.revision, blake3::hash(&[]).to_hex().to_string());
    let mut stream = transfers.open(caller, file.stream).unwrap();
    assert_eq!(stream.read(&mut [0]).await.unwrap(), 0);
    released(&transfers).await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        transfers
            .download(
                caller,
                WorktreeId::new(),
                root.path().into(),
                "file".into(),
                cancel
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    released(&transfers).await;
}
