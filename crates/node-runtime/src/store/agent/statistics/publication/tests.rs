use super::*;
use crate::{
    agent::canonical,
    store::agent::{history, runs, tests::fixture::Fixture},
};
use std::{collections::BTreeSet, time::Duration};

fn response(turn: TurnId) -> canonical::Record {
    let mut record = crate::store::agent::tests::fixture::message(
        "assistant",
        canonical::Role::Assistant,
        vec![canonical::BlockKind::Text {
            text: "Committed reply".into(),
        }],
    );
    record.turn = turn;
    record.message_mut().unwrap().usage = Some(canonical::Usage {
        input: Some(10),
        output: Some(1),
        ..Default::default()
    });
    record
}

#[tokio::test]
async fn coalesces_committed_batches_after_entries() {
    let mut fixture = Fixture::new().await;
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    let history = fixture.read(None, 100).await;
    let mut stream = fixture
        .database
        .feeds
        .subscribe(
            fixture.database.node,
            history,
            Default::default(),
            Default::default(),
            sailry_link::CancellationToken::new(),
        )
        .unwrap();
    assert!(matches!(
        stream.next().await.unwrap(),
        Update::ConversationSnapshot(_)
    ));
    let owner = crate::tasks::Supervisor::start(4);
    fixture.database.statistics.configure(owner.tasks.clone());
    let (entered, release) = super::fixture::hold(&fixture.database.statistics, fixture.session);
    let mut expected = BTreeSet::new();
    for count in 1..=3 {
        let events = (0..count).map(|_| response(turn)).collect::<Vec<_>>();
        expected.extend(events.iter().map(|event| event.id.clone()));
        history::append_batch(
            &mut fixture.database,
            fixture.session,
            turn,
            events
                .into_iter()
                .map(|event| history::Input::Record(Box::new(event)))
                .collect(),
            &fixture.events,
        )
        .await
        .unwrap();
        if count == 1 {
            tokio::time::timeout(Duration::from_secs(5), entered.notified())
                .await
                .unwrap();
        }
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while !expected.is_empty() {
            let Update::ConversationFrame(frame) = stream.next().await.unwrap() else {
                panic!("committed frame expected")
            };
            assert!(!matches!(frame.change, Change::Statistics(_)));
            if let Change::Entry(entry) = frame.change {
                expected.remove(&entry.id);
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        fixture
            .database
            .statistics
            .publication
            .reads
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    release.send(()).unwrap();
    let actual = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Update::ConversationFrame(frame) = stream.next().await.unwrap()
                && let Change::Statistics(statistics) = frame.change
            {
                break statistics;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(actual.responses, 6);
    assert_eq!(actual.usage.as_ref().unwrap().input, 60);
    assert_eq!(
        *actual,
        super::super::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
    );
    owner.shutdown(Duration::from_secs(5)).await.unwrap();
    assert_eq!(
        fixture
            .database
            .statistics
            .publication
            .reads
            .load(std::sync::atomic::Ordering::Relaxed),
        2
    );
    assert!(
        fixture
            .database
            .statistics
            .publication
            .pending
            .lock()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn shutdown_releases_pending_reader() {
    let fixture = Fixture::new().await;
    let owner = crate::tasks::Supervisor::start(4);
    fixture.database.statistics.configure(owner.tasks.clone());
    let (entered, release) = super::fixture::hold(&fixture.database.statistics, fixture.session);
    publish(&fixture.database, fixture.session).await;
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    assert!(matches!(
        owner.shutdown(Duration::ZERO).await,
        Err(crate::Error::ShutdownTimeout)
    ));
    assert!(
        fixture
            .database
            .statistics
            .publication
            .pending
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(release.send(()).is_err());
}
