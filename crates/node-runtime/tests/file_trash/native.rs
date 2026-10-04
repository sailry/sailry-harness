use super::*;
use std::{path::PathBuf, time::Duration};

// Each fixture has a unique leaf on the same volume as the user's Trash. Only
// that exact leaf is recovered; never enumerate or purge the user's Trash.
struct Entry {
    path: PathBuf,
    recycled: PathBuf,
    name: String,
}

impl Entry {
    fn new(root: &Path) -> Self {
        let name = format!("sailry-trash-test-{}-文件", RequestId::new());
        let recycled = std::env::home_dir().unwrap().join(".Trash").join(&name);
        assert_eq!(
            recycled.symlink_metadata().unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        Self {
            path: root.join(&name),
            recycled,
            name,
        }
    }

    fn recover(&self, destination: &Path) {
        assert!(!destination.exists());
        std::fs::rename(&self.recycled, destination).expect("recover isolated fixture from Trash");
    }
}

#[tokio::test]
#[ignore = "moves uniquely named test fixtures into the system Trash and recovers them"]
async fn shares_trash_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let local = Client::new(node.local());
    let worktree = register(&local, &project).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address.clone()));
    for (index, client) in [&local, &remote].into_iter().enumerate() {
        let entry = Entry::new(&project);
        std::fs::create_dir(&entry.path).unwrap();
        std::fs::write(entry.path.join("内容"), "preserved").unwrap();
        let transport = if index == 0 {
            node.local()
        } else {
            controller.handle().remote(address.clone())
        };
        let mut watch = transport.subscribe(Topic::Files(worktree)).await.unwrap();
        watch.next().await.unwrap();
        let request = client.prepare(command(worktree, &entry.name));
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let result = admission.completion.await.unwrap().unwrap();
        assert_eq!(
            result,
            Output::EntryTrashed {
                path: entry.name.clone()
            }
        );
        assert!(!entry.path.exists());
        let recovered = temp.path().join(format!("recovered-{index}"));
        entry.recover(&recovered);
        assert_eq!(
            std::fs::read_to_string(recovered.join("内容")).unwrap(),
            "preserved"
        );
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(5), watch.next())
                .await
                .unwrap()
                .unwrap(),
            Update::FilesChanged { .. }
        ));
        std::fs::write(&entry.path, "replacement").unwrap();
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert_eq!(std::fs::read_to_string(&entry.path).unwrap(), "replacement");
        drop(watch);

        let link = Entry::new(&project);
        std::os::unix::fs::symlink(&entry.path, &link.path).unwrap();
        client
            .execute(client.prepare(command(worktree, &link.name)))
            .await
            .unwrap();
        let restored = temp.path().join(format!("recovered-link-{index}"));
        link.recover(&restored);
        assert_eq!(std::fs::read_link(restored).unwrap(), entry.path);
        assert_eq!(std::fs::read_to_string(&entry.path).unwrap(), "replacement");
    }

    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let mut requests = Vec::new();
    for (index, client) in [&local, &remote].into_iter().enumerate() {
        let entry = Entry::new(&project);
        std::fs::write(&entry.path, "preserved").unwrap();
        let request = client.prepare(command(worktree, &entry.name));
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(!entry.path.exists());
        let recovered = temp.path().join(format!("recovered-lost-{index}"));
        entry.recover(&recovered);
        assert_eq!(std::fs::read_to_string(recovered).unwrap(), "preserved");
        std::fs::write(&entry.path, "replacement").unwrap();
        requests.push((request, entry));
    }
    db.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(db);
    node.shutdown().await.unwrap();
    let node = Node::start(&profile).await.unwrap();
    let local = Client::new(node.local());
    let remote = Client::new(controller.handle().remote(node.link().address()));
    for ((request, entry), client) in requests.into_iter().zip([local, remote]) {
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(std::fs::read_to_string(entry.path).unwrap(), "replacement");
        assert!(entry.recycled.symlink_metadata().is_err());
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
