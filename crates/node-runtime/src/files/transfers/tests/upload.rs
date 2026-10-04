use super::*;
use sailry_protocol::{FILE_UPLOAD_READY, FileUpload, FileUploadSpec};

async fn prepare(
    transfers: &Arc<Transfers>,
    root: &std::path::Path,
    caller: NodeId,
    path: &str,
    data: &[u8],
) -> FileUpload {
    transfers
        .upload(
            caller,
            root.into(),
            FileUploadSpec {
                worktree: WorktreeId::new(),
                path: path.into(),
                size: data.len() as u64,
                revision: blake3::hash(data).to_hex().to_string(),
                expected_revision: None,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap()
}

async fn send(transfers: &Arc<Transfers>, caller: NodeId, upload: &FileUpload, data: &[u8]) {
    let mut stream = transfers.open(caller, upload.stream).unwrap();
    stream.write_all(data).await.unwrap();
    stream.shutdown().await.unwrap();
    assert_eq!(stream.read_u8().await.unwrap(), FILE_UPLOAD_READY);
    assert_eq!(stream.read(&mut [0]).await.unwrap(), 0);
}

#[tokio::test]
async fn confines_unpublished_staging() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let transfers = Transfers::new(None);
    let caller = NodeId([1; 32]);
    let other = NodeId([2; 32]);
    let data = b"verified bytes";
    let upload = prepare(&transfers, root.path(), caller, "file", data).await;
    assert!(!root.path().join("file").exists());
    assert!(transfers.open(other, upload.stream).is_err());
    assert!(
        transfers
            .commit(
                caller,
                root.path(),
                upload.spec.worktree,
                "file",
                upload.stream
            )
            .is_err()
    );
    send(&transfers, caller, &upload, data).await;
    assert!(!root.path().join("file").exists());
    assert!(transfers.open(caller, upload.stream).is_err());
    for (caller, worktree, path) in [
        (other, upload.spec.worktree, "file"),
        (caller, WorktreeId::new(), "file"),
        (caller, upload.spec.worktree, "other"),
    ] {
        assert_eq!(
            transfers
                .commit(caller, root.path(), worktree, path, upload.stream)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
    assert_eq!(
        transfers.cancel(other, upload.stream).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        transfers
            .commit(
                caller,
                root.path(),
                upload.spec.worktree,
                "file",
                upload.stream
            )
            .unwrap()
            .size,
        data.len() as u64
    );
    assert_eq!(std::fs::read(root.path().join("file")).unwrap(), data);
    released(&transfers).await;
}

#[tokio::test]
async fn releases_malformed_streams() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let transfers = Transfers::new(None);
    let caller = NodeId([1; 32]);
    for data in [b"sh".as_slice(), b"other", b"too long"] {
        let upload = prepare(&transfers, root.path(), caller, "file", b"valid").await;
        let mut stream = transfers.open(caller, upload.stream).unwrap();
        stream.write_all(data).await.unwrap();
        stream.shutdown().await.unwrap();
        assert_eq!(stream.read(&mut [0]).await.unwrap(), 0);
        released(&transfers).await;
        assert!(!root.path().join("file").exists());
        assert!(
            transfers
                .commit(
                    caller,
                    root.path(),
                    upload.spec.worktree,
                    "file",
                    upload.stream
                )
                .is_err()
        );
    }
}

#[tokio::test]
async fn extends_staged_expiry() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let transfers = Transfers::new(None);
    let caller = NodeId([1; 32]);
    let upload = prepare(&transfers, root.path(), caller, "file", &[]).await;
    send(&transfers, caller, &upload, &[]).await;
    transfers.expire(upload.stream);
    assert!(
        transfers
            .entries
            .lock()
            .unwrap()
            .entries
            .contains_key(&upload.stream)
    );
    transfers
        .entries
        .lock()
        .unwrap()
        .entries
        .get_mut(&upload.stream)
        .unwrap()
        .expires = Instant::now() - Duration::from_secs(1);
    transfers.expire(upload.stream);
    released(&transfers).await;
    assert!(!root.path().join("file").exists());
    let upload = prepare(&transfers, root.path(), caller, "file", &[]).await;
    send(&transfers, caller, &upload, &[]).await;
    transfers
        .entries
        .lock()
        .unwrap()
        .entries
        .get_mut(&upload.stream)
        .unwrap()
        .expires = Instant::now() - Duration::from_secs(1);
    assert!(
        transfers
            .commit(
                caller,
                root.path(),
                upload.spec.worktree,
                "file",
                upload.stream
            )
            .is_err()
    );
    released(&transfers).await;
}

#[tokio::test]
async fn shares_transfer_limits() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    std::fs::write(root.path().join("file"), []).unwrap();
    let transfers = Transfers::new(None);
    let caller = NodeId([1; 32]);
    for index in 0..PER_CALLER {
        prepare(&transfers, root.path(), caller, &format!("{index}"), &[]).await;
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
    for _ in 0..PER_CALLER {
        prepared(&transfers, root.path(), NodeId([2; 32])).await;
    }
    assert_eq!(transfers.capacity.available_permits(), 0);
    transfers.shutdown();
    released(&transfers).await;
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn releases_cancelled_phases() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let transfers = Transfers::new(None);
    let caller = NodeId([1; 32]);
    for phase in ["pending", "active", "uploaded"] {
        let upload = prepare(&transfers, root.path(), caller, "file", b"content").await;
        let active = if phase == "active" {
            Some(transfers.open(caller, upload.stream).unwrap())
        } else {
            None
        };
        if phase == "uploaded" {
            send(&transfers, caller, &upload, b"content").await;
        }
        transfers.remove(root.path());
        drop(active);
        released(&transfers).await;
        assert!(!root.path().join("file").exists());
    }
    let closed = CancellationToken::new();
    let upload = transfers
        .upload(
            caller,
            root.path().into(),
            FileUploadSpec {
                worktree: WorktreeId::new(),
                path: "file".into(),
                size: 0,
                revision: blake3::hash(&[]).to_hex().to_string(),
                expected_revision: None,
            },
            closed.clone(),
        )
        .await
        .unwrap();
    closed.cancel();
    released(&transfers).await;
    assert!(transfers.open(caller, upload.stream).is_err());
}
