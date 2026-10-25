use super::*;

#[tokio::test]
async fn validates_retired_metadata_without_rewriting() {
    let fixture = Fixture::new().await;
    let connection = fixture.storage.handle.lease(false).await.unwrap();
    let legacy: sailry_protocol::plugin::Info = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/legacy-code.json"
    )))
    .unwrap();
    let body = serde_json::to_vec(&legacy).unwrap();
    connection
        .execute(
            "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
            params![
                legacy.summary.digest.as_str(),
                legacy.summary.name.as_str(),
                body.clone()
            ],
        )
        .await
        .unwrap();
    crate::store::database::validate_backup(&connection, fixture.storage.node)
        .await
        .unwrap();
    let saved: Vec<u8> = connection
        .prepare("SELECT body FROM plugin_packages WHERE digest=?1")
        .await
        .unwrap()
        .query_row([legacy.summary.digest.as_str()])
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(saved, body);
    let extension = legacy.extension.unwrap();
    assert_eq!(
        extension.enabled_service,
        Some(sailry_protocol::plugin::Service::Language)
    );
    assert!(extension.actions.iter().all(|action| !action.current()));
    assert!(!extension.tools[0].operation.unwrap().current());
    drop(connection);
    fixture.close().await;
}
