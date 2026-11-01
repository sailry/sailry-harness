use super::*;
use serde_json::json;

/// A retired package that owned the child budget before the Node did.
pub(in crate::store) async fn install(db: &Connection, name: &str, value: &str) -> Info {
    let info: Info = serde_json::from_value(json!({
        "summary":{"name":name,"revision":1,"settings_revision":0,"enabled":true,"digest":"a".repeat(64),"version":"0.1.0"},
        "extension":{"api_version":"v1","scope":"host","actions":["agents.delegate"]},
        "settings":{"$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
            "properties":{"parallel":{"type":"string","default":"8","x-sailry-child-concurrency":true}},"required":["parallel"]}
    })).unwrap();
    super::super::save(db, &info, true).await.unwrap();
    let mut state = settings::read(db, &info.summary.reference()).await.unwrap();
    state.values.insert("parallel".into(), json!(value));
    settings::save(db, &state.package, &state.values, &Default::default())
        .await
        .unwrap();
    required(db, name).await.unwrap()
}
