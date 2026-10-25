use super::*;
use crate::plugins::modes::tests::{config, package};
use sailry_protocol::WorkMode;
use serde_json::json;

async fn fixture() -> (Connection, plugin::Info) {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    let info = package("example", "review", false);
    super::super::save(&db, &info, true).await.unwrap();
    db.execute(
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
        params![
            info.summary.digest.as_str(),
            info.summary.name.as_str(),
            encode(&info).unwrap()
        ],
    )
    .await
    .unwrap();
    (db, info)
}

#[tokio::test]
async fn bounds_disabled_and_retired_modes() {
    let (db, mut info) = fixture().await;
    let mut config = config();
    config.mode = WorkMode::Plan;
    config.options =
        serde_json::from_value(json!({"example":{"mode":"review","proactive":true}})).unwrap();
    let original = config.clone();
    assert_eq!(
        validate(&db, &config).await.unwrap(),
        Some(info.summary.reference())
    );
    info.summary.enabled = false;
    super::super::save(&db, &info, true).await.unwrap();
    assert_eq!(
        validate(&db, &config).await.unwrap_err().code,
        ErrorCode::NotConfigured
    );
    clear(&db, &mut config).await.unwrap();
    assert_eq!(config.options["example"]["mode"], json!(""));
    assert_eq!(config.options["example"]["proactive"], json!(true));
    assert!(validate(&db, &config).await.unwrap().is_none());
    config = original.clone();
    super::super::save(&db, &info, false).await.unwrap();
    assert!(validate(&db, &config).await.unwrap().is_none());
    assert_eq!(config, original);
    info.summary.enabled = true;
    info.extension.as_mut().unwrap().conversation_modes.clear();
    super::super::save(&db, &info, true).await.unwrap();
    assert!(validate(&db, &config).await.unwrap().is_none());
    assert_eq!(config.mode, WorkMode::Plan);
    assert_eq!(config, original);
}

#[tokio::test]
async fn captures_mode_only_dependencies() {
    let (db, info) = fixture().await;
    let mut config = config();
    config.mode = WorkMode::Plan;
    config.options = serde_json::from_value(json!({"example":{"mode":"review"}})).unwrap();
    let references = crate::store::agent::plugins::configuration(&db, &config)
        .await
        .unwrap();
    assert_eq!(references, [info.summary.reference()]);
    let mut assistant = package("assistant", "unused", true);
    let extension = assistant.extension.as_mut().unwrap();
    extension.conversation_modes.clear();
    extension.desktop = Some(
        serde_json::from_value(json!({"conversations":[{
            "id":"help","resource":"plugin","context":"Help with this package","tools":[]
        }]}))
        .unwrap(),
    );
    super::super::save(&db, &assistant, true).await.unwrap();
    db.execute(
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
        params![
            assistant.summary.digest.as_str(),
            assistant.summary.name.as_str(),
            encode(&assistant).unwrap()
        ],
    )
    .await
    .unwrap();
    config.assistant = Some(plugin::conversation::Binding {
        package: assistant.summary.reference(),
        id: "help".into(),
    });
    let references = crate::store::agent::plugins::configuration(&db, &config)
        .await
        .unwrap();
    assert_eq!(
        references,
        [assistant.summary.reference(), info.summary.reference()]
    );
    let mut disabled = info.clone();
    disabled.summary.enabled = false;
    super::super::save(&db, &disabled, true).await.unwrap();
    let captured = crate::store::agent::plugins::packages(&db, &references)
        .await
        .unwrap();
    assert_eq!(
        crate::plugins::modes::selected(&captured, &config)
            .unwrap()
            .unwrap()
            .0
            .summary
            .reference(),
        info.summary.reference()
    );
    assert_eq!(
        crate::store::agent::plugins::configuration(&db, &config)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
    let original = config.assistant.clone();
    config
        .options
        .get_mut("example")
        .unwrap()
        .insert("mode".into(), json!(""));
    config.options.insert(
        "assistant".into(),
        serde_json::from_value(json!({"mode":"unused"})).unwrap(),
    );
    assistant.summary.revision += 1;
    assistant.summary.digest = "b".repeat(64);
    assistant.extension.as_mut().unwrap().conversation_modes = package("assistant", "unused", true)
        .extension
        .unwrap()
        .conversation_modes;
    super::super::save(&db, &assistant, true).await.unwrap();
    assert_eq!(
        crate::store::agent::plugins::configuration(&db, &config)
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(config.assistant, original);
}
