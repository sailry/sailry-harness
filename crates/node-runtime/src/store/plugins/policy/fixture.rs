use super::*;
use serde_json::json;

pub(in crate::store) async fn install(db: &Connection, name: &str) -> Info {
    let info: Info = serde_json::from_value(json!({
        "summary":{"name":name,"revision":1,"settings_revision":0,"enabled":true,"digest":"a".repeat(64),"version":"0.1.0"},
        "extension":{"api_version":"v1","scope":"host","actions":["agents.delegate"]},
        "settings":{"$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
            "properties":{"parallel":{"type":"string","default":"8","x-sailry-child-concurrency":true}},"required":["parallel"]}
    })).unwrap();
    check(db, &info).await.unwrap();
    super::super::save(db, &info, true).await.unwrap();
    let mut info = info;
    migrate(db, &mut info).await.unwrap();
    info
}

pub(in crate::store) async fn configure(db: &Connection, value: u64) {
    let transaction = db.unchecked_transaction().await.unwrap();
    let db = &transaction;
    let info = match owner(db).await.unwrap() {
        Some(info) => info,
        None => install(db, "example").await,
    };
    let mut state = settings::read(db, &info.summary.reference()).await.unwrap();
    state
        .values
        .insert("parallel".into(), json!(value.to_string()));
    settings::save(db, &state.package, &state.values, &Default::default())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}
