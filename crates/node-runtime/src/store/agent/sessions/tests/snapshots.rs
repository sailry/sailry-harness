use super::*;
use crate::store::database::conflict;
use tokio::sync::{mpsc, oneshot};

tokio::task_local! {
    static READS: mpsc::UnboundedSender<oneshot::Sender<Result<(), Fault>>>;
}

pub(in crate::store::agent::sessions) async fn before_state() -> Result<(), Fault> {
    let Ok(sender) = READS.try_with(Clone::clone) else {
        return Ok(());
    };
    let (reply, response) = oneshot::channel();
    sender.send(reply).map_err(storage_error)?;
    response.await.map_err(storage_error)?
}

#[tokio::test]
async fn pins_history_and_state() {
    let (mut fixture, turn) = running().await;
    let message = ModelMessage::assistant()
        .with_generated_id()
        .with_tool_request("first", Ok(CallToolRequestParams::new("read_file")));
    apply(&mut fixture, turn, vec![message.into()])
        .await
        .unwrap();
    let writer = fixture.database.handle.lease(false).await.unwrap();
    let (send, mut reads) = mpsc::unbounded_channel();
    let loading = READS.scope(send, load(&fixture, turn));
    let writing = async {
        let release = reads.recv().await.expect("loader reached its state read");
        writer
            .execute(
                "UPDATE agent_state SET body=?1 WHERE owner=?2 AND key=?3",
                params![
                    encode(&vec!["new_tool"]).unwrap(),
                    fixture.session.to_string(),
                    LOADED_TOOLS
                ],
            )
            .await
            .unwrap();
        release.send(Ok(())).unwrap();
    };
    let (loaded, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(loading, writing)
    })
    .await
    .expect("session snapshot fixture timed out");
    assert_eq!(loaded.tool_names, ["read_file".to_string()].into());
    assert_eq!(loaded.context.loaded_tools, Some(vec!["read_file".into()]));
    assert!(fixture.database.connection.is_autocommit().unwrap());
    assert_eq!(
        load(&fixture, turn).await.context.loaded_tools,
        Some(vec!["new_tool".into()])
    );
}

#[tokio::test]
async fn preserves_errors_without_replaying_reads() {
    let (fixture, turn) = running().await;
    for error in [
        storage_error(turso::Error::Busy("checkpoint".into())),
        Fault::new(ErrorCode::RevisionConflict, "session revision changed"),
    ] {
        let expected = error.clone();
        let (send, mut reads) = mpsc::unbounded_channel();
        let loading = READS.scope(
            send,
            get(
                &fixture.database.connection,
                turn,
                Query {
                    session: fixture.session,
                    recent: None,
                    after: None,
                },
            ),
        );
        let failing = async {
            reads
                .recv()
                .await
                .expect("loader reached its state read")
                .send(Err(error))
                .unwrap();
        };
        let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(loading, failing)
        })
        .await
        .expect("failed session snapshot fixture timed out");
        assert_eq!(result.err(), Some(expected));
        assert!(matches!(
            reads.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
        assert!(fixture.database.connection.is_autocommit().unwrap());
        let transaction = conflict::begin_read(&fixture.database.connection)
            .await
            .unwrap();
        conflict::finish_read(transaction, Ok(())).await.unwrap();
    }
}
