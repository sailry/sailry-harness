use super::*;
use sailry_protocol::ChangeKind;

fn patch(body: &str) -> Value {
    json!({"input":format!("*** Begin Patch\n{body}*** End Patch\n")})
}

fn read_file(path: &str) -> (String, Value) {
    (name("read_file"), json!({"path":path}))
}

#[tokio::test]
async fn applies_one_approved_patch() {
    let server = Server::tools(vec![
        read_file("src/lib.rs"),
        read_file("README.md"),
        read_file("old.txt"),
        read_file("dead.txt"),
        (
            name("apply_patch"),
            patch(
                "*** Add File: src/new.rs
+pub fn new() {}
*** Update File: README.md
@@
-# Original
+# Replaced
*** Update File: src/lib.rs
@@
-fn old() {}
+fn renamed() {}
*** Update File: old.txt
*** Move to: src/moved.txt
@@
 moved
*** Delete File: dead.txt
",
            ),
        ),
        // The session's own result establishes the new revision of src/lib.rs.
        (
            name("apply_patch"),
            patch(
                "*** Update File: src/lib.rs
@@
-fn renamed() {}
+fn again() {}
*** Add File: README.md
+Never
",
            ),
        ),
    ])
    .await;
    let fixture = process::Fixture::packages(&server, &[]).await;
    install(&fixture).await;
    let read = |path: &str| std::fs::read_to_string(fixture.root.join(path)).unwrap();
    std::fs::create_dir_all(fixture.root.join("src")).unwrap();
    std::fs::write(fixture.root.join("src/lib.rs"), "fn old() {}\n").unwrap();
    std::fs::write(fixture.root.join("README.md"), "# Original\n").unwrap();
    std::fs::write(fixture.root.join("old.txt"), "moved\n").unwrap();
    std::fs::write(fixture.root.join("dead.txt"), "dead\n").unwrap();
    let turn = submit(&fixture).await;
    let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
    assert_eq!(approval.source, ApprovalSource::User);
    let preview = approval.changes.clone().expect("validated change preview");
    assert_eq!(
        preview
            .files
            .iter()
            .map(|file| file.kind)
            .collect::<Vec<_>>(),
        [
            ChangeKind::Added,
            ChangeKind::Modified,
            ChangeKind::Modified,
            ChangeKind::Renamed,
            ChangeKind::Deleted
        ]
    );
    assert!(
        preview.files[2].diff.contains("-fn old() {}")
            && preview.files[2].diff.contains("+fn renamed() {}"),
        "{preview:?}"
    );
    assert_eq!(
        preview.files[3].destination.as_deref(),
        Some("src/moved.txt")
    );
    assert_eq!(
        (preview.files[4].additions, preview.files[4].deletions),
        (0, 1)
    );
    // One decision covers the patch; nothing is written while it is pending.
    assert!(!fixture.root.join("src/new.rs").exists());
    assert_eq!(read("README.md"), "# Original\n");
    process::decide(&fixture.client, &approval, Decision::Approve).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    assert_eq!(
        page.approvals.len(),
        1,
        "a rejected patch requests no approval"
    );
    let outputs = results(&fixture.client, &page).await;
    assert_eq!(outputs.len(), 6, "{outputs:?}");
    assert_eq!(outputs[4]["kind"], "file_changes");
    assert_eq!(outputs[4]["data"]["status"], "applied", "{outputs:?}");
    assert_eq!(outputs[5]["data"]["status"], "rejected");
    assert_eq!(outputs[5]["data"]["files"][0]["status"], "not_attempted");
    assert_eq!(outputs[5]["data"]["files"][1]["status"], "failed");
    assert_eq!(outputs[5]["data"]["files"][1]["error"]["code"], "conflict");
    assert_eq!(read("src/new.rs"), "pub fn new() {}\n");
    assert_eq!(read("README.md"), "# Replaced\n");
    assert_eq!(read("src/lib.rs"), "fn renamed() {}\n");
    assert_eq!(read("src/moved.txt"), "moved\n");
    assert!(!fixture.root.join("old.txt").exists());
    assert!(!fixture.root.join("dead.txt").exists());
    let view = crate::progress::observe(&fixture.client, fixture.session.id).await;
    assert!(view.calls[4].failure.is_none());
    assert_eq!(
        view.calls[5]
            .failure
            .as_ref()
            .and_then(|failure| failure.code),
        Some(ErrorCode::Conflict)
    );
    let files = checkpoints(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(
        files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["src/lib.rs", "README.md", "src/new.rs"]
    );
    assert!(
        files
            .iter()
            .all(|file| file.receipt() == checkpoint::Receipt::Written)
    );
    let Output::FileCheckpoint(content) = execute(
        &fixture.client,
        Command::ReadFileCheckpoint {
            session: fixture.session.id,
            checkpoint: files[1].id,
        },
    )
    .await
    else {
        panic!("checkpoint content expected")
    };
    assert_eq!(content.before.as_deref(), Some("# Original\n"));
    assert_eq!(content.after, "# Replaced\n");
    let Output::TurnDiff(diff) = execute(
        &fixture.client,
        Command::ReadTurnDiff {
            session: fixture.session.id,
            turn: turn.id,
        },
    )
    .await
    else {
        panic!("turn diff expected")
    };
    assert!(!diff.partial);
    assert_eq!(diff.files.len(), 3);
    let restored = execute(
        &fixture.client,
        Command::RestoreFileCheckpoint {
            session: fixture.session.id,
            checkpoint: files[1].id,
            worktree: fixture.session.worktree,
        },
    )
    .await;
    assert!(matches!(restored, Output::FileRestored(_)));
    assert_eq!(read("README.md"), "# Original\n");
    assert_eq!(read("src/lib.rs"), "fn renamed() {}\n");
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn requires_current_reads() {
    let server = Server::tools(vec![
        (
            name("apply_patch"),
            patch("*** Update File: note.txt\n@@\n-first\n+second\n"),
        ),
        read_file("note.txt"),
        (
            name("apply_patch"),
            patch("*** Update File: note.txt\n@@\n-first\n+second\n"),
        ),
        (
            name("apply_patch"),
            patch("*** Update File: note.txt\n@@\n-second\n+third\n"),
        ),
    ])
    .await;
    let mut fixture = process::Fixture::packages(&server, &[]).await;
    install(&fixture).await;
    std::fs::write(fixture.root.join("note.txt"), "first\n").unwrap();
    configure(&mut fixture, Permission::Project, WorkMode::Code).await;
    let turn = submit(&fixture).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    let outputs = results(&fixture.client, &page).await;
    assert_eq!(outputs.len(), 4, "{outputs:?}");
    assert_eq!(outputs[0]["data"]["status"], "rejected");
    assert_eq!(
        outputs[0]["data"]["files"][0]["error"]["code"],
        "revision_conflict"
    );
    assert_eq!(
        outputs[0]["data"]["files"][0]["error"]["message"],
        "the current revision of this file has not been read; read the file again"
    );
    // A fresh read admits the patch; the session's own result admits the next one.
    assert_eq!(outputs[2]["data"]["status"], "applied", "{outputs:?}");
    assert_eq!(outputs[3]["data"]["status"], "applied", "{outputs:?}");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
        "third\n"
    );
    assert_eq!(page.approvals.len(), 2);
    assert!(
        page.approvals
            .iter()
            .all(|approval| approval.source == ApprovalSource::Project)
    );
    assert_eq!(
        checkpoints(&fixture.client, fixture.session.id, turn.id)
            .await
            .len(),
        2
    );
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn rechecks_revisions_after_approval() {
    let server = Server::tools(vec![
        read_file("a.txt"),
        read_file("b.txt"),
        (
            name("apply_patch"),
            patch(
                "*** Update File: a.txt
@@
-A
+A2
*** Update File: b.txt
@@
-B
+B2
*** Add File: c.txt
+C
",
            ),
        ),
    ])
    .await;
    let fixture = process::Fixture::packages(&server, &[]).await;
    install(&fixture).await;
    std::fs::write(fixture.root.join("a.txt"), "A\n").unwrap();
    std::fs::write(fixture.root.join("b.txt"), "B\n").unwrap();
    let turn = submit(&fixture).await;
    let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
    assert_eq!(approval.changes.as_ref().unwrap().files.len(), 3);
    std::fs::write(fixture.root.join("b.txt"), "External\n").unwrap();
    process::decide(&fixture.client, &approval, Decision::Approve).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
    let outputs = results(&fixture.client, &page).await;
    assert_eq!(outputs.len(), 3);
    let rejected = &outputs[2]["data"];
    assert_eq!(rejected["status"], "rejected", "{outputs:?}");
    assert_eq!(
        rejected["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["status"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["not_attempted", "failed", "not_attempted"]
    );
    assert_eq!(rejected["files"][1]["path"], "b.txt");
    assert_eq!(
        rejected["files"][1]["error"]["code"], "revision_conflict",
        "{outputs:?}"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("a.txt")).unwrap(),
        "A\n"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("b.txt")).unwrap(),
        "External\n"
    );
    assert!(!fixture.root.join("c.txt").exists());
    assert!(
        checkpoints(&fixture.client, fixture.session.id, turn.id)
            .await
            .is_empty()
    );
    fixture.node.shutdown().await.unwrap();
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn follows_plan_and_project_permissions() {
    let server = Server::turn_tools(vec![(
        name("apply_patch"),
        patch("*** Add File: planned.txt\n+Planned\n"),
    )])
    .await;
    let mut fixture = process::Fixture::packages(&server, &[]).await;
    install(&fixture).await;
    configure(&mut fixture, Permission::Full, WorkMode::Plan).await;
    let planned = submit(&fixture).await;
    let page = finished(&fixture.client, fixture.session.id, planned.id).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
    let outputs = results(&fixture.client, &page).await;
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0]["error"]["code"], "permission_denied");
    assert!(outputs[0].to_string().contains("planning turns"));
    assert!(page.approvals.is_empty());
    assert!(!fixture.root.join("planned.txt").exists());
    configure(&mut fixture, Permission::Project, WorkMode::Code).await;
    let turn = submit(&fixture).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(page.approvals.len(), 1);
    assert_eq!(page.approvals[0].source, ApprovalSource::Project);
    assert_eq!(page.approvals[0].state, ApprovalState::Approved);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("planned.txt")).unwrap(),
        "Planned\n"
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
