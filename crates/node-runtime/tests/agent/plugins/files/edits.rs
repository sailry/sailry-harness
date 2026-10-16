use super::*;

#[tokio::test]
async fn captures_one_checkpoint_for_batch() {
    for remote in [false, true] {
        let original = "Header\r\nRepeat Repeat\r\nTail\r\n";
        let updated = "Tail\r\n中文 中文\r\nRepeat\r\n";
        let server = Server::tools(vec![(name("edit_file"), json!({
            "path":"note.txt", "expected_revision":blake3::hash(original.as_bytes()).to_hex().to_string(),
            "edits":[{"old_text":"Tail","new_text":"Repeat"},
                {"old_text":"Repeat","new_text":"中文","replace_all":true},
                {"old_text":"Header","new_text":"Tail"}]
        }))]).await;
        let mut fixture = process::Fixture::packages(remote, &server, &[]).await;
        install(&fixture).await;
        std::fs::write(fixture.root.join("note.txt"), original).unwrap();
        configure(&mut fixture, Permission::Project, WorkMode::Code).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(page.approvals[0].source, ApprovalSource::Project);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            updated
        );
        let files = checkpoints(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(files.len(), 1);
        let Output::FileCheckpoint(content) = execute(
            &fixture.client,
            Command::ReadFileCheckpoint {
                session: fixture.session.id,
                checkpoint: files[0].id,
            },
        )
        .await
        else {
            panic!("checkpoint content expected");
        };
        assert_eq!(content.before.as_deref(), Some(original));
        assert_eq!(content.after, updated);
        let Output::TurnDiff(diff) = execute(
            &fixture.client,
            Command::ReadTurnDiff {
                session: fixture.session.id,
                turn: turn.id,
            },
        )
        .await
        else {
            panic!("turn diff expected");
        };
        assert!(!diff.partial);
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].path, "note.txt");
        assert!(diff.files[0].text.contains("+中文 中文"));
        assert!(diff.files[0].text.contains("-Repeat Repeat"));
        execute(
            &fixture.client,
            Command::RestoreFileCheckpoint {
                session: fixture.session.id,
                checkpoint: files[0].id,
                worktree: fixture.session.worktree,
            },
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            original
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_batches_without_partial_writes() {
    for remote in [false, true] {
        let original = "First Repeat Repeat Tail";
        let revision = blake3::hash(original.as_bytes()).to_hex().to_string();
        let failures = [
            json!({"edits":[{"old_text":"First","new_text":"Changed"},{"old_text":"Missing","new_text":"Present"}]}),
            json!({"edits":[{"old_text":"First","new_text":"Changed"},{"old_text":"irst","new_text":"Overlap"}]}),
            json!({"edits":[{"old_text":"First","new_text":"Changed"},{"old_text":"Changed","new_text":"Again"}]}),
            json!({"edits":[{"old_text":"Repeat","new_text":"Unique"}]}),
            json!({"old_text":"First","new_text":"Changed","edits":[{"old_text":"Tail","new_text":"Mixed"}]}),
            json!({"edits":[{"old_text":"First","new_text":"Changed"}],"expected_revision":blake3::hash(b"stale").to_hex().to_string()}),
        ];
        let calls = failures
            .into_iter()
            .map(|mut arguments| {
                arguments["path"] = json!("note.txt");
                if arguments.get("expected_revision").is_none() {
                    arguments["expected_revision"] = json!(revision);
                }
                (name("edit_file"), arguments)
            })
            .collect();
        let server = Server::tools(calls).await;
        let mut fixture = process::Fixture::packages(remote, &server, &[]).await;
        install(&fixture).await;
        std::fs::write(fixture.root.join("note.txt"), original).unwrap();
        configure(&mut fixture, Permission::Project, WorkMode::Code).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        let output = results(&page);
        assert_eq!(output.len(), 6);
        assert!(
            output.iter().all(|result| result["isError"] == true),
            "{output:?}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            original
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
async fn preserves_large_edit_checkpoints() {
    for remote in [false, true] {
        let original = format!(
            "{}Unique target\r\nEnd\r\n",
            "Existing line\r\n".repeat(12000)
        );
        let revision = blake3::hash(original.as_bytes()).to_hex().to_string();
        let updated = original.replace("Unique target\r\n", "Replacement\r\n");
        assert!(original.len() > MAX_FILE_BYTES);
        let server = Server::tools(vec![
            (name("read_file"), json!({"path":"large.txt","offset":12000,"limit":2})),
            (name("find_files"), json!({"globs":["**/*.txt"]})),
            (name("edit_file"), json!({"path":"large.txt","old_text":"Unique target\r\n","new_text":"Replacement\r\n","expected_revision":revision})),
        ])
        .await;
        let mut fixture = process::Fixture::packages(remote, &server, &[]).await;
        install(&fixture).await;
        std::fs::write(fixture.root.join("large.txt"), &original).unwrap();
        configure(&mut fixture, Permission::Project, WorkMode::Code).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("large.txt")).unwrap(),
            updated
        );
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(page.approvals[0].source, ApprovalSource::Project);
        let output = results(&page);
        assert_eq!(output.len(), 3);
        assert_eq!(output[0]["data"]["lines"][0]["number"], 12001);
        assert_eq!(output[0]["data"]["revision"], revision);
        assert_eq!(output[1]["data"]["paths"][0], "large.txt");
        assert_eq!(output[2]["kind"], "file_written");
        let files = checkpoints(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(files.len(), 1);
        let Output::FileCheckpoint(content) = execute(
            &fixture.client,
            Command::ReadFileCheckpoint {
                session: fixture.session.id,
                checkpoint: files[0].id,
            },
        )
        .await
        else {
            panic!("checkpoint content expected")
        };
        assert_eq!(content.before.as_deref(), Some(original.as_str()));
        assert_eq!(content.after, updated);
        execute(
            &fixture.client,
            Command::RestoreFileCheckpoint {
                session: fixture.session.id,
                checkpoint: files[0].id,
                worktree: fixture.session.worktree,
            },
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("large.txt")).unwrap(),
            original
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn waits_for_edit_approval() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            name("edit_file"),
            json!({
                "path":"note.txt","old_text":"Original","new_text":"Edited",
                "expected_revision":blake3::hash(b"Original").to_hex().to_string()
            }),
        )])
        .await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        install(&fixture).await;
        std::fs::write(fixture.root.join("note.txt"), "Original").unwrap();
        let turn = submit(&fixture).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            "Original"
        );
        assert!(
            checkpoints(&fixture.client, fixture.session.id, turn.id)
                .await
                .is_empty()
        );
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            "Edited"
        );
        assert_eq!(
            checkpoints(&fixture.client, fixture.session.id, turn.id)
                .await
                .len(),
            1
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn explores_names_and_lines_in_plan() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (name("find_files"), json!({"globs":["**/*.txt"]})),
            (
                name("read_file"),
                json!({"path":"note.txt","offset":1,"limit":1}),
            ),
        ])
        .await;
        let mut fixture = process::Fixture::packages(remote, &server, &[]).await;
        install(&fixture).await;
        std::fs::write(fixture.root.join("note.txt"), "First\nSecond\nThird\n").unwrap();
        configure(&mut fixture, Permission::Ask, WorkMode::Plan).await;
        let turn = submit(&fixture).await;
        let page = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&fixture.client, fixture.session.id).await;
                if !page.questions.is_empty()
                    || page.runs.iter().any(|run| run.status == Status::Completed)
                {
                    break page;
                }
                assert!(
                    !page.runs.iter().any(|run| run.status == Status::Failed),
                    "{page:?}"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(page.approvals.is_empty());
        let output = results(&page);
        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["data"]["paths"][0], "note.txt");
        assert_eq!(output[1]["data"]["lines"][0]["text"], "Second\n");
        assert_eq!(output[1]["data"]["lines"][0]["number"], 2);
        assert!(
            checkpoints(&fixture.client, fixture.session.id, turn.id)
                .await
                .is_empty()
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
