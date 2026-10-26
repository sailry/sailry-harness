use super::super::tests::{START, event, query};
use super::*;
use crate::store::agent::tests::fixture::Fixture;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn reuses_unchanged_usage() {
    let mut fixture = Fixture::new().await;
    fixture.append([event(START, 10)]).await;
    let cache = &fixture.database.usage;
    let first = cache
        .read(
            &fixture.database.connection,
            fixture.database.node,
            &query(),
        )
        .await
        .unwrap();
    let second = cache
        .read(
            &fixture.database.connection,
            fixture.database.node,
            &query(),
        )
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(cache.scans.load(Ordering::Relaxed), 1);
    fixture.completed(&["Unmetered output"]).await;
    let third = fixture
        .database
        .usage
        .read(
            &fixture.database.connection,
            fixture.database.node,
            &query(),
        )
        .await
        .unwrap();
    assert_eq!(first.totals, third.totals);
    assert!(third.cursor > first.cursor);
    assert_eq!(fixture.database.usage.scans.load(Ordering::Relaxed), 1);
    fixture.append([event(START + DAY, 20)]).await;
    let updated = fixture
        .database
        .usage
        .read(
            &fixture.database.connection,
            fixture.database.node,
            &query(),
        )
        .await
        .unwrap();
    assert_eq!(updated.totals.responses, 2);
    assert_eq!(fixture.database.usage.scans.load(Ordering::Relaxed), 2);
    let mut filtered = query();
    filtered.models = vec!["other".into()];
    let filtered = fixture
        .database
        .usage
        .read(
            &fixture.database.connection,
            fixture.database.node,
            &filtered,
        )
        .await
        .unwrap();
    assert_eq!(filtered.totals.responses, 0);
    assert_eq!(fixture.database.usage.scans.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn invalidates_pricing_and_metadata() {
    let mut fixture = Fixture::new().await;
    fixture.append([event(START, 10)]).await;
    let database = &fixture.database;
    database
        .usage
        .read(&database.connection, database.node, &query())
        .await
        .unwrap();
    database
        .connection
        .execute(
            "INSERT INTO model_catalog_status(id,body) VALUES(1,?1)",
            [
                serde_json::to_vec(&sailry_protocol::conversation::catalog::Status {
                    revision: 1,
                    ..Default::default()
                })
                .unwrap(),
            ],
        )
        .await
        .unwrap();
    database
        .usage
        .read(&database.connection, database.node, &query())
        .await
        .unwrap();
    assert_eq!(database.usage.scans.load(Ordering::Relaxed), 2);
    crate::store::database::record(
        &database.connection,
        database.node,
        vec![Event::SessionsRemoved(Vec::new())],
    )
    .await
    .unwrap();
    database
        .usage
        .read(&database.connection, database.node, &query())
        .await
        .unwrap();
    assert_eq!(database.usage.scans.load(Ordering::Relaxed), 3);
}
