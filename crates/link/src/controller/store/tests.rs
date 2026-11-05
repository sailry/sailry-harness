use super::*;

mod records;

#[test]
fn rejects_node_storage() {
    for filename in ["node.turso", "node.sqlite3"] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let (_, store) = Store::open(&profile).unwrap();
        drop(store);
        let identity = std::fs::read(profile.join("link.key")).unwrap();
        std::fs::create_dir(profile.join("storage")).unwrap();
        let database = profile.join("storage").join(filename);
        std::fs::write(&database, b"existing Node storage").unwrap();
        assert!(Store::open(&profile).is_err());
        assert_eq!(std::fs::read(profile.join("link.key")).unwrap(), identity);
        assert_eq!(std::fs::read(database).unwrap(), b"existing Node storage");
    }
}

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
    assert_eq!(store.peers().await.unwrap().len(), MAX_PEERS);
    store.close().await.unwrap();
    let (_, reopened) = Store::open(&profile).unwrap();
    assert!(store.set_trust(NodeId([0; 32]), false).await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    reopened.close().await.unwrap();
}

#[test]
fn rejects_relative_directory() {
    assert!(Store::open(Path::new("controller")).is_err());
}

#[test]
fn shares_profile_lock_with_node() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("controller");
    let (identity, store) = Store::open(&profile).unwrap();
    assert!(Store::open(&profile).is_err());
    drop(store);
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(profile.join("node.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(Store::open(&profile).is_err());
    drop(lock);
    assert_eq!(Store::open(&profile).unwrap().0.id(), identity.id());
}

#[test]
fn preserves_invalid_records() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("controller");
    let (_, store) = Store::open(&profile).unwrap();
    drop(store);
    let identity = std::fs::read(profile.join("link.key")).unwrap();
    let path = profile.join("peers.json");
    for bytes in [b"truncated".as_slice(), br#"{"version":2,"peers":[]}"#] {
        std::fs::write(&path, bytes).unwrap();
        assert!(Store::open(&profile).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(std::fs::read(profile.join("link.key")).unwrap(), identity);
    }
}

#[tokio::test]
async fn reload_preserves_identity_and_peers() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("controller");
    let (identity, store) = Store::open(&profile).unwrap();
    let address = EndpointAddr::new(iroh::SecretKey::generate().public())
        .with_ip_addr("127.0.0.1:1234".parse().unwrap());
    let id = NodeId(*address.id.as_bytes());
    assert_eq!(
        store.remember(address.clone()).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    store.set_trust(id, true).await.unwrap();
    store.remember(address.clone()).await.unwrap();
    let removed = NodeId([255; 32]);
    store.set_trust(removed, true).await.unwrap();
    store.set_trust(removed, false).await.unwrap();
    store.close().await.unwrap();
    let (reloaded, reopened) = Store::open(&profile).unwrap();
    assert_eq!(reloaded.id(), identity.id());
    assert_eq!(reopened.peers().await.unwrap(), vec![id]);
    assert_eq!(reopened.addresses().await.unwrap(), vec![address]);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn failed_publication_preserves_memory() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("controller");
    let (_, store) = Store::open(&profile).unwrap();
    let id = NodeId([1; 32]);
    store.set_trust(id, true).await.unwrap();
    let path = profile.join("peers.json");
    let before = std::fs::read(&path).unwrap();
    let saved = profile.join("saved.json");
    std::fs::rename(&path, &saved).unwrap();
    std::fs::create_dir(&path).unwrap();
    let marker = path.join("marker");
    std::fs::write(&marker, b"preserved data").unwrap();
    assert!(store.set_trust(NodeId([2; 32]), true).await.is_err());
    assert_eq!(store.peers().await.unwrap(), vec![id]);
    assert_eq!(std::fs::read(saved).unwrap(), before);
    assert_eq!(std::fs::read(marker).unwrap(), b"preserved data");
    assert_eq!(std::fs::read_dir(&profile).unwrap().count(), 4);
    store.close().await.unwrap();
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
