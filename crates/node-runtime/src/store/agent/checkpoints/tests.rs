use super::*;
use crate::agent::canonical::{BlockKind, Role as MessageRole};
use crate::store::agent::tests::fixture::{message, request as tool_request};
use crate::store::agent::tests::{command, fixture::Fixture};

mod diff;
mod edit;
mod restore;

async fn running(fixture: &mut Fixture) -> TurnId {
    crate::store::agent::tests::fixture::files(&fixture.database, fixture._root.path()).await;
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap()
            .turn
            .id,
        turn
    );
    turn
}

async fn approved(
    fixture: &mut Fixture,
    turn: TurnId,
    path: &str,
    before: Option<&str>,
    text: &str,
) -> Request {
    let worktree = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .worktree;
    mutation(
        fixture,
        turn,
        Command::WriteFile {
            worktree,
            path: path.into(),
            text: text.into(),
            expected_revision: before.map(version).map(|value| value.revision),
        },
    )
    .await
}

async fn mutation(fixture: &mut Fixture, turn: TurnId, command: Command) -> Request {
    let (name, arguments) = match &command {
        Command::WriteFile {
            path,
            text,
            expected_revision,
            ..
        } => (
            "write_file",
            json!({"path":path,"text":text,"expected_revision":expected_revision}),
        ),
        Command::EditFile {
            path,
            old_text,
            new_text,
            replace_all,
            edits,
            expected_revision,
            ..
        } => (
            "edit_file",
            if let Some(edits) = edits {
                json!({"path":path,"edits":edits,"expected_revision":expected_revision})
            } else {
                json!({"path":path,"old_text":old_text,"new_text":new_text,"replace_all":replace_all,"expected_revision":expected_revision})
            },
        ),
        _ => panic!("file mutation expected"),
    };
    let authorization = crate::agent::canonical::ToolIdentity {
        name: crate::plugins::tools::alias("files", name),
        id: Some(RequestId::new().to_string()),
        arguments,
    };
    let mut event = message(
        "assistant",
        MessageRole::Assistant,
        vec![BlockKind::ToolRequest(tool_request(
            authorization.id.as_deref().expect("call identity"),
            &authorization.name,
            authorization.arguments.clone(),
        ))],
    );
    event.turn = turn;
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    let approval = approvals::begin(
        &mut fixture.database,
        turn,
        &authorization,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    approvals::resolve(
        &fixture.database.connection,
        fixture.session,
        approval.id,
        Decision::Approve,
    )
    .await
    .unwrap();
    let id = approvals::authorize(&mut fixture.database, turn, &authorization, None)
        .await
        .unwrap();
    let session = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let mut request = Request::new(fixture.database.node, command);
    request.id = id;
    request.plugin = Some(sailry_protocol::plugin::Context {
        invocation: None,
        turn: Some(turn),
        surface: Default::default(),
        package: crate::store::plugins::get(&fixture.database.connection, "files")
            .await
            .unwrap()
            .unwrap()
            .summary
            .reference(),
        worktree: Some(session.worktree),
        session: Some(session.id),
    });
    request
}

#[tokio::test]
async fn keeps_original_receipt() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let request = approved(
        &mut fixture,
        turn,
        "资料.txt",
        Some("Before 中文 🙂"),
        "After 中文 🙂",
    )
    .await;
    capture(
        &mut fixture.database,
        turn,
        &request,
        Some("Before 中文 🙂"),
    )
    .await
    .unwrap();
    capture(
        &mut fixture.database,
        turn,
        &request,
        Some("Before 中文 🙂"),
    )
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
    assert_eq!(page.files.len(), 1);
    assert_eq!(page.files[0].outcome, RequestOutcome::NotAdmitted);
    let id = page.files[0].id;
    let content = read(&fixture.database.connection, fixture.session, id)
        .await
        .unwrap();
    assert_eq!(content.before.as_deref(), Some("Before 中文 🙂"));
    assert_eq!(content.after, "After 中文 🙂");
    assert_eq!(content.file.before, Some(version("Before 中文 🙂")));
    let (reply, _) = oneshot::channel();
    let caller = fixture.database.node;
    let work = fixture
        .database
        .dispatch(caller, request.clone(), reply, &fixture.events)
        .await;
    assert!(matches!(
        work,
        Some(crate::store::external::Work::Mutation(_))
    ));
    assert_eq!(
        read(&fixture.database.connection, fixture.session, id)
            .await
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
            .finish_external(caller, &request, outcome.clone(), &fixture.events)
            .await,
        outcome
    );
    assert_eq!(
        read(&fixture.database.connection, fixture.session, id)
            .await
            .unwrap()
            .file
            .outcome,
        RequestOutcome::Completed(Box::new(outcome))
    );
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM agent_events")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        1
    );
    let metadata: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT body FROM file_checkpoints")
        .await
        .unwrap()
        .query_row(())
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    let metadata = String::from_utf8(metadata).unwrap();
    assert!(!metadata.contains("Before") && !metadata.contains("After"));
    let prepared: String = fixture
        .database
        .connection
        .prepare("SELECT after_text FROM file_checkpoints")
        .await
        .unwrap()
        .query_row(())
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(prepared, "After 中文 🙂");
}

#[tokio::test]
async fn checks_call_scope() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let request = approved(&mut fixture, turn, "file.txt", Some("before"), "after").await;
    assert_eq!(
        capture(&mut fixture.database, turn, &request, Some("changed"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let mut altered = request.clone();
    altered.plugin.as_mut().unwrap().package.digest = "unapproved".into();
    assert_eq!(
        capture(&mut fixture.database, turn, &altered, Some("before"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    altered = request.clone();
    altered.id = RequestId::new();
    assert_eq!(
        capture(&mut fixture.database, turn, &altered, Some("before"))
            .await
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
            .await
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
        .await
        .unwrap()
        .files
        .is_empty()
    );
    capture(&mut fixture.database, turn, &request, Some("before"))
        .await
        .unwrap();
    let mut altered = request.clone();
    let Command::WriteFile { text, .. } = &mut altered.command else {
        unreachable!()
    };
    *text = "changed after capture".into();
    assert_eq!(
        capture(&mut fixture.database, turn, &altered, Some("before"))
            .await
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
    .await
    .unwrap();
    assert_eq!(
        capture(&mut fixture.database, turn, &request, Some("before"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn preserves_fork_references() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    for path in ["first", "second", "third"] {
        let request = approved(&mut fixture, turn, path, None, "new file").await;
        capture(&mut fixture.database, turn, &request, None)
            .await
            .unwrap();
    }
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    let page = list(&fixture.database.connection, fixture.session, turn, None, 2)
        .await
        .unwrap();
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
    .await
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
    )
    .await
    else {
        panic!("branch expected")
    };
    assert_eq!(
        list(&fixture.database.connection, branch.id, turn, None, 2)
            .await
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
    )
    .await
    else {
        panic!("rewind expected")
    };
    let id = page.files[0].id;
    assert_eq!(
        read(&fixture.database.connection, fixture.session, id)
            .await
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
    assert!(rewound.backup.is_none());
    assert_eq!(
        read(&fixture.database.connection, branch.id, id)
            .await
            .unwrap()
            .file,
        page.files[0]
    );
    assert_eq!(
        list(&fixture.database.connection, branch.id, turn, None, 0)
            .await
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
        .await
        .unwrap_err()
        .code,
        ErrorCode::WrongTarget
    );
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM file_checkpoints")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        3
    );
}
