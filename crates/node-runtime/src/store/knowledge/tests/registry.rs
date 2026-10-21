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

fn register(db: &Connection, definition: &ModelDefinition) -> String {
    let (output, event) = execute(
        db,
        &request(Command::RegisterLocalModel {
            definition: definition.clone(),
        }),
    )
    .unwrap();
    assert!(event.is_none());
    let Output::LocalModel { id: model } = output else {
        panic!("registered model expected")
    };
    assert!(sailry_protocol::models::valid_id(&model) && model.len() == 64);
    model
}

fn rows(db: &Connection) -> Vec<(String, String, Vec<u8>)> {
    db.prepare("SELECT id,digest,body FROM local_models ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn deduplicates_normalized_assets_without_replacing_metadata() {
    let db = database();
    let definition = metadata();
    let first = register(&db, &definition);
    let original = rows(&db);
    let mut reordered = definition.clone();
    reordered.assets.reverse();
    for asset in &mut reordered.assets {
        asset.source.revision.make_ascii_uppercase();
        asset.sha256.make_ascii_uppercase();
    }
    assert_eq!(register(&db, &reordered), first);
    assert_eq!(rows(&db), original);
    let record = crate::store::local_models::read(&db)
        .unwrap()
        .records
        .remove(0);
    let mut normalized = definition.clone();
    normalized
        .assets
        .sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(record.id, first);
    assert_eq!(record.definition, normalized);
    let mut different = definition;
    different.source.revision = "b".repeat(40);
    for asset in &mut different.assets {
        asset.source.revision = "b".repeat(40);
    }
    let second = register(&db, &different);
    assert_ne!(second, first);
    assert_eq!(
        crate::store::local_models::read(&db).unwrap().records.len(),
        2
    );
    assert_eq!(
        rows(&db).iter().find(|row| row.0 == original[0].0).unwrap(),
        &original[0]
    );
}

#[test]
fn separates_prefixes_in_immutable_model_identity() {
    let db = database();
    let definition = metadata();
    let plain = register(&db, &definition);
    let mut query = definition.clone();
    query.query_prefix = Some("query: ".into());
    let prefixed = register(&db, &query);
    assert_ne!(plain, prefixed);
    assert_eq!(register(&db, &query), prefixed);
    let mut document = query.clone();
    document.document_prefix = Some("passage: ".into());
    let documents = register(&db, &document);
    assert_ne!(prefixed, documents);
    assert_ne!(plain, documents);
    assert_eq!(
        crate::store::local_models::read(&db).unwrap().records.len(),
        3
    );
}

#[test]
fn rejects_invalid_definitions_without_writes() {
    let db = database();
    register(&db, &metadata());
    let previous = rows(&db);
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
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(rows(&db), previous);
    }
}

#[test]
fn requires_model_install_grant() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("node.sqlite3");
    let node = NodeId([19; 32]);
    let db = Database::open(&path, node, None).unwrap();
    let owner = package(
        &db.connection,
        "owner",
        'a',
        &["knowledge.read", "knowledge.write"],
    );
    let request = captured(
        &owner,
        Command::RegisterLocalModel {
            definition: metadata(),
        },
    );
    assert_eq!(
        db.check_plugin(node, &request).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert!(rows(&db.connection).is_empty());
    let owner = package(&db.connection, "owner", 'b', &["models.install"]);
    let request = captured(
        &owner,
        Command::RegisterLocalModel {
            definition: metadata(),
        },
    );
    db.check_plugin(node, &request).unwrap();
    let (Output::LocalModel { id: model }, event) = execute(&db.connection, &request).unwrap()
    else {
        panic!("registered model expected")
    };
    assert!(sailry_protocol::models::valid_id(&model) && model.len() == 64);
    assert!(event.is_none());
    assert_eq!(rows(&db.connection).len(), 1);
    db.close().unwrap();
}

#[test]
fn accepts_registered_selections_with_cas() {
    let db = database();
    let model = register(&db, &metadata());
    let previous = settings(&db, &request(Command::ReadLocalModelSettings)).unwrap();
    let mut selected = previous.clone();
    select_model(&mut selected, Some(model.clone()));
    let (Output::LocalModelSettings(saved), event) =
        execute(&db, &request(Command::SaveLocalModelSettings(selected))).unwrap()
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
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let owner = package(&db, "owner", 'a', &["storage.read"]);
    let mut definition = storage();
    definition.model = Some(model.clone());
    let collection = insert(&db, &owner, definition);
    assert_eq!(collection.definition.model, Some(model.clone()));
    assert_eq!(local_models::settings(&db).unwrap(), saved);
}

#[test]
fn refuses_unknown_selections_without_side_effects() {
    let db = database();
    let owner = package(&db, "owner", 'a', &["storage.read"]);
    let collection = insert(&db, &owner, storage());
    let previous = local_models::settings(&db).unwrap();
    let unknown = format!("model-{}", ModelId::new());
    let mut selected = previous.clone();
    select_model(&mut selected, Some(unknown.clone()));
    let before = raw(&db);
    assert_eq!(
        execute(&db, &request(Command::SaveLocalModelSettings(selected)))
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
            execute(&db, &captured(&owner, command)).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(raw(&db), before);
    }
    assert_eq!(local_models::settings(&db).unwrap(), previous);
    assert!(rows(&db).is_empty());
}

#[test]
fn restores_immutable_metadata_and_selection() {
    let profile = tempfile::tempdir().unwrap();
    let path = profile.path().canonicalize().unwrap().join("node.sqlite3");
    let node = NodeId([37; 32]);
    let db = Database::open(&path, node, None).unwrap();
    let model = register(&db.connection, &metadata());
    let mut settings = local_models::settings(&db.connection).unwrap();
    select_model(&mut settings, Some(model.clone()));
    execute(
        &db.connection,
        &request(Command::SaveLocalModelSettings(settings)),
    )
    .unwrap();
    let records = local_models::read(&db.connection).unwrap().records;
    let original = rows(&db.connection);
    db.close().unwrap();
    let db = Database::open(&path, node, None).unwrap();
    assert_eq!(rows(&db.connection), original);
    assert_eq!(local_models::read(&db.connection).unwrap().records, records);
    assert_eq!(
        local_models::settings(&db.connection)
            .unwrap()
            .choice(sailry_protocol::models::Kind::Embedding)
            .model,
        Some(model.clone())
    );
    assert_eq!(register(&db.connection, &records[0].definition), model);
    assert_eq!(rows(&db.connection), original);
    db.close().unwrap();
}

#[test]
fn refuses_modified_persisted_definition() {
    let db = database();
    register(&db, &metadata());
    let mut row = rows(&db).remove(0);
    let mut changed: ModelDefinition = serde_json::from_slice(&row.2).unwrap();
    changed.name = "Unexpected replacement".into();
    row.2 = serde_json::to_vec(&changed).unwrap();
    db.execute(
        "UPDATE local_models SET body=?1 WHERE id=?2",
        params![row.2, row.0],
    )
    .unwrap();
    let before = rows(&db);
    assert!(crate::store::local_models::read(&db).is_err());
    assert_eq!(rows(&db), before);
}
