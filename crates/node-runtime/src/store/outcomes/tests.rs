use super::*;
use crate::store::{Database, database::conflict, external::Work, mutations};
use sailry_protocol::{Command, EventEnvelope, FileWritten, Request};
use tokio::sync::{broadcast, mpsc, oneshot};

tokio::task_local! {
    static WRITES: mpsc::UnboundedSender<oneshot::Sender<()>>;
}

pub(super) async fn before_write() -> Result<(), Fault> {
    let Ok(sender) = WRITES.try_with(Clone::clone) else {
        return Ok(());
    };
    let (reply, released) = oneshot::channel();
    if sender.send(reply).is_ok() {
        released.await.map_err(storage_error)?;
    }
    Ok(())
}

async fn command(
    database: &mut Database,
    events: &broadcast::Sender<EventEnvelope>,
    request: Request,
) -> sailry_link::Response {
    let (reply, response) = oneshot::channel();
    assert!(
        database
            .dispatch(database.node, request, reply, events)
            .await
            .is_none()
    );
    response.await.unwrap().unwrap().completion.await.unwrap()
}

async fn preserves_effect(conflicts: usize) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let mut database = Database::open(&root.join("node.turso"), NodeId([83; 32]), None)
        .await
        .unwrap();
    let (events, _) = broadcast::channel(64);
    command(
        &mut database,
        &events,
        Request::new(
            NodeId([83; 32]),
            Command::RegisterProject {
                name: "Captured receipt".into(),
                path: root.to_str().unwrap().into(),
            },
        ),
    )
    .await
    .unwrap();
    let worktree: String = database
        .connection
        .prepare("SELECT id FROM worktrees LIMIT 1")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    let caller = database.node;
    let request = Request::new(
        caller,
        Command::WriteFile {
            worktree: worktree.parse().unwrap(),
            path: "saved.txt".into(),
            text: "captured effect".into(),
            expected_revision: None,
        },
    );
    let (reply, response) = oneshot::channel();
    let Some(Work::Mutation(mutation)) = database
        .dispatch(caller, request.clone(), reply, &events)
        .await
    else {
        panic!("file work expected")
    };
    let admission = response.await.unwrap().unwrap();
    assert!(admission.receipt.durable);
    let captured = mutations::execute(&mutation.roots, &mutation.request);
    assert!(matches!(
        &captured,
        Ok(Output::FileWritten(FileWritten { size: 15, .. }))
    ));
    // A receipt retry must never overwrite a later edit by another file owner.
    std::fs::write(root.join("saved.txt"), "later manual edit").unwrap();
    let other = database.handle.lease(false).await.unwrap();
    let (writes, mut started) = mpsc::unbounded_channel();
    let record = WRITES.scope(
        writes,
        database.finish_external(caller, &request, captured.clone(), &events),
    );
    let competing = async {
        for _ in 0..conflicts {
            let release = started.recv().await.unwrap();
            let transaction = conflict::begin(&other).await.unwrap();
            transaction
                .execute(
                    "UPDATE requests SET body=body WHERE caller=?1 AND id=?2",
                    params![&caller.0[..], request.id.to_string()],
                )
                .await
                .unwrap();
            conflict::commit(transaction).await.unwrap();
            release.send(()).unwrap();
        }
        drop(started);
    };
    let (result, ()) = tokio::join!(record, competing);
    assert!(database.connection.is_autocommit().unwrap());
    if conflicts > conflict::RETRIES {
        assert_eq!(result.unwrap_err().code, ErrorCode::OutcomeUnknown);
        let row = database
            .connection
            .prepare("SELECT status,result FROM requests WHERE id=?1")
            .await
            .unwrap()
            .query_row([request.id.to_string()])
            .await
            .unwrap();
        assert_eq!(row.get::<String>(0).unwrap(), "admitted");
        assert_eq!(row.get::<Option<String>>(1).unwrap(), None);
        assert_eq!(
            command(&mut database, &events, request)
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
    } else {
        assert_eq!(result, captured);
        assert_eq!(command(&mut database, &events, request).await, captured);
    }
    assert_eq!(
        std::fs::read_to_string(root.join("saved.txt")).unwrap(),
        "later manual edit"
    );
}

#[tokio::test]
async fn retains_captured_file_outcomes_after_conflict() {
    preserves_effect(1).await;
}

#[tokio::test]
async fn bounds_recording_without_repeating_effects() {
    preserves_effect(conflict::RETRIES + 1).await;
}
