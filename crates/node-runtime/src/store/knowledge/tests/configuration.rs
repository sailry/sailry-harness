use super::*;

#[test]
fn keeps_model_settings_controller_owned() {
    let db = database();
    let owner = package(&db, "owner", 'a', &["storage.read"]);
    let previous = settings(&db, &request(Command::ReadLocalModelSettings)).unwrap();
    assert_eq!(previous, sailry_protocol::models::Settings::default());
    assert_eq!(
        settings(&db, &captured(&owner, Command::ReadLocalModelSettings))
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        execute(
            &db,
            &captured(&owner, Command::SaveLocalModelSettings(previous.clone()))
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    for mirror in [
        "http://models.example/",
        "https://user:secret@models.example/",
        "https://models.example/?token=secret",
        "https://models.example/#fragment",
    ] {
        let mut settings = previous.clone();
        settings.mirror = Some(mirror.into());
        assert_eq!(
            execute(&db, &request(Command::SaveLocalModelSettings(settings)))
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    assert_eq!(local_models::settings(&db).unwrap(), previous);
    let mut settings = previous.clone();
    settings.mirror = Some("https://models.example/public/models".into());
    let (Output::LocalModelSettings(saved), event) =
        execute(&db, &request(Command::SaveLocalModelSettings(settings))).unwrap()
    else {
        panic!("settings expected")
    };
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
    assert_eq!(local_models::settings(&db).unwrap(), saved);
}

#[test]
fn invalidates_only_collections_following_default() {
    let db = database();
    let owner = package(&db, "owner", 'a', &["storage.read"]);
    let following = insert(&db, &owner, storage());
    let mut definition = storage();
    definition.model = Some("bge-small-zh".to_owned());
    let pinned = insert(&db, &owner, definition);
    let mut settings = local_models::settings(&db).unwrap();
    select_model(&mut settings, Some("potion-multi".into()));
    execute(&db, &request(Command::SaveLocalModelSettings(settings))).unwrap();
    let snapshot = catalog(&db).unwrap();
    let following = &snapshot
        .collections
        .iter()
        .find(|stored| stored.collection.id == following.id)
        .unwrap()
        .collection;
    assert_eq!(following.revision, 2);
    assert_eq!(following.progress.generation, 2);
    assert_eq!(
        snapshot
            .collections
            .iter()
            .find(|stored| stored.collection.id == pinned.id)
            .unwrap()
            .collection,
        pinned
    );
    let original = raw(&db);
    let mut settings = local_models::settings(&db).unwrap();
    settings.mirror = Some("https://models.example/".into());
    execute(&db, &request(Command::SaveLocalModelSettings(settings))).unwrap();
    assert_eq!(raw(&db), original);
}

#[test]
fn guards_revision_generation_and_progress() {
    let db = database();
    let owner = package(&db, "owner", 'a', &["storage.read"]);
    let original = insert(&db, &owner, storage());
    let mut changed = original.definition.clone();
    changed.name = "Updated".into();
    let updated = create(
        &db,
        &captured(
            &owner,
            Command::UpdateKnowledgeCollection {
                id: original.id,
                expected_revision: 1,
                definition: changed,
            },
        ),
    );
    assert_eq!(updated.revision, 2);
    assert_eq!(updated.progress.generation, 2);
    let before = raw(&db);
    for command in [
        Command::RebuildKnowledgeCollection {
            id: original.id,
            expected_revision: 1,
        },
        Command::DeleteKnowledgeCollection {
            id: original.id,
            expected_revision: 1,
        },
    ] {
        assert_eq!(
            execute(&db, &captured(&owner, command)).unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(raw(&db), before);
    }
    assert_eq!(
        progress(&db, original.id, 1, &original.progress)
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let mut completed = updated.progress.clone();
    completed.status = knowledge::Status::Ready;
    completed.sources = 2;
    completed.completed = 2;
    completed.chunks = 4;
    completed.model = Some("potion-multi".to_owned());
    assert_eq!(
        progress(&db, original.id, 2, &completed).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    completed.model = Some("granite-multi".to_owned());
    assert!(matches!(
        progress(&db, original.id, 2, &completed).unwrap(),
        Some(Event::KnowledgeCollectionsChanged)
    ));
    assert!(progress(&db, original.id, 2, &completed).unwrap().is_none());
    let stored = &catalog(&db).unwrap().collections[0].collection;
    assert_eq!(stored.revision, updated.revision);
    assert_eq!(stored.progress, completed);
    let rebuilt = create(
        &db,
        &captured(
            &owner,
            Command::RebuildKnowledgeCollection {
                id: original.id,
                expected_revision: 2,
            },
        ),
    );
    assert_eq!(
        rebuilt.progress,
        knowledge::Progress {
            generation: 3,
            ..Default::default()
        }
    );
    assert_eq!(
        progress(&db, original.id, 2, &completed).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn rejects_exhaustion_before_partial_writes() {
    let db = database();
    let owner = package(&db, "owner", 'a', &["storage.read"]);
    insert(&db, &owner, storage());
    insert(&db, &owner, storage());
    let mut entries = catalog(&db).unwrap().collections;
    entries[1].collection.progress.generation = i64::MAX as u64;
    collections::persist(&db, &entries[1]).unwrap();
    let before = raw(&db);
    let previous = local_models::settings(&db).unwrap();
    let mut changed = previous.clone();
    select_model(&mut changed, None);
    assert_eq!(
        execute(&db, &request(Command::SaveLocalModelSettings(changed)))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(raw(&db), before);
    assert_eq!(local_models::settings(&db).unwrap(), previous);
    entries[0].collection.revision = i64::MAX as u64;
    collections::persist(&db, &entries[0]).unwrap();
    let before = raw(&db);
    let change = captured(
        &owner,
        Command::RebuildKnowledgeCollection {
            id: entries[0].collection.id,
            expected_revision: i64::MAX as u64,
        },
    );
    assert_eq!(
        execute(&db, &change).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(raw(&db), before);
}
