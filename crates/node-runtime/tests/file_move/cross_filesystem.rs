use super::*;
use std::{fs, os::unix::fs::MetadataExt, path::Path};

// Only recover this request's unique leaf; never enumerate the system Trash.
struct Entry {
    name: String,
    source: PathBuf,
    recycled: PathBuf,
    recovery: tempfile::TempDir,
}

impl Entry {
    fn new(root: &Path) -> Self {
        let name = format!("sailry-move-test-{}-文件", RequestId::new());
        let home = std::env::home_dir().unwrap();
        assert_eq!(
            fs::metadata(root).unwrap().dev(),
            fs::metadata(&home).unwrap().dev()
        );
        let recycled = home.join(".Trash").join(&name);
        assert_eq!(
            recycled.symlink_metadata().unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        Self {
            source: root.join(&name),
            name,
            recycled,
            recovery: tempfile::tempdir().unwrap(),
        }
    }

    fn recover(&self) -> PathBuf {
        let destination = self.recovery.path().join("entry");
        assert!(!destination.exists());
        fs::rename(&self.recycled, &destination).expect("recover isolated move fixture from Trash");
        destination
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        if self.recycled.symlink_metadata().is_ok() {
            let destination = self.recovery.path().join("entry");
            if !destination.exists()
                && let Err(error) = fs::rename(&self.recycled, destination)
            {
                eprintln!("isolated move fixture remains in Trash: {error}");
            }
        }
    }
}

fn destination_volume(source: &Path) -> tempfile::TempDir {
    let volume = std::env::var_os("SAILRY_TEST_VOLUME")
        .expect("set SAILRY_TEST_VOLUME to a writable directory on another filesystem");
    let destination = tempfile::Builder::new()
        .prefix("sailry-move-volume-test-")
        .tempdir_in(volume)
        .unwrap();
    assert_ne!(
        fs::metadata(source).unwrap().dev(),
        fs::metadata(destination.path()).unwrap().dev(),
        "the fixture must cross actual filesystems"
    );
    destination
}

#[tokio::test]
#[ignore = "requires SAILRY_TEST_VOLUME and recovers uniquely named fixtures from native Trash"]
async fn recovers_without_replay() {
    let fixture = Fixture::start().await;
    let destination = destination_volume(&fixture.root);
    let worktree = register(&fixture.clients()[0], destination.path()).await;
    for transport in [
        fixture.node.local(),
        fixture
            .controller
            .handle()
            .remote(fixture.node.link().address()),
    ] {
        let client = Client::new(transport.clone());
        for kind in 0..3 {
            let entry = Entry::new(&fixture.root);
            let target = destination.path().join(&entry.name);
            let bytes = if kind == 1 {
                vec![]
            } else {
                vec![0xff; 10 * 1024 * 1024]
            };
            let relative = if kind == 2 { "sub/文件.bin" } else { "" };
            if kind == 2 {
                fs::create_dir_all(entry.source.join("sub/empty")).unwrap();
                fs::create_dir(entry.source.join(".git")).unwrap();
                fs::write(entry.source.join(".git/config"), "retained metadata").unwrap();
                fs::write(entry.source.join("sub/empty-file"), []).unwrap();
            }
            let file = if relative.is_empty() {
                entry.source.clone()
            } else {
                entry.source.join(relative)
            };
            fs::write(&file, &bytes).unwrap();
            let before = fs::metadata(&file).unwrap();
            let mut source_watch = transport
                .subscribe(Topic::Files(fixture.worktree))
                .await
                .unwrap();
            let mut target_watch = transport.subscribe(Topic::Files(worktree)).await.unwrap();
            source_watch.next().await.unwrap();
            target_watch.next().await.unwrap();
            let request = client.prepare(move_to(
                fixture.worktree,
                worktree,
                &entry.name,
                &entry.name,
            ));
            let admission = client.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            let expected = Output::EntryMoved {
                from: entry.name.clone(),
                to: entry.name.clone(),
            };
            assert_eq!(admission.completion.await.unwrap().unwrap(), expected);
            assert!(!entry.source.exists());
            let copied_file = if relative.is_empty() {
                target.clone()
            } else {
                target.join(relative)
            };
            assert_eq!(fs::read(&copied_file).unwrap(), bytes);
            let after = fs::metadata(&copied_file).unwrap();
            assert_eq!(before.modified().unwrap(), after.modified().unwrap());
            assert_eq!(before.permissions(), after.permissions());
            if kind == 2 {
                assert!(target.join("sub/empty").is_dir());
                assert!(fs::read(target.join("sub/empty-file")).unwrap().is_empty());
                assert_eq!(
                    fs::read_to_string(target.join(".git/config")).unwrap(),
                    "retained metadata"
                );
            }
            let recovered = entry.recover();
            let recovered_file = if relative.is_empty() {
                recovered
            } else {
                recovered.join(relative)
            };
            assert_eq!(fs::read(recovered_file).unwrap(), bytes);
            for (watch, worktree) in [
                (&mut source_watch, fixture.worktree),
                (&mut target_watch, worktree),
            ] {
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(5), watch.next())
                        .await
                        .unwrap()
                        .unwrap(),
                    Update::FilesChanged {
                        node: fixture.node.id(),
                        worktree
                    }
                );
            }
            drop(source_watch);
            drop(target_watch);
            fs::write(&entry.source, "replacement").unwrap();
            fs::write(&copied_file, "external edit").unwrap();
            assert_eq!(client.execute(request).await.unwrap(), expected);
            assert_eq!(
                client
                    .execute(client.prepare(move_to(
                        fixture.worktree,
                        worktree,
                        &entry.name,
                        &entry.name
                    )))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            assert_eq!(fs::read_to_string(&entry.source).unwrap(), "replacement");
            assert_eq!(fs::read_to_string(copied_file).unwrap(), "external edit");
            assert!(entry.recycled.symlink_metadata().is_err());
        }
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "requires SAILRY_TEST_VOLUME and recovers uniquely named fixtures from native Trash"]
async fn preserves_uncertain_replacements() {
    let fixture = Fixture::start().await;
    let destination = destination_volume(&fixture.root);
    let worktree = register(&fixture.clients()[0], destination.path()).await;
    let profile = fixture._temp.path().join("node");
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let mut requests = Vec::new();
    for client in fixture.clients() {
        let entry = Entry::new(&fixture.root);
        fs::write(&entry.source, "content").unwrap();
        let request = client.prepare(move_to(
            fixture.worktree,
            worktree,
            &entry.name,
            &entry.name,
        ));
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(!entry.source.exists());
        assert_eq!(fs::read_to_string(entry.recover()).unwrap(), "content");
        assert_eq!(
            fs::read_to_string(destination.path().join(&entry.name)).unwrap(),
            "content"
        );
        fs::write(&entry.source, "replacement").unwrap();
        fs::write(destination.path().join(&entry.name), "external edit").unwrap();
        requests.push((request, entry));
    }
    db.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(db);
    fixture.node.shutdown().await.unwrap();
    let node = Node::start(profile).await.unwrap();
    for ((request, entry), client) in requests.into_iter().zip([
        Client::new(node.local()),
        Client::new(fixture.controller.handle().remote(node.link().address())),
    ]) {
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(fs::read_to_string(&entry.source).unwrap(), "replacement");
        assert_eq!(
            fs::read_to_string(destination.path().join(&entry.name)).unwrap(),
            "external edit"
        );
        assert!(entry.recycled.symlink_metadata().is_err());
    }
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
