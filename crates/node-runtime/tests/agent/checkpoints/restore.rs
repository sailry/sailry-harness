use super::{fixture::Fixture, *};

#[tokio::test]
async fn restores_captured_versions() {
    for remote in [false, true] {
        let original = "Before 中文 🙂\n".repeat(4000);
        let fixture = Fixture::start(
            remote,
            &[
                ("资料.txt", Some(&original), "first"),
                ("资料.txt", Some("first"), "second"),
                ("lost.txt", Some("lost before"), "lost after"),
            ],
        )
        .await;
        let client = &fixture.client;
        let files = &fixture.files;
        let counts = fixture.counts();
        let mut watch = fixture
            .transport
            .subscribe(Topic::Files(fixture.session.worktree))
            .await
            .unwrap();
        watch.next().await.unwrap();
        // The older checkpoint cannot overwrite a later managed or external change.
        let stale = fixture.restore(&files[2]);
        assert_eq!(
            client.execute(stale.clone()).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("资料.txt")).unwrap(),
            "second"
        );
        let request = fixture.restore(&files[1]);
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        // Drop the application completion, then recover only through the original receipt.
        drop(admission.completion);
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&request).await.unwrap() {
                    break result.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let Output::FileRestored(restored) = &result else {
            panic!("restore expected")
        };
        assert_eq!(restored.session, fixture.session.id);
        assert_eq!(restored.checkpoint, files[1].id);
        assert_eq!(
            restored.revision.as_ref(),
            Some(&files[1].before.as_ref().unwrap().revision)
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("资料.txt")).unwrap(),
            "first"
        );
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(5), watch.next())
                .await
                .unwrap()
                .unwrap(),
            Update::FilesChanged { .. }
        ));
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Ok(result.clone())))
        );
        assert_eq!(client.execute(request.clone()).await.unwrap(), result);
        // An already rejected request keeps its original result; retrying is a new choice.
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        client.execute(fixture.restore(&files[2])).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("资料.txt")).unwrap(),
            original
        );
        std::fs::write(fixture.root.join("资料.txt"), "later external edit").unwrap();
        assert_eq!(client.execute(request.clone()).await.unwrap(), result);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("资料.txt")).unwrap(),
            "later external edit"
        );
        assert_eq!(history(client, fixture.session.id).await, fixture.history);
        assert_eq!(
            list(client, fixture.session.id, fixture.turn, None, 100)
                .await
                .files,
            *files
        );
        assert_eq!(fixture.counts(), counts);

        let db = fixture.database();
        db.execute_batch("CREATE TRIGGER lose_restore BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
        let lost = fixture.restore(&files[0]);
        assert_eq!(
            client.execute(lost.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("lost.txt")).unwrap(),
            "lost before"
        );
        db.execute_batch("DROP TRIGGER lose_restore").unwrap();
        drop(db);
        std::fs::write(fixture.root.join("lost.txt"), "replacement").unwrap();
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
        assert_eq!(
            client.execute(lost.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            client.outcome(&lost).await.unwrap(),
            RequestOutcome::Unknown
        );
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("lost.txt")).unwrap(),
            "replacement"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("资料.txt")).unwrap(),
            "later external edit"
        );
        assert_eq!(history(&client, fixture.session.id).await, fixture.history);
        assert_eq!(
            list(&client, fixture.session.id, fixture.turn, None, 100)
                .await
                .files,
            *files
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 4);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
