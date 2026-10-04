use super::*;

#[tokio::test]
async fn failed_writes_preserve_disk() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("controller");
    let (_, store) = Store::open(&profile).unwrap();
    store
        .update(|peers| {
            *peers = (0..MAX_PEERS)
                .map(|index| Peer {
                    id: NodeId([index as u8; 32]),
                    address: None,
                })
                .collect();
            Ok(())
        })
        .await
        .unwrap();
    let path = profile.join("peers.json");
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        store
            .set_trust(NodeId([255; 32]), true)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    store.close().await.unwrap();
    let (_, reopened) = Store::open(&profile).unwrap();
    assert!(store.set_trust(NodeId([0; 32]), false).await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    reopened.close().await.unwrap();
}

#[cfg(unix)]
#[test]
fn enforces_private_storage() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("profile");
    let (_, store) = Store::open(&profile).unwrap();
    drop(store);
    let path = profile.join("peers.json");
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o077, 0);
    let saved = directory.path().join("saved.json");
    std::fs::rename(&path, &saved).unwrap();
    symlink(&saved, &path).unwrap();
    assert!(Store::open(&profile).is_err());
    assert_eq!(
        std::fs::read(&saved).unwrap(),
        br#"{"version":1,"peers":[]}"#
    );
}
