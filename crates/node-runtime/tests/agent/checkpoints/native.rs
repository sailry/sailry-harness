use super::{fixture::Fixture, *};
use crate::trash_fixture::Entry;

#[tokio::test]
#[ignore = "moves uniquely named checkpoint fixtures into native Trash and recovers them"]
async fn preserves_replacements() {
    for remote in [false, true] {
        let names = [
            format!("sailry-checkpoint-{}-文件", RequestId::new()),
            format!("sailry-checkpoint-{}-empty", RequestId::new()),
        ];
        let fixture = Fixture::start(
            remote,
            &[(&names[0], None, "Created 中文 🙂"), (&names[1], None, "")],
        )
        .await;
        let client = &fixture.client;
        let before = fixture.counts();
        let mut records = Vec::new();
        let mut watch = fixture
            .transport
            .subscribe(Topic::Files(fixture.session.worktree))
            .await
            .unwrap();
        watch.next().await.unwrap();
        for (index, name) in names.into_iter().enumerate() {
            let entry = Entry::new(&fixture.root, name);
            let file = fixture
                .files
                .iter()
                .find(|file| file.path == entry.name)
                .unwrap();
            let content = read(client, fixture.session.id, file.id).await;
            let request = fixture.restore(file);
            let db = fixture.database();
            if index == 1 {
                db.execute_batch("CREATE TRIGGER lose_restore BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
            }
            let result = client.execute(request.clone()).await;
            if index == 1 {
                assert_eq!(result.as_ref().unwrap_err().code, ErrorCode::OutcomeUnknown);
                db.execute_batch("DROP TRIGGER lose_restore").unwrap();
            } else {
                assert_eq!(
                    result.as_ref().unwrap(),
                    &Output::FileRestored(checkpoint::Restored {
                        session: fixture.session.id,
                        checkpoint: file.id,
                        path: entry.name.clone(),
                        revision: None,
                    })
                );
            }
            drop(db);
            assert!(!entry.source.exists());
            assert_eq!(
                std::fs::read_to_string(entry.recover()).unwrap(),
                content.after
            );
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(5), watch.next())
                    .await
                    .unwrap()
                    .unwrap(),
                Update::FilesChanged { .. }
            ));
            std::fs::write(&entry.source, "replacement").unwrap();
            let retry = client.execute(request.clone()).await;
            if index == 1 {
                assert_eq!(retry.unwrap_err().code, ErrorCode::OutcomeUnknown);
            } else {
                assert_eq!(retry, result);
            }
            assert_eq!(
                std::fs::read_to_string(&entry.source).unwrap(),
                "replacement"
            );
            records.push((request, result, entry));
        }
        assert_eq!(fixture.counts(), before);
        assert_eq!(history(client, fixture.session.id).await, fixture.history);
        drop(watch);
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(fixture.directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        for (request, result, entry) in records {
            let retry = client.execute(request).await;
            match result {
                Ok(result) => assert_eq!(retry.unwrap(), result),
                Err(_) => assert_eq!(retry.unwrap_err().code, ErrorCode::OutcomeUnknown),
            }
            assert_eq!(
                std::fs::read_to_string(&entry.source).unwrap(),
                "replacement"
            );
            assert!(entry.recycled.symlink_metadata().is_err());
        }
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        assert_eq!(history(&client, fixture.session.id).await, fixture.history);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
