use super::*;

fn request(fixture: &mut Fixture, turn: TurnId, before: &str) -> Request {
    let worktree = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .unwrap()
        .worktree;
    mutation(
        fixture,
        turn,
        Command::EditFile {
            worktree,
            path: "file.txt".into(),
            old_text: "target".into(),
            new_text: "replacement".into(),
            expected_revision: version(before).revision,
        },
    )
}

#[test]
fn captures_full_content_and_receipt() {
    let mut fixture = Fixture::new();
    let turn = running(&mut fixture);
    let before = format!("{}target\r\n", "retained 🙂\r\n".repeat(20_000));
    let request = request(&mut fixture, turn, &before);
    capture(&mut fixture.database, turn, &request, Some(&before)).unwrap();
    let page = list(
        &fixture.database.connection,
        fixture.session,
        turn,
        None,
        20,
    )
    .unwrap();
    let stored = read(
        &fixture.database.connection,
        fixture.session,
        page.files[0].id,
    )
    .unwrap();
    assert_eq!(stored.before.as_deref(), Some(before.as_str()));
    assert_eq!(stored.after, before.replace("target", "replacement"));
    assert_eq!(stored.file.after, version(&stored.after));
    assert_eq!(stored.file.outcome, RequestOutcome::NotAdmitted);
    let mut altered = request;
    let Command::EditFile { new_text, .. } = &mut altered.command else {
        unreachable!()
    };
    *new_text = "changed".into();
    assert_eq!(
        capture(&mut fixture.database, turn, &altered, Some(&before))
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[test]
fn requires_the_claimed_operation() {
    let mut fixture = Fixture::new();
    let turn = running(&mut fixture);
    let mut request = approved(
        &mut fixture,
        turn,
        "file.txt",
        Some("target"),
        "replacement",
    );
    let Command::WriteFile { worktree, .. } = request.command else {
        unreachable!()
    };
    request.command = Command::EditFile {
        worktree,
        path: "file.txt".into(),
        old_text: "target".into(),
        new_text: "replacement".into(),
        expected_revision: version("target").revision,
    };
    assert_eq!(
        capture(&mut fixture.database, turn, &request, Some("target"))
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
        .unwrap()
        .files
        .is_empty()
    );
}

#[test]
fn rejects_stale_and_ambiguous_content() {
    let mut fixture = Fixture::new();
    let turn = running(&mut fixture);
    for (original, before, code) in [
        ("target", "changed", ErrorCode::RevisionConflict),
        ("target target", "target target", ErrorCode::InvalidRequest),
    ] {
        let request = request(&mut fixture, turn, original);
        assert_eq!(
            capture(&mut fixture.database, turn, &request, Some(before))
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
        .unwrap()
        .files
        .is_empty()
    );
}

#[test]
fn bounds_reads_without_blocking_restore() {
    let mut fixture = Fixture::new();
    let turn = running(&mut fixture);
    let before = format!("{}target", "x".repeat(MAX_FRAME_BYTES / 8));
    let request = request(&mut fixture, turn, &before);
    capture(&mut fixture.database, turn, &request, Some(&before)).unwrap();
    let page = list(
        &fixture.database.connection,
        fixture.session,
        turn,
        None,
        20,
    )
    .unwrap();
    let id = page.files[0].id;
    assert_eq!(
        read(&fixture.database.connection, fixture.session, id)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let content = reads::content(&fixture.database.connection, fixture.session, id).unwrap();
    assert_eq!(content.before.as_deref(), Some(before.as_str()));
    let caller = fixture.database.node;
    let (reply, _) = oneshot::channel();
    fixture
        .database
        .dispatch(caller, request.clone(), reply, &fixture.events);
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
        .unwrap();
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    let diff = diff(&fixture.database.connection, fixture.session, turn).unwrap();
    assert!(diff.partial);
    let root = fixture
        .database
        .worktree_root(content.file.worktree)
        .unwrap();
    let prepared = crate::store::agent::checkpoints::restore::prepare(
        &fixture.database,
        fixture.session,
        id,
        content.file.worktree,
        &root,
    )
    .unwrap();
    std::fs::write(root.join("file.txt"), &prepared.after).unwrap();
    crate::files::checkpoints::restore(&root, &prepared).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        before
    );
}
