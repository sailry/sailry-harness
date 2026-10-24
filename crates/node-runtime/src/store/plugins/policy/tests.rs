use super::*;
use sailry_protocol::Secret;
use serde_json::json;

async fn fixture() -> Connection {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    db
}

#[tokio::test]
async fn retains_previous_values_and_ignores_absent_owners() {
    let connection = fixture().await;
    let db = &connection;
    let original = json!({"revision":7,"resume":false,"child_concurrency":u64::MAX});
    db.execute(
        "INSERT INTO background_settings(singleton,body) VALUES(1,?1)",
        [encode(&original).unwrap()],
    )
    .await
    .unwrap();
    assert_eq!(child_limit(db).await.unwrap(), None);
    assert_eq!(
        crate::store::background_settings::retained(db)
            .await
            .unwrap(),
        Some(u64::MAX)
    );
    let mut info = super::fixture::install(db, "neutral-policy").await;
    assert_eq!(child_limit(db).await.unwrap(), Some(u64::MAX));
    let frozen = settings::read(db, &info.summary.reference()).await.unwrap();
    assert_eq!(frozen.values["parallel"], json!(u64::MAX.to_string()));
    super::fixture::configure(db, 3).await;
    info = required(db, "neutral-policy").await.unwrap();
    migrate(db, &mut info).await.unwrap();
    assert_eq!(child_limit(db).await.unwrap(), Some(3));
    assert_eq!(
        settings::read_admitted(
            db,
            &Info {
                summary: plugin::Summary {
                    settings_revision: frozen.package.settings_revision,
                    ..info.summary.clone()
                },
                ..info.clone()
            }
        )
        .await
        .unwrap()
        .values,
        frozen.values
    );
    assert_eq!(
        crate::store::background_settings::retained(db)
            .await
            .unwrap(),
        Some(u64::MAX)
    );
    info.summary.enabled = false;
    super::super::save(db, &info, true).await.unwrap();
    assert_eq!(child_limit(db).await.unwrap(), Some(3));
    super::super::save(db, &info, false).await.unwrap();
    assert_eq!(child_limit(db).await.unwrap(), None);
}

#[tokio::test]
async fn prevents_competing_policy_owners() {
    let connection = fixture().await;
    let db = &connection;
    let mut info = super::fixture::install(db, "neutral-policy").await;
    info.summary.name = "another-policy".into();
    assert_eq!(
        check(db, &info).await.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    info.summary.name = "neutral-policy".into();
    check(db, &info).await.unwrap();
    info.extension.as_mut().unwrap().scope = plugin::Scope::Desktop;
    assert_eq!(
        check(db, &info).await.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[tokio::test]
async fn preserves_unrelated_secret_bindings_during_migration() {
    let connection = fixture().await;
    let db = &connection;
    let mut info = super::fixture::install(db, "neutral-policy").await;
    let original: Field = serde_json::from_value(
        json!({"type":"string","x-sailry-secret":{"server":"original","header":"authorization"}}),
    )
    .unwrap();
    let mut saved = settings::stored(db, &info).await.unwrap();
    saved.values.clear();
    let previous = saved.schema.as_mut().unwrap();
    previous.properties.clear();
    previous.required = vec![];
    previous.properties.insert("token".into(), original.clone());
    saved.slots.insert(
        "token".into(),
        Some(Secret::new("synthetic-private-value".into())),
    );
    settings::persist(db, &mut info, &saved).await.unwrap();
    let mut replacement = original.clone();
    replacement.secret.as_mut().unwrap().server = Some("replacement".into());
    info.settings
        .as_mut()
        .unwrap()
        .properties
        .insert("token".into(), replacement);
    super::super::save(db, &info, true).await.unwrap();
    db.execute(
        "INSERT INTO background_settings(singleton,body) VALUES(1,?1)",
        [encode(&json!({"revision":1,"resume":true,"child_concurrency":13})).unwrap()],
    )
    .await
    .unwrap();
    migrate(db, &mut info).await.unwrap();
    let migrated = settings::stored(db, &info).await.unwrap();
    assert_eq!(migrated.schema.unwrap().properties["token"], original);
    assert_eq!(migrated.slots, saved.slots);
    assert_eq!(migrated.values["parallel"], json!("13"));
}
