use super::*;

#[test]
fn copies_only_the_closed_family() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("node.turso");
    let bodies = ["database", "logical log", "write log", "shared memory"];
    for (path, body) in crate::profile::database_files(&source)
        .into_iter()
        .zip(bodies)
    {
        std::fs::write(path, body).unwrap();
    }
    std::fs::write(directory.path().join("node.turso-log"), "unrelated file").unwrap();
    std::fs::write(directory.path().join("node.sqlite3"), "old database").unwrap();
    let snapshot = preview(&source).unwrap();
    for (path, body) in crate::profile::database_files(snapshot.path())
        .into_iter()
        .zip(bodies)
    {
        assert_eq!(std::fs::read_to_string(path).unwrap(), body);
    }
    assert!(!snapshot.directory.path().join("node.turso-log").exists());
    assert!(!snapshot.directory.path().join("node.sqlite3").exists());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("node.turso-log")).unwrap(),
        "unrelated file"
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("node.sqlite3")).unwrap(),
        "old database"
    );
    let root = snapshot.directory.path().to_owned();
    drop(snapshot);
    assert!(!root.exists());
}

#[test]
fn preserves_backup_and_neighbors() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("node.turso");
    let bodies = ["database", "logical log", "write log", "shared memory"];
    for (path, body) in crate::profile::database_files(&source)
        .into_iter()
        .zip(bodies)
    {
        std::fs::write(path, body).unwrap();
    }
    let neighbor = directory.path().join("knowledge.turso");
    std::fs::write(&neighbor, "unrelated database").unwrap();
    let saved = save(&source).unwrap();
    clear(&source).unwrap();
    assert!(
        crate::profile::database_files(&source)
            .iter()
            .all(|path| !path.exists())
    );
    assert_eq!(
        std::fs::read_to_string(&neighbor).unwrap(),
        "unrelated database"
    );
    std::fs::write(&source, "partial replacement").unwrap();
    clear(&source).unwrap();
    copy(&saved, &source).unwrap();
    for path in [&source, &saved] {
        for (member, body) in crate::profile::database_files(path).into_iter().zip(bodies) {
            assert_eq!(std::fs::read_to_string(member).unwrap(), body);
        }
    }
}

#[test]
fn maps_each_destination_member() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("node.turso");
    let target = directory.path().join("renamed.turso");
    let bodies = ["database", "logical log", "write log", "shared memory"];
    for (path, body) in crate::profile::database_files(&source)
        .into_iter()
        .zip(bodies)
    {
        std::fs::write(path, body).unwrap();
    }
    copy(&source, &target).unwrap();
    for (path, body) in crate::profile::database_files(&target)
        .into_iter()
        .zip(bodies)
    {
        assert_eq!(std::fs::read_to_string(path).unwrap(), body);
    }
    assert!(directory.path().join("renamed.db-log").is_file());
    assert!(!directory.path().join("renamed.turso-log").exists());
}

#[tokio::test]
async fn opens_snapshots_read_only() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("node.turso");
    let handle = crate::store::database::Handle::new(
        source.clone(),
        crate::store::database::engine(&source).await.unwrap(),
    );
    let connection = handle.lease(false).await.unwrap();
    connection
        .execute_batch("BEGIN; CREATE TABLE records(value BLOB NOT NULL); INSERT INTO records VALUES(X'007FFF'); COMMIT")
        .await
        .unwrap();
    drop(connection);
    drop(handle.pause().await.unwrap());
    let snapshot = preview(&source).unwrap();
    assert!(snapshot.directory.path().join("node.db-log").is_file());
    let database = turso::Builder::new_local(snapshot.path().to_str().unwrap())
        .experimental_index_method(true)
        .read_only(true)
        .build()
        .await
        .unwrap();
    let connection = database.connect().unwrap();
    assert_eq!(
        connection
            .prepare("SELECT value FROM records")
            .await
            .unwrap()
            .query_row(())
            .await
            .unwrap()
            .get::<Vec<u8>>(0)
            .unwrap(),
        vec![0, 127, 255]
    );
}

#[test]
fn publishes_a_private_backup() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("node.turso");
    std::fs::write(&source, "source bytes").unwrap();
    std::fs::write(&crate::profile::database_files(&source)[1], b"").unwrap();
    let saved = save(&source).unwrap();
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), "source bytes");
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "source bytes");
    assert!(
        std::fs::read(&crate::profile::database_files(&saved)[1])
            .unwrap()
            .is_empty()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            saved.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            saved
                .parent()
                .unwrap()
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn refuses_existing_destinations() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("node.turso");
    let target = directory.path().join("target.turso");
    std::fs::write(&source, "source").unwrap();
    std::fs::write(&crate::profile::database_files(&source)[1], b"").unwrap();
    std::fs::write(&target, "target").unwrap();
    assert!(copy(&source, &target).is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "target");
}

#[test]
fn refuses_missing_logical_logs() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("node.turso");
    std::fs::write(&source, "source bytes").unwrap();
    assert!(preview(&source).is_err());
    assert!(save(&source).is_err());
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "source bytes");
    assert!(!crate::profile::database_files(&source)[1].exists());
}

#[cfg(unix)]
#[test]
fn refuses_links_in_the_family() {
    use std::os::unix::fs::symlink;
    for index in 0..4 {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("node.turso");
        let outside = directory.path().join("outside");
        std::fs::write(&outside, "private marker").unwrap();
        for (member, path) in crate::profile::database_files(&source)
            .into_iter()
            .take(2)
            .enumerate()
        {
            if member != index {
                std::fs::write(path, "source").unwrap();
            }
        }
        symlink(&outside, &crate::profile::database_files(&source)[index]).unwrap();
        assert!(preview(&source).is_err());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "private marker");
    }
}
