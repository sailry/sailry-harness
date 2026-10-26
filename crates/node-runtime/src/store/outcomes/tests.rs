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

async fn preserves_external(conflicts: usize, known_failure: bool) {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let caller = NodeId([84; 32]);
    let mut database = Database::open(&root.join("node.turso"), caller, None)
        .await
        .unwrap();
    let (events, _) = broadcast::channel(64);
    command(
        &mut database,
        &events,
        Request::new(
            caller,
            Command::RegisterProject {
                name: "External receipt".into(),
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
    let request = Request::new(
        caller,
        Command::CreateDirectory {
            worktree: worktree.parse().unwrap(),
            path: "created".into(),
        },
    );
    let (reply, response) = oneshot::channel();
    let Some(Work::Mutation(mutation)) = database
        .dispatch(caller, request.clone(), reply, &events)
        .await
    else {
        panic!("directory work expected")
    };
    assert!(response.await.unwrap().unwrap().receipt.durable);
    let captured = if known_failure {
        let mut error = Fault::new(ErrorCode::Busy, "captured external conflict");
        error.details = Some(sailry_protocol::FaultDetails::StorageConflict);
        Err(error)
    } else {
        let result = mutations::execute(&mutation.roots, &mutation.request);
        assert!(result.is_ok());
        std::fs::write(root.join("created/sentinel"), "later manual edit").unwrap();
        result
    };
    let other = database.handle.lease(false).await.unwrap();
    let attempts = AtomicUsize::new(0);
    let mut changes = events.subscribe();
    let result = database
        .finish_external_with(
            caller,
            &request,
            captured.clone(),
            &events,
            async |db, result| {
                assert_eq!(*result, captured);
                let status: String = db
                    .prepare("SELECT status FROM requests WHERE id=?1")
                    .await
                    .map_err(storage_error)?
                    .query_row([request.id.to_string()])
                    .await
                    .map_err(storage_error)?
                    .get(0)
                    .map_err(storage_error)?;
                assert_eq!(status, "admitted");
                if attempts.fetch_add(1, Ordering::SeqCst) < conflicts {
                    let transaction = conflict::begin(&other).await?;
                    transaction
                        .execute(
                            "UPDATE requests SET body=body WHERE id=?1",
                            [request.id.to_string()],
                        )
                        .await
                        .map_err(storage_error)?;
                    conflict::commit(transaction).await?;
                    // The next attempt must restore the captured result before finalizers run.
                    *result = Err(Fault::invalid("uncommitted attempt"));
                }
                Ok(Some(sailry_protocol::Event::KnowledgeCollectionsChanged))
            },
        )
        .await;
    assert!(database.connection.is_autocommit().unwrap());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        conflicts.min(conflict::RETRIES) + 1
    );
    if conflicts > conflict::RETRIES {
        assert_eq!(result.unwrap_err().code, ErrorCode::OutcomeUnknown);
        assert!(changes.try_recv().is_err());
        assert_eq!(
            command(&mut database, &events, request)
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
    } else {
        assert_eq!(result, captured);
        assert!(matches!(
            changes.try_recv().unwrap().event,
            sailry_protocol::Event::KnowledgeCollectionsChanged
        ));
        assert!(changes.try_recv().is_err());
        assert_eq!(command(&mut database, &events, request).await, captured);
    }
    if known_failure {
        assert!(!root.join("created").exists());
    } else {
        assert_eq!(
            std::fs::read_to_string(root.join("created/sentinel")).unwrap(),
            "later manual edit"
        );
    }
}

#[tokio::test]
async fn retries_external_receipts_after_conflict() {
    preserves_external(1, false).await;
}

#[tokio::test]
async fn bounds_external_receipt_retries() {
    preserves_external(conflict::RETRIES + 1, false).await;
}

#[tokio::test]
async fn records_captured_external_conflicts() {
    preserves_external(0, true).await;
}
