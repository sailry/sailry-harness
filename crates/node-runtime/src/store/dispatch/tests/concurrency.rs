use super::*;
use crate::store::database::{WRITE_CONCURRENCY, conflict};
use std::time::Duration;

#[tokio::test]
async fn claim_allows_peer_commit() {
    overlap(false).await;
}

#[tokio::test]
async fn finish_allows_peer_commit() {
    overlap(true).await;
}

async fn overlap(finishing: bool) {
    let mut fixture = Fixture::new().await;
    let handler = fixture.setup("concurrent").await;
    let transaction = conflict::begin(&fixture.database.connection).await.unwrap();
    let job = events::enqueue(
        &transaction,
        Some(&fixture.package),
        &handler,
        &fixture.event("concurrent"),
    )
    .await
    .unwrap();
    conflict::commit(transaction).await.unwrap();
    let (events, mut updates) = tokio::sync::broadcast::channel(16);
    if finishing {
        let batch = tick(&mut fixture.database, &events, false).await.unwrap();
        assert_eq!(
            batch.work.iter().map(|work| work.job).collect::<Vec<_>>(),
            vec![job.id]
        );
        updates.try_recv().unwrap();
    }
    let prior: i64 = fixture
        .database
        .connection
        .prepare("SELECT coalesce(max(cursor),0) FROM events")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    fixture
        .database
        .connection
        .execute(
            "INSERT INTO oauth_registrations(rowid,provider,client_id) \
             VALUES(-1,'dispatch-probe','fixture')",
            (),
        )
        .await
        .unwrap();
    let probe = fixture.database.connection.connection().clone();
    assert_eq!(probe.last_insert_rowid(), -1);
    let writer = fixture.database.handle.writer().await.unwrap();
    let transaction = conflict::begin(&writer).await.unwrap();
    let peer = protocol::NodeId([9; 32]);
    transaction
        .execute("INSERT INTO link_peers(identity) VALUES(?1)", [&peer.0[..]])
        .await
        .unwrap();
    let publication = fixture.database.publication.clone().lock().await;
    let dispatch = async {
        if finishing {
            finish(&mut fixture.database, job.id, Ok(()), &events)
                .await
                .map(|()| vec![])
        } else {
            tick(&mut fixture.database, &events, true)
                .await
                .map(|batch| {
                    assert!(!batch.pending);
                    assert_eq!(batch.next_ms, None);
                    batch.work.into_iter().map(|work| work.job).collect()
                })
        }
    };
    let peer_commit = async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while probe.last_insert_rowid() == -1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("dispatch did not write before waiting for publication");
        assert!(!probe.is_autocommit().unwrap());
        let result = conflict::commit(transaction).await;
        drop(publication);
        result
    };
    let (dispatched, committed) = tokio::join!(dispatch, peer_commit);
    let restored = writer.is_autocommit().unwrap();
    drop(writer);
    assert!(restored);
    assert!(fixture.database.connection.is_autocommit().unwrap());
    assert_eq!(
        fixture.database.writes.available_permits(),
        WRITE_CONCURRENCY
    );
    committed.expect("dispatch blocked an unrelated peer commit");
    assert_eq!(
        dispatched.unwrap(),
        if finishing { vec![] } else { vec![job.id] }
    );
    assert_eq!(fixture.database.peers().await.unwrap(), vec![peer]);
    let envelope = updates.try_recv().unwrap();
    assert_eq!(envelope.node, fixture.database.node);
    assert_eq!(envelope.cursor, prior as u64 + 1);
    assert_eq!(
        envelope.event,
        protocol::Event::DispatchChanged {
            packages: vec![fixture.package.name.clone()]
        }
    );
    assert_eq!(
        updates.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    );
    let page = jobs::list(
        &fixture.database.connection,
        &fixture.package.name,
        None,
        100,
    )
    .await
    .unwrap();
    assert_eq!(page.next_before, None);
    assert_eq!(page.jobs.len(), 1);
    let stored = &page.jobs[0];
    assert_eq!(stored.id, job.id);
    assert_eq!(
        stored.status,
        if finishing {
            Status::Completed
        } else {
            Status::Running
        }
    );
    assert!(stored.started_ms.is_some());
    assert_eq!(stored.finished_ms.is_some(), finishing);
    assert_eq!(stored.error, None);
}
