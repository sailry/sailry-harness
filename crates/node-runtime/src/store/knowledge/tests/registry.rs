use super::*;
use crate::store::database::Database;
use sailry_protocol::{ModelId, models::Definition as ModelDefinition};

fn metadata() -> ModelDefinition {
    let mut definition = crate::models::registry::catalog::definitions(&[])
        .unwrap()
        .remove("potion-multi")
        .unwrap()
        .as_ref()
        .clone();
    definition.name = "Registered static model".into();
    definition.source.repository = "owner/static-model".into();
    definition
}

async fn register(db: &Connection, definition: &ModelDefinition) -> String {
    let (output, event) = execute(
        db,
        &request(Command::RegisterLocalModel {
            definition: definition.clone(),
        }),
    )
    .await
    .unwrap();
    assert!(event.is_none());
    let Output::LocalModel { id: model } = output else {
        panic!("registered model expected")
    };
    assert!(sailry_protocol::models::valid_id(&model) && model.len() == 64);
    model
}

async fn rows(db: &Connection) -> Vec<(String, String, Vec<u8>)> {
    (async {
        let mut rows = db
            .prepare("SELECT id,digest,body FROM local_models ORDER BY id")
            .await
            .unwrap()
            .query(())
            .await?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await? {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))(&row);
            values.push(mapped);
        }
        Ok::<_, turso::Error>(values.into_iter())
    })
    .await
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

#[tokio::test]
async fn deduplicates_normalized_assets_without_replacing_metadata() {
    let db = database().await;
    let definition = metadata();
    let first = register(&db, &definition).await;
    let original = rows(&db).await;
    let mut reordered = definition.clone();
    reordered.assets.reverse();
    for asset in &mut reordered.assets {
        asset.source.revision.make_ascii_uppercase();
        asset.sha256.make_ascii_uppercase();
    }
    assert_eq!(register(&db, &reordered).await, first);
    assert_eq!(rows(&db).await, original);
    let catalog = crate::store::local_models::read(&db).await.unwrap();
    let mut normalized = definition.clone();
    normalized
        .assets
        .sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(
        catalog.definitions.get(&first).unwrap().as_ref(),
        &normalized
    );
    let mut different = definition;
    different.source.revision = "b".repeat(40);
    for asset in &mut different.assets {
        asset.source.revision = "b".repeat(40);
    }
    let second = register(&db, &different).await;
    assert_ne!(second, first);
    assert_eq!(rows(&db).await.len(), 2);
    assert_eq!(
        rows(&db)
            .await
            .iter()
            .find(|row| row.0 == original[0].0)
            .unwrap(),
        &original[0]
    );
}

#[tokio::test]
async fn separates_prefixes_in_immutable_model_identity() {
    let db = database().await;
    let definition = metadata();
    let plain = register(&db, &definition).await;
    let mut query = definition.clone();
    query.query_prefix = Some("query: ".into());
    let prefixed = register(&db, &query).await;
    assert_ne!(plain, prefixed);
    assert_eq!(register(&db, &query).await, prefixed);
    let mut document = query.clone();
    document.document_prefix = Some("passage: ".into());
    let documents = register(&db, &document).await;
    assert_ne!(prefixed, documents);
    assert_ne!(plain, documents);
    assert_eq!(rows(&db).await.len(), 3);
}

#[tokio::test]
async fn rejects_invalid_definitions_without_writes() {
    let db = database().await;
    register(&db, &metadata()).await;
    let previous = rows(&db).await;
    let mut traversal = metadata();
    traversal.assets[0].path = "../config.json".into();
    let mut unpinned = metadata();
    unpinned.assets[0].source.revision = "main".into();
    let mut duplicate = metadata();
    duplicate.assets[1].name = duplicate.assets[0].name.clone();
    let mut empty = metadata();
    empty.assets[0].bytes = 0;
    let mut checksum = metadata();
    checksum.assets[0].sha256 = "invalid-checksum".into();
    let mut speech = metadata();
    speech.format = sailry_protocol::models::Format::SenseVoice {};
    speech.assets[0].name = "tokens.txt".into();
    speech.assets[1].name = "model.onnx".into();
    speech.assets.pop();
    speech.validate().unwrap();
    for definition in [traversal, unpinned, duplicate, empty, checksum, speech] {
        assert_eq!(
            execute(&db, &request(Command::RegisterLocalModel { definition }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(rows(&db).await, previous);
    }
}

#[tokio::test]
async fn requires_model_install_grant() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().canonicalize().unwrap().join("node.turso");
    let node = NodeId([19; 32]);
    let db = Database::open(&path, node, None).await.unwrap();
    let owner = package(
        &db.connection,
        "owner",
        'a',
        &["knowledge.read", "knowledge.write"],
    )
    .await;
    let request = captured(
        &owner,
        Command::RegisterLocalModel {
            definition: metadata(),
        },
    );
    assert_eq!(
        db.check_plugin(node, &request).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert!(rows(&db.connection).await.is_empty());
    let owner = package(&db.connection, "owner", 'b', &["models.install"]).await;
    let request = captured(
        &owner,
        Command::RegisterLocalModel {
            definition: metadata(),
        },
    );
    db.check_plugin(node, &request).await.unwrap();
    let (Output::LocalModel { id: model }, event) =
        execute(&db.connection, &request).await.unwrap()
    else {
        panic!("registered model expected")
    };
    assert!(sailry_protocol::models::valid_id(&model) && model.len() == 64);
    assert!(event.is_none());
    assert_eq!(rows(&db.connection).await.len(), 1);
    db.close().await.unwrap();
}

#[tokio::test]
async fn accepts_registered_selections_with_cas() {
    let db = database().await;
    let model = register(&db, &metadata()).await;
    let previous = settings(&db, &request(Command::ReadLocalModelSettings))
        .await
        .unwrap();
    let mut selected = previous.clone();
    select_model(&mut selected, Some(model.clone()));
    let (Output::LocalModelSettings(saved), event) =
        execute(&db, &request(Command::SaveLocalModelSettings(selected)))
            .await
            .unwrap()
    else {
        panic!("settings expected")
    };
    assert_eq!(
        saved.choice(sailry_protocol::models::Kind::Embedding).model,
        Some(model.clone())
    );
    assert_eq!(saved.revision, 1);
    assert!(matches!(
        event,
        Some(Event::LocalModelSettingsChanged { revision: 1 })
    ));
    assert_eq!(
        execute(&db, &request(Command::SaveLocalModelSettings(previous)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let owner = package(&db, "owner", 'a', &["storage.read"]).await;
    let mut definition = storage();
    definition.model = Some(model.clone());
    let collection = insert(&db, &owner, definition).await;
    assert_eq!(collection.definition.model, Some(model.clone()));
    assert_eq!(local_models::settings(&db).await.unwrap(), saved);
}

#[tokio::test]
async fn refuses_unknown_selections_without_side_effects() {
    let db = database().await;
    let owner = package(&db, "owner", 'a', &["storage.read"]).await;
    let collection = insert(&db, &owner, storage()).await;
    let previous = local_models::settings(&db).await.unwrap();
    let unknown = format!("model-{}", ModelId::new());
    let mut selected = previous.clone();
    select_model(&mut selected, Some(unknown.clone()));
    let before = raw(&db).await;
    assert_eq!(
        execute(&db, &request(Command::SaveLocalModelSettings(selected)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let mut definition = collection.definition.clone();
    definition.model = Some(unknown.clone());
    for command in [
        Command::CreateKnowledgeCollection {
            definition: definition.clone(),
            plugin: None,
        },
        Command::UpdateKnowledgeCollection {
            id: collection.id,
            expected_revision: collection.revision,
            definition,
        },
    ] {
        assert_eq!(
            execute(&db, &captured(&owner, command))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(raw(&db).await, before);
    }
    assert_eq!(local_models::settings(&db).await.unwrap(), previous);
    assert!(rows(&db).await.is_empty());
}

#[tokio::test]
async fn restores_immutable_metadata_and_selection() {
    let profile = tempfile::tempdir().unwrap();
    let path = profile.path().canonicalize().unwrap().join("node.turso");
    let node = NodeId([37; 32]);
    let db = Database::open(&path, node, None).await.unwrap();
    let model = register(&db.connection, &metadata()).await;
    let mut settings = local_models::settings(&db.connection).await.unwrap();
    select_model(&mut settings, Some(model.clone()));
    execute(
        &db.connection,
        &request(Command::SaveLocalModelSettings(settings)),
    )
    .await
    .unwrap();
    let definitions = local_models::read(&db.connection)
        .await
        .unwrap()
        .definitions;
    let original = rows(&db.connection).await;
    db.close().await.unwrap();
    let db = Database::open(&path, node, None).await.unwrap();
    assert_eq!(rows(&db.connection).await, original);
    assert_eq!(
        local_models::read(&db.connection)
            .await
            .unwrap()
            .definitions,
        definitions
    );
    assert_eq!(
        local_models::settings(&db.connection)
            .await
            .unwrap()
            .choice(sailry_protocol::models::Kind::Embedding)
            .model,
        Some(model.clone())
    );
    assert_eq!(register(&db.connection, &definitions[&model]).await, model);
    assert_eq!(rows(&db.connection).await, original);
    db.close().await.unwrap();
}

#[tokio::test]
async fn refuses_modified_persisted_definition() {
    let db = database().await;
    register(&db, &metadata()).await;
    let mut row = rows(&db).await.remove(0);
    let mut changed: ModelDefinition = serde_json::from_slice(&row.2).unwrap();
    changed.name = "Unexpected replacement".into();
    row.2 = serde_json::to_vec(&changed).unwrap();
    db.execute(
        "UPDATE local_models SET body=?1 WHERE id=?2",
        params![row.2, row.0],
    )
    .await
    .unwrap();
    let before = rows(&db).await;
    assert!(crate::store::local_models::read(&db).await.is_err());
    assert_eq!(rows(&db).await, before);
}
