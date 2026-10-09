use super::*;

#[tokio::test]
async fn uses_dedicated_revision_checked_actions() {
    for remote in [false, true] {
        let revision = blake3::hash(b"Keep content").to_hex().to_string();
        let server = Server::tools(vec![
            (name("create_directory"), json!({"path":"folder"})),
            (name("create_directory"), json!({"path":"folder"})),
            (name("move_file"), json!({"path":"note.txt","destination":"occupied.txt","expected_revision":revision})),
            (name("move_file"), json!({"path":"note.txt","destination":"folder/moved.txt","expected_revision":blake3::hash(b"stale").to_hex().to_string()})),
            (name("move_file"), json!({"path":"note.txt","destination":"folder/moved.txt","expected_revision":revision})),
            (name("delete_file"), json!({"path":"folder/moved.txt","expected_revision":blake3::hash(b"stale").to_hex().to_string()})),
            (name("read_file"), json!({"path":"folder/moved.txt"})),
            (name("delete_file"), json!({"path":"folder/moved.txt","expected_revision":revision})),
        ]).await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        install(&fixture).await;
        std::fs::write(fixture.root.join("note.txt"), "Keep content").unwrap();
        std::fs::write(fixture.root.join("occupied.txt"), "Preserve destination").unwrap();
        configure(&mut fixture, Permission::Project, WorkMode::Code).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        let output = results(&page);
        assert_eq!(output.len(), 8, "{output:?}");
        assert_eq!(output[0]["kind"], "directory_created");
        assert_eq!(output[1]["kind"], "directory_created");
        assert_eq!(output[2]["error"]["code"], "conflict");
        assert_eq!(output[3]["error"]["code"], "revision_conflict");
        assert_eq!(output[4]["kind"], "entry_renamed");
        assert_eq!(output[5]["error"]["code"], "revision_conflict");
        assert_eq!(output[6]["data"]["lines"][0]["text"], "Keep content");
        assert_eq!(output[7]["kind"], "entry_trashed");
        assert!(fixture.root.join("folder").is_dir());
        assert!(!fixture.root.join("note.txt").exists());
        assert!(!fixture.root.join("folder/moved.txt").exists());
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("occupied.txt")).unwrap(),
            "Preserve destination"
        );
        assert!(
            page.approvals
                .iter()
                .all(|approval| approval.source == ApprovalSource::Project)
        );
        assert!(
            checkpoints(&fixture.client, fixture.session.id, turn.id)
                .await
                .is_empty()
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn asks_before_deleting_a_file() {
    for remote in [false, true] {
        let server = Server::tools(vec![(name("delete_file"), json!({
            "path":"note.txt", "expected_revision":blake3::hash(b"Original").to_hex().to_string()
        }))]).await;
        let fixture = process::Fixture::new(remote, &server).await;
        install(&fixture).await;
        std::fs::write(fixture.root.join("note.txt"), "Original").unwrap();
        let turn = submit(&fixture).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            "Original"
        );
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            "Original"
        );
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(page.approvals[0].state, ApprovalState::Denied);
        let output = results(&page);
        assert_eq!(output.len(), 1);
        assert_eq!(
            output[0]["error"],
            format!(
                "Tool '{}' execution denied by confirmation policy",
                name("delete_file")
            )
        );
        assert!(
            checkpoints(&fixture.client, fixture.session.id, turn.id)
                .await
                .is_empty()
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
