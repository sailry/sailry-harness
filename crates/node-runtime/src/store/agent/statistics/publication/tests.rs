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
    request(&fixture.database, fixture.session);
    assert_eq!(
        fixture
            .database
            .statistics
            .publication
            .pending
            .lock()
            .unwrap()
            .get(&fixture.session),
        Some(&true)
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
async fn requests_share_an_unmodified_flight() {
    let fixture = Fixture::new().await;
    let owner = crate::tasks::Supervisor::start(4);
    let cache = fixture.database.statistics.clone();
    cache.configure(owner.tasks.clone());
    let mut stream = fixture
        .database
        .feeds
        .subscribe(
            fixture.database.node,
            fixture.read(None, 20).await,
            None,
            Default::default(),
            sailry_link::CancellationToken::new(),
        )
        .unwrap();
    assert!(matches!(
        stream.next().await.unwrap(),
        Update::ConversationSnapshot(_)
    ));
    let (entered, release) = super::fixture::hold(&cache, fixture.session);
    request(&fixture.database, fixture.session);
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    let mut later = fixture
        .database
        .feeds
        .subscribe(
            fixture.database.node,
            fixture.read(None, 20).await,
            None,
            Default::default(),
            sailry_link::CancellationToken::new(),
        )
        .unwrap();
    let Update::ConversationSnapshot(snapshot) = later.next().await.unwrap() else {
        panic!("pending snapshot expected")
    };
    assert!(snapshot.statistics.is_none());
    request(&fixture.database, fixture.session);
    assert_eq!(
        cache
            .publication
            .pending
            .lock()
            .unwrap()
            .get(&fixture.session),
        Some(&false)
    );
    release.send(()).unwrap();
    let Update::ConversationFrame(frame) =
        tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("statistics frame expected")
    };
    assert!(matches!(frame.change, Change::Statistics(_)));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), later.next())
            .await
            .unwrap()
            .unwrap(),
        Update::ConversationFrame(frame)
    );
    owner.shutdown(Duration::from_secs(5)).await.unwrap();
    assert_eq!(
        cache
            .publication
            .reads
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}

#[tokio::test]
async fn reports_failures_and_recovers_without_reset() {
    let fixture = Fixture::new().await;
    let cache = fixture.database.statistics.clone();
    let mut stream = fixture
        .database
        .feeds
        .subscribe(
            fixture.database.node,
            fixture.read(None, 20).await,
            None,
            Default::default(),
            sailry_link::CancellationToken::new(),
        )
        .unwrap();
    let Update::ConversationSnapshot(snapshot) = stream.next().await.unwrap() else {
        panic!("conversation snapshot expected")
    };
    assert!(snapshot.statistics.is_none());
    let mut flight = Flight {
        cache: cache.clone(),
        feeds: fixture.database.feeds.clone(),
        node: fixture.database.node,
        session: fixture.session,
        finished: false,
    };
    let mut sequence = snapshot.sequence;
    for error in [
        Fault::new(ErrorCode::Busy, "statistics read is busy"),
        Fault::new(ErrorCode::Internal, "statistics read failed"),
    ] {
        cache
            .publication
            .pending
            .lock()
            .unwrap()
            .insert(fixture.session, false);
        flight.finished = false;
        assert!(finish(
            &mut flight,
            fixture.database.node,
            Err(error.clone())
        ));
        let Update::ConversationFrame(failed) = stream.next().await.unwrap() else {
            panic!("statistics failure frame expected")
        };
        assert_eq!(failed.sequence, sequence + 1);
        assert_eq!(failed.change, Change::StatisticsFailed(error));
        assert!(cache.publication.pending.lock().unwrap().is_empty());
        sequence = failed.sequence;
    }
    let (statistics, entry) =
        super::super::prepare(&cache, &fixture.database.connection, fixture.session)
            .await
            .unwrap();
    let expected = statistics.clone();
    cache
        .publication
        .pending
        .lock()
        .unwrap()
        .insert(fixture.session, false);
    flight.finished = false;
    assert!(finish(
        &mut flight,
        fixture.database.node,
        Ok((statistics, entry))
    ));
    let Update::ConversationFrame(recovered) = stream.next().await.unwrap() else {
        panic!("statistics recovery frame expected")
    };
    assert_eq!(recovered.sequence, sequence + 1);
    assert_eq!(recovered.change, Change::Statistics(Box::new(expected)));
    assert!(cache.publication.pending.lock().unwrap().is_empty());
}

#[tokio::test]
async fn reports_saturated_supervisor_without_reset() {
    let fixture = Fixture::new().await;
    let owner = crate::tasks::Supervisor::start(1);
    fixture.database.statistics.configure(owner.tasks.clone());
    let _occupied = owner.tasks.spawn(std::future::pending::<()>()).unwrap();
    let mut stream = fixture
        .database
        .feeds
        .subscribe(
            fixture.database.node,
            fixture.read(None, 20).await,
            None,
            Default::default(),
            sailry_link::CancellationToken::new(),
        )
        .unwrap();
    assert!(matches!(
        stream.next().await.unwrap(),
        Update::ConversationSnapshot(_)
    ));
    request(&fixture.database, fixture.session);
    let Update::ConversationFrame(frame) =
        tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("statistics admission failure frame expected")
    };
    assert!(matches!(frame.change, Change::StatisticsFailed(_)));
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
    assert!(matches!(
        owner.shutdown(Duration::ZERO).await,
        Err(crate::Error::ShutdownTimeout)
    ));
}

#[tokio::test]
async fn shutdown_releases_pending_reader() {
    let fixture = Fixture::new().await;
    let owner = crate::tasks::Supervisor::start(4);
    fixture.database.statistics.configure(owner.tasks.clone());
    let mut stream = fixture
        .database
        .feeds
        .subscribe(
            fixture.database.node,
            fixture.read(None, 20).await,
            None,
            Default::default(),
            sailry_link::CancellationToken::new(),
        )
        .unwrap();
    assert!(matches!(
        stream.next().await.unwrap(),
        Update::ConversationSnapshot(_)
    ));
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
    let Update::ConversationFrame(frame) =
        tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("interrupted statistics frame expected")
    };
    assert!(matches!(frame.change, Change::StatisticsFailed(_)));
}
