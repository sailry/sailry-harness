use super::*;
use serde_json::json;

async fn fixture() -> Connection {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    db.execute("BEGIN", ()).await.unwrap();
    db
}

async fn budget(db: &Connection) -> sailry_protocol::team::Settings {
    crate::store::teams::settings::read(db).await.unwrap()
}

#[tokio::test]
async fn imports_package_configuration_once() {
    let connection = fixture().await;
    let db = &connection;
    assert_eq!(budget(db).await.child_concurrency, 8);
    let info = super::fixture::install(db, "neutral-policy", "13").await;
    import(db, &info).await.unwrap();
    let imported = budget(db).await;
    assert_eq!((imported.revision, imported.child_concurrency), (1, 13));
    let mut changed = imported.clone();
    changed.child_concurrency = 5;
    crate::store::teams::settings::save(db, &changed)
        .await
        .unwrap();
    import(db, &info).await.unwrap();
    let current = budget(db).await;
    assert_eq!((current.revision, current.child_concurrency), (2, 5));
}

#[tokio::test]
async fn prefers_package_over_retained_value() {
    let connection = fixture().await;
    let db = &connection;
    db.execute(
        "INSERT INTO background_settings(singleton,body) VALUES(1,?1)",
        [encode(&json!({"revision":7,"resume":false,"child_concurrency":u64::MAX})).unwrap()],
    )
    .await
    .unwrap();
    let info = super::fixture::install(db, "neutral-policy", "3").await;
    import(db, &info).await.unwrap();
    retained(db).await.unwrap();
    assert_eq!(budget(db).await.child_concurrency, 3);
}

#[tokio::test]
async fn imports_retained_value_without_a_package() {
    let connection = fixture().await;
    let db = &connection;
    retained(db).await.unwrap();
    assert_eq!(budget(db).await, Default::default());
    db.execute(
        "INSERT INTO background_settings(singleton,body) VALUES(1,?1)",
        [encode(&json!({"revision":7,"resume":false,"child_concurrency":u64::MAX})).unwrap()],
    )
    .await
    .unwrap();
    retained(db).await.unwrap();
    assert_eq!(budget(db).await.child_concurrency, u64::MAX);
}

#[tokio::test]
async fn ignores_packages_without_a_budget() {
    let connection = fixture().await;
    let db = &connection;
    let mut info = super::fixture::install(db, "neutral-policy", "4").await;
    info.settings
        .as_mut()
        .unwrap()
        .properties
        .get_mut("parallel")
        .unwrap()
        .child_concurrency = false;
    import(db, &info).await.unwrap();
    assert_eq!(budget(db).await, Default::default());
}
