use super::*;

async fn request(fixture: &mut Fixture, turn: TurnId, before: &str) -> Request {
    let worktree = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .worktree;
    mutation(
        fixture,
        turn,
        Command::EditFile {
            worktree,
            path: "file.txt".into(),
            old_text: Some("target".into()),
            new_text: Some("replacement".into()),
            replace_all: false,
            edits: None,
            expected_revision: version(before).revision,
        },
    )
    .await
}

#[tokio::test]
async fn captures_full_content_and_receipt() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let before = format!("{}target\r\n", "retained 🙂\r\n".repeat(20_000));
    let request = request(&mut fixture, turn, &before).await;
    capture(&mut fixture.database, turn, &request, Some(&before))
        .await
        .unwrap();
    let page = list(
        &fixture.database.connection,
        fixture.session,
        turn,
        None,
        20,
    )
    .await
    .unwrap();
    let stored = read(
        &fixture.database.connection,
        fixture.session,
        page.files[0].id,
    )
    .await
    .unwrap();
    assert_eq!(stored.before.as_deref(), Some(before.as_str()));
    assert_eq!(stored.after, before.replace("target", "replacement"));
    assert_eq!(stored.file.after, version(&stored.after));
    assert_eq!(stored.file.outcome, RequestOutcome::NotAdmitted);
    let mut altered = request;
    let Command::EditFile { new_text, .. } = &mut altered.command else {
        unreachable!()
    };
    *new_text = Some("changed".into());
    assert_eq!(
        capture(&mut fixture.database, turn, &altered, Some(&before))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn requires_the_claimed_operation() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let mut request = approved(
        &mut fixture,
        turn,
        "file.txt",
        Some("target"),
        "replacement",
    )
    .await;
    let Command::WriteFile { worktree, .. } = request.command else {
        unreachable!()
    };
    request.command = Command::EditFile {
        worktree,
        path: "file.txt".into(),
        old_text: Some("target".into()),
        new_text: Some("replacement".into()),
        replace_all: false,
        edits: None,
        expected_revision: version("target").revision,
    };
    assert_eq!(
        capture(&mut fixture.database, turn, &request, Some("target"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert!(
        list(
            &fixture.database.connection,
            fixture.session,
            turn,
            None,
            20
        )
        .await
        .unwrap()
        .files
        .is_empty()
    );
}

#[tokio::test]
async fn rejects_stale_and_ambiguous_content() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    for (original, before, code) in [
        ("target", "changed", ErrorCode::RevisionConflict),
        ("target target", "target target", ErrorCode::InvalidRequest),
    ] {
        let request = request(&mut fixture, turn, original).await;
        assert_eq!(
            capture(&mut fixture.database, turn, &request, Some(before))
                .await
                .unwrap_err()
                .code,
            code
        );
    }
    assert!(
        list(
            &fixture.database.connection,
            fixture.session,
            turn,
            None,
            20
        )
        .await
        .unwrap()
        .files
        .is_empty()
    );
}

#[tokio::test]
async fn bounds_reads_without_blocking_restore() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let before = format!("{}target", "x".repeat(MAX_FRAME_BYTES / 8));
    let request = request(&mut fixture, turn, &before).await;
    capture(&mut fixture.database, turn, &request, Some(&before))
        .await
        .unwrap();
    let page = list(
        &fixture.database.connection,
        fixture.session,
        turn,
        None,
        20,
    )
    .await
    .unwrap();
    let id = page.files[0].id;
    assert_eq!(
        read(&fixture.database.connection, fixture.session, id)
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let content = reads::content(&fixture.database.connection, fixture.session, id)
        .await
        .unwrap();
    assert_eq!(content.before.as_deref(), Some(before.as_str()));
    let caller = fixture.database.node;
    let (reply, _) = oneshot::channel();
    fixture
        .database
        .dispatch(caller, request.clone(), reply, &fixture.events)
        .await;
    let written = FileWritten {
        path: "file.txt".into(),
        revision: content.file.after.revision.clone(),
        size: content.file.after.size,
    };
    fixture
        .database
        .finish_external(
            caller,
            &request,
            Ok(Output::FileWritten(written)),
            &fixture.events,
        )
        .await
        .unwrap();
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    let diff = diff(&fixture.database.connection, fixture.session, turn)
        .await
        .unwrap();
    assert!(diff.partial);
    let root = fixture
        .database
        .worktree_root(content.file.worktree)
        .await
        .unwrap();
    let prepared = crate::store::agent::checkpoints::restore::prepare(
        &fixture.database,
        fixture.session,
        id,
        content.file.worktree,
        &root,
    )
    .await
    .unwrap();
    std::fs::write(root.join("file.txt"), &prepared.after).unwrap();
    crate::files::checkpoints::restore(&root, &prepared).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        before
    );
}
