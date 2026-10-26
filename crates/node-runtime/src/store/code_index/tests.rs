use super::*;

#[tokio::test]
async fn preserves_unavailable_service_configuration() {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let mut db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../schema.sql"))
        .await
        .unwrap();
    let info: sailry_protocol::plugin::Info = serde_json::from_value(serde_json::json!({
        "summary":{"name":"index-owner", "revision":1, "digest":"a", "settings_revision":0, "enabled":true},
        "extension":{"api_version":"v1", "enabled_service":"code_index", "min_sailry_version":"99.0.0"}
    })).unwrap();
    db.execute(
        "INSERT INTO plugins(name,revision,installed,body) VALUES('index-owner',1,1,?1)",
        [serde_json::to_vec(&info).unwrap()],
    )
    .await
    .unwrap();
    let configured = save(&db, &Settings::default(), 0).await.unwrap();
    assert!(configured.enabled);
    let runtime = initialize(&mut db).await.unwrap();
    assert!(!runtime.enabled);
    assert_eq!(settings(&db).await.unwrap(), configured);
    assert!(!effective(&db).await.unwrap().enabled);
}
