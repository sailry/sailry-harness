use super::*;

#[tokio::test]
async fn fills_missing_drivers_without_changing_settings() {
    let directory = tempfile::tempdir().unwrap();
    let database = crate::store::database::engine(&directory.path().join("node.turso"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&database).await.unwrap();
    db.execute(
        "CREATE TABLE search_settings(singleton INTEGER PRIMARY KEY,body BLOB NOT NULL)",
        (),
    )
    .await
    .unwrap();
    let mut original = Stored::default();
    original.settings.revision = 7;
    original.settings.fallback = false;
    original.settings.providers.remove(&Driver::Tinyfish);
    original
        .settings
        .providers
        .get_mut(&Driver::Tavily)
        .unwrap()
        .key_configured = true;
    original.credentials.insert(
        Driver::Tavily,
        Auth {
            key: Some(Secret::new("private-fixture-key".into())),
            basic: None,
        },
    );
    db.execute(
        "INSERT INTO search_settings VALUES(1,?1)",
        [encode(&original).unwrap()],
    )
    .await
    .unwrap();
    assert!(pending_defaults(&db).await.unwrap());
    apply_defaults(&db).await.unwrap();
    assert!(!pending_defaults(&db).await.unwrap());
    let actual = load(&db).await.unwrap();
    original
        .settings
        .providers
        .insert(Driver::Tinyfish, search::Config::default());
    assert_eq!(actual.settings, original.settings);
    assert_eq!(
        actual.credentials[&Driver::Tavily]
            .key
            .as_ref()
            .unwrap()
            .expose(),
        "private-fixture-key"
    );
    assert!(!actual.settings.providers[&Driver::Tinyfish].enabled);
}

#[tokio::test]
async fn invalid_defaults_leave_storage_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let database = crate::store::database::engine(&directory.path().join("node.turso"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&database).await.unwrap();
    db.execute(
        "CREATE TABLE search_settings(singleton INTEGER PRIMARY KEY,body BLOB NOT NULL)",
        (),
    )
    .await
    .unwrap();
    let mut original = Stored::default();
    original.settings.providers.remove(&Driver::Tinyfish);
    original
        .settings
        .providers
        .get_mut(&Driver::Tavily)
        .unwrap()
        .key_configured = true;
    let bytes = encode(&original).unwrap();
    db.execute("INSERT INTO search_settings VALUES(1,?1)", [bytes.clone()])
        .await
        .unwrap();
    assert!(apply_defaults(&db).await.is_err());
    let retained = db
        .prepare("SELECT body FROM search_settings")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get::<Vec<u8>>(0)
        .unwrap();
    assert_eq!(retained, bytes);
}
