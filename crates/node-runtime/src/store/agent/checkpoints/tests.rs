use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

mod diff;
mod restore;

fn running(fixture: &mut Fixture) -> TurnId {
    crate::store::agent::tests::fixture::files(&fixture.database, fixture._root.path());
    let turn = fixture.queued();
    runs::start(&fixture.database.connection, turn).unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .unwrap()
            .turn
            .id,
        turn
    );
    turn
}

fn approved(
    fixture: &mut Fixture,
    turn: TurnId,
    path: &str,
    before: Option<&str>,
    text: &str,
) -> Request {
    let authorization = adk_core::ToolConfirmationRequest {
        tool_name: crate::plugins::tools::alias("files", "write_file"),
        function_call_id: Some(RequestId::new().to_string()),
        args: json!({"path": path, "text": text, "expected_revision": before.map(version).map(|value| value.revision)}),
    };
    let mut event = AdkEvent::new("checkpoint-fixture");
    event.author = "assistant".into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: authorization.function_call_id.clone(),
            name: authorization.tool_name.clone(),
            args: authorization.args.clone(),
            thought_signature: None,
        }],
    });
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        event,
        &fixture.events,
    )
    .unwrap();
    let approval =
        approvals::begin(&mut fixture.database, turn, &authorization, &fixture.events).unwrap();
    approvals::resolve(
        &fixture.database.connection,
        fixture.session,
        approval.id,
        Decision::Approve,
    )
    .unwrap();
    let id = approvals::authorize(&mut fixture.database.connection, turn, &authorization).unwrap();
    let session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    let mut request = Request::new(
        fixture.database.node,
        Command::WriteFile {
            worktree: session.worktree,
            path: path.into(),
            text: text.into(),
            expected_revision: before.map(version).map(|value| value.revision),
        },
    );
    request.id = id;
    request.plugin = Some(sailry_protocol::plugin::Context {
        invocation: None,
        turn: Some(turn),
        surface: Default::default(),
        package: crate::store::plugins::get(&fixture.database.connection, "files")
            .unwrap()
            .unwrap()
            .summary
            .reference(),
        worktree: Some(session.worktree),
        session: Some(session.id),
    });
    request
}

#[test]
fn keeps_original_receipt() {
    let mut fixture = Fixture::new();
    let turn = running(&mut fixture);
    let request = approved(
        &mut fixture,
        turn,
        "资料.txt",
        Some("Before 中文 🙂"),
        "After 中文 🙂",
    );
    capture(
        &mut fixture.database,
        turn,
        &request,
        Some("Before 中文 🙂"),
    )
    .unwrap();
    capture(
        &mut fixture.database,
        turn,
        &request,
        Some("Before 中文 🙂"),
    )
    .unwrap();
    let page = list(
        &fixture.database.connection,
        fixture.session,
        turn,
        None,
        20,
    )
    .unwrap();
    assert_eq!(page.files.len(), 1);
    assert_eq!(page.files[0].outcome, RequestOutcome::NotAdmitted);
    let id = page.files[0].id;
    let content = read(&fixture.database.connection, fixture.session, id).unwrap();
    assert_eq!(content.before.as_deref(), Some("Before 中文 🙂"));
    assert_eq!(content.after, "After 中文 🙂");
    assert_eq!(content.file.before, Some(version("Before 中文 🙂")));
    let (reply, _) = oneshot::channel();
    let caller = fixture.database.node;
    let work = fixture
        .database
        .dispatch(caller, request.clone(), reply, &fixture.events);
    assert!(matches!(
        work,
        Some(crate::store::external::Work::Mutation(_))
    ));
    assert_eq!(
        read(&fixture.database.connection, fixture.session, id)
            .unwrap()
            .file
            .outcome,
        RequestOutcome::Admitted
    );
    let outcome = Err(Fault::new(
        ErrorCode::OutcomeUnknown,
        "injected uncertain write",
    ));
    assert_eq!(
        fixture
            .database
            .finish_external(caller, &request, outcome.clone(), &fixture.events),
        outcome
    );
    assert_eq!(
        read(&fixture.database.connection, fixture.session, id)
            .unwrap()
            .file
            .outcome,
        RequestOutcome::Completed(Box::new(outcome))
    );
    assert_eq!(
        fixture
            .database
            .connection
            .query_row("SELECT count(*) FROM agent_events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let metadata: Vec<u8> = fixture
        .database
        .connection
        .query_row("SELECT body FROM file_checkpoints", [], |row| row.get(0))
        .unwrap();
    let metadata = String::from_utf8(metadata).unwrap();
    assert!(!metadata.contains("Before") && !metadata.contains("After"));
    let prepared: String = fixture
        .database
        .connection
        .query_row("SELECT after_text FROM file_checkpoints", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(prepared, "After 中文 🙂");
}

#[test]
fn checks_call_scope() {
    let mut fixture = Fixture::new();
    let turn = running(&mut fixture);
    let request = approved(&mut fixture, turn, "file.txt", Some("before"), "after");
    assert_eq!(
        capture(&mut fixture.database, turn, &request, Some("changed"))
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let mut altered = request.clone();
    altered.plugin.as_mut().unwrap().package.digest = "unapproved".into();
    assert_eq!(
        capture(&mut fixture.database, turn, &altered, Some("before"))
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    altered = request.clone();
    altered.id = RequestId::new();
    assert_eq!(
        capture(&mut fixture.database, turn, &altered, Some("before"))
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    altered = request.clone();
    let Command::WriteFile { worktree, .. } = &mut altered.command else {
        unreachable!()
    };
    *worktree = WorktreeId::new();
    assert_eq!(
        capture(&mut fixture.database, turn, &altered, Some("before"))
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
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
    capture(&mut fixture.database, turn, &request, Some("before")).unwrap();
    let mut altered = request.clone();
    let Command::WriteFile { text, .. } = &mut altered.command else {
        unreachable!()
    };
    *text = "changed after capture".into();
    assert_eq!(
        capture(&mut fixture.database, turn, &altered, Some("before"))
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    assert_eq!(
        capture(&mut fixture.database, turn, &request, Some("before"))
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[test]
fn preserves_fork_references() {
    let mut fixture = Fixture::new();
    let turn = running(&mut fixture);
    for path in ["first", "second", "third"] {
        let request = approved(&mut fixture, turn, path, None, "new file");
        capture(&mut fixture.database, turn, &request, None).unwrap();
    }
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    let page = list(&fixture.database.connection, fixture.session, turn, None, 2).unwrap();
    assert_eq!(
        page.files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["third", "second"]
    );
    let older = list(
        &fixture.database.connection,
        fixture.session,
        turn,
        page.next,
        2,
    )
    .unwrap();
    assert_eq!(older.files.len(), 1);
    assert_eq!(older.files[0].path, "first");
    assert!(older.files[0].before.is_none() && older.next.is_none());
    let Output::Session(branch) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through: turn,
            expected_revision: 1,
        },
    ) else {
        panic!("branch expected")
    };
    assert_eq!(
        list(&fixture.database.connection, branch.id, turn, None, 2)
            .unwrap()
            .files,
        page.files
    );
    let Output::Rewound(rewound) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: fixture.session,
            through: None,
            expected_head: turn,
            expected_revision: 1,
        },
    ) else {
        panic!("rewind expected")
    };
    let id = page.files[0].id;
    assert_eq!(
        read(&fixture.database.connection, fixture.session, id)
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
    assert_eq!(
        read(&fixture.database.connection, rewound.backup.id, id)
            .unwrap()
            .file,
        page.files[0]
    );
    assert_eq!(
        list(&fixture.database.connection, branch.id, turn, None, 0)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        list(
            &fixture.database.connection,
            branch.id,
            turn,
            Some(CheckpointId::new()),
            1
        )
        .unwrap_err()
        .code,
        ErrorCode::WrongTarget
    );
    assert_eq!(
        fixture
            .database
            .connection
            .query_row("SELECT count(*) FROM file_checkpoints", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
}
