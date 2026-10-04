use std::time::Duration;

fn tempdir() -> std::io::Result<tempfile::TempDir> {
    let directory = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(directory)
}

use super::{Error, Node, State};

#[tokio::test]
async fn enforces_single_owner() {
    let directory = tempdir().unwrap();
    let node = Node::start(directory.path()).await.unwrap();
    assert!(matches!(
        Node::start(directory.path()).await,
        Err(Error::ProfileInUse(_))
    ));
    node.shutdown().await.unwrap();
    Node::start(directory.path())
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
}

#[tokio::test]
async fn isolates_profiles() {
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    let first = Node::start(first.path()).await.unwrap();
    let second = Node::start(second.path()).await.unwrap();
    first.shutdown().await.unwrap();
    second.observe().probe().await.unwrap();
    second.shutdown().await.unwrap();
}

#[tokio::test]
async fn survives_observer_disconnect() {
    let directory = tempdir().unwrap();
    let node = Node::start(directory.path()).await.unwrap();
    let observer = node.observe();
    observer.probe().await.unwrap();
    drop(observer);
    node.observe().probe().await.unwrap();
    assert!(matches!(
        Node::start(directory.path()).await,
        Err(Error::ProfileInUse(_))
    ));
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn startup_failure_releases_lock() {
    let directory = tempdir().unwrap();
    let obstacle = directory.path().join("storage");
    std::fs::write(&obstacle, "startup fault").unwrap();
    assert!(matches!(
        Node::start(directory.path()).await,
        Err(Error::Io(_))
    ));
    assert!(directory.path().join("node.lock").exists());
    std::fs::remove_file(&obstacle).unwrap();
    Node::start(directory.path())
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
}

#[tokio::test]
async fn invalidates_previous_observers() {
    let directory = tempdir().unwrap();
    let node = Node::start(directory.path()).await.unwrap();
    let old = node.observe();
    node.shutdown().await.unwrap();
    assert_eq!(old.state(), State::Stopped);
    let node = Node::start(directory.path()).await.unwrap();
    assert!(matches!(old.probe().await, Err(Error::Stopped)));
    node.observe().probe().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn dropping_owner_cleans_up() {
    let directory = tempdir().unwrap();
    let node = Node::start(directory.path()).await.unwrap();
    let mut observer = node.observe();
    drop(node);
    tokio::time::timeout(Duration::from_secs(1), async {
        while observer.state() != State::Stopped {
            observer.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    Node::start(directory.path())
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelled_shutdown_retains_lock() {
    let directory = tempdir().unwrap();
    let node = Node::start(directory.path()).await.unwrap();
    let mut observer = node.observe();
    let (release, released) = tokio::sync::oneshot::channel();
    let result = observer
        .tasks
        .spawn(async move {
            released.await.unwrap();
        })
        .unwrap();
    let shutdown = tokio::spawn(node.shutdown());
    tokio::time::timeout(Duration::from_secs(1), async {
        while observer.state() != State::Stopping {
            observer.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    shutdown.abort();
    let _ = shutdown.await;
    assert!(matches!(
        Node::start(directory.path()).await,
        Err(Error::ProfileInUse(_))
    ));
    release.send(()).unwrap();
    result.await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while observer.state() != State::Stopped {
            observer.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    Node::start(directory.path())
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
}

#[tokio::test]
async fn rejects_relative_profile() {
    assert!(matches!(
        Node::start("relative-profile").await,
        Err(Error::InvalidProfile { .. })
    ));
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[tokio::test]
    async fn rejects_insecure_directory() {
        let directory = tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            Node::start(directory.path()).await,
            Err(Error::InvalidProfile { .. })
        ));
        assert_eq!(
            std::fs::metadata(directory.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert!(!directory.path().join("node.lock").exists());
    }

    #[tokio::test]
    async fn rejects_lock_symlink() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("preserved.txt");
        std::fs::write(&target, "preserve").unwrap();
        symlink(&target, directory.path().join("node.lock")).unwrap();
        assert!(matches!(
            Node::start(directory.path()).await,
            Err(Error::InvalidProfile { .. })
        ));
        assert_eq!(std::fs::read_to_string(target).unwrap(), "preserve");
    }

    #[tokio::test]
    async fn rejects_profile_symlink() {
        let directory = tempdir().unwrap();
        let link = directory.path().join("alias");
        symlink(directory.path(), &link).unwrap();
        assert!(matches!(
            Node::start(link).await,
            Err(Error::InvalidProfile { .. })
        ));
    }

    #[tokio::test]
    async fn rejects_database_symlink() {
        let directory = tempdir().unwrap();
        let profile = directory.path().join("profile");
        let node = Node::start(&profile).await.unwrap();
        node.shutdown().await.unwrap();
        let database = profile.join("storage/node.sqlite3");
        std::fs::remove_file(&database).unwrap();
        let preserved = directory.path().join("preserved.db");
        std::fs::write(&preserved, "preserve").unwrap();
        symlink(&preserved, database).unwrap();
        assert!(Node::start(&profile).await.is_err());
        assert_eq!(std::fs::read_to_string(preserved).unwrap(), "preserve");
    }
}

#[tokio::test]
async fn bind_failure_releases_ownership() {
    let directory = tempdir().unwrap();
    let occupied = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let scope = crate::NetworkScope::Direct(occupied.local_addr().unwrap());
    assert!(
        Node::start_with_network(directory.path(), scope)
            .await
            .is_err()
    );
    drop(occupied);
    Node::start(directory.path())
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
}
