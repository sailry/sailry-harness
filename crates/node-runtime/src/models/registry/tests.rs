use super::*;
use crate::models::{
    runtime::Lane,
    test_support::{Fixture, record},
};

mod ocr;
mod scopes;

#[tokio::test]
async fn exposes_catalog_without_acquiring_models() {
    let profile = tempfile::tempdir().unwrap();
    let owner = Registry::new(Some(profile.path().into()), CancellationToken::new());
    let models = owner.models(None).await.unwrap();
    assert_eq!(models.len(), 6);
    assert!(models.iter().all(|model| model.state == State::Missing));
    for definition in catalog::ocr() {
        definition.validate().unwrap();
        assert_eq!(definition.assets.len(), 4);
        let mut reordered = definition.clone();
        reordered.assets.reverse();
        assert_eq!(catalog::id(&reordered), catalog::id(definition));
    }
    assert!(owner.remove("../outside").await.is_err());
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(
        owner
            .install("granite-multi", &stop, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert!(owner.state.lock().unwrap().transfers.is_empty());
    owner.shutdown().await;
}

#[tokio::test]
async fn preserves_unrelated_settings_and_rejects_replaced_files() {
    let fixture = Fixture::new().await;
    let captured = fixture
        .owner
        .resource(Kind::Embedding, &fixture.record.id, None)
        .unwrap();
    fixture.owner.current(&captured).unwrap();
    let mut settings = fixture.owner.settings();
    settings.revision = 1;
    fixture
        .owner
        .configure(settings, std::slice::from_ref(&fixture.record))
        .unwrap();
    fixture.owner.current(&captured).unwrap();
    let captured = fixture
        .owner
        .resource(Kind::Embedding, &fixture.record.id, None)
        .unwrap();
    let path = fixture.path.join("config.json");
    std::fs::write(path.with_extension("new"), std::fs::read(&path).unwrap()).unwrap();
    std::fs::rename(path.with_extension("new"), &path).unwrap();
    assert_eq!(
        fixture
            .owner
            .execute::<()>(
                captured.clone(),
                Lane::Interactive,
                &CancellationToken::new(),
                |_, _| panic!("changed model must not execute")
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    // A new capture may use the replacement; previous captures remain stale.
    fixture.owner.models(None).await.unwrap();
    assert_eq!(
        fixture
            .owner
            .execute::<()>(
                captured.clone(),
                Lane::Interactive,
                &CancellationToken::new(),
                |_, _| panic!("changed model must not execute")
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn rejects_same_size_asset_changes() {
    let fixture = Fixture::new().await;
    let captured = fixture
        .owner
        .resource(Kind::Embedding, &fixture.record.id, None)
        .unwrap();
    let path = fixture.path.join("model.safetensors");
    let mut contents = std::fs::read(&path).unwrap();
    *contents.last_mut().unwrap() ^= 1;
    std::fs::write(path, contents).unwrap();
    let models = fixture.owner.models(None).await.unwrap();
    assert_eq!(
        models
            .iter()
            .find(|model| model.id == fixture.record.id)
            .unwrap()
            .state,
        State::Missing
    );
    assert_eq!(
        fixture
            .owner
            .resource(Kind::Embedding, &fixture.record.id, None)
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
    assert_eq!(
        fixture
            .owner
            .execute(
                captured,
                Lane::Interactive,
                &CancellationToken::new(),
                |_, _| Ok(())
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn preserves_explicit_embedding_override_when_disabled() {
    let fixture = Fixture::new().await;
    let mut settings = fixture.owner.settings();
    settings.choices.insert(
        Kind::Embedding,
        sailry_protocol::models::Choice {
            enabled: false,
            model: None,
        },
    );
    settings.revision = 1;
    fixture
        .owner
        .configure(settings, std::slice::from_ref(&fixture.record))
        .unwrap();
    let resource = fixture
        .owner
        .resource(Kind::Embedding, &fixture.record.id, None)
        .unwrap();
    let vectors = fixture
        .owner
        .execute(
            resource,
            Lane::Interactive,
            &CancellationToken::new(),
            |engine, _| {
                let Engine::Embedding(engine) = engine else {
                    panic!("embedding engine expected")
                };
                engine
                    .encode_for(sailry_embedding::Input::Query, &["alpha"])
                    .map_err(super::super::runtime::embedding_error)
            },
        )
        .await
        .unwrap();
    assert_eq!(vectors, [vec![1., 0.]]);
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn runs_synchronous_calls_on_current_thread_executor() {
    let fixture = Fixture::new().await;
    let resource = fixture
        .owner
        .resource(Kind::Embedding, &fixture.record.id, None)
        .unwrap();
    let offsets = fixture
        .owner
        .blocking(
            resource,
            Lane::Interactive,
            &CancellationToken::new(),
            |engine, _| {
                let Engine::Embedding(engine) = engine else {
                    panic!("embedding engine expected")
                };
                engine
                    .offsets("alpha beta")
                    .map_err(super::super::runtime::embedding_error)
            },
        )
        .unwrap();
    assert_eq!(offsets, [(0, 5), (6, 10)]);
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn removes_owned_files_and_invalidates_queued_captures() {
    let fixture = Fixture::new().await;
    let captured = fixture
        .owner
        .resource(Kind::Embedding, &fixture.record.id, None)
        .unwrap();
    let other = fixture.path.parent().unwrap().join("another-revision");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("keep"), b"owned elsewhere").unwrap();
    fixture.owner.remove(&fixture.record.id).await.unwrap();
    assert!(!fixture.path.exists());
    assert!(other.join("keep").is_file());
    let error = fixture
        .owner
        .execute(
            captured,
            Lane::Background,
            &CancellationToken::new(),
            |_, _| Ok(()),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    assert!(fixture.owner.state.lock().unwrap().removing.is_empty());
    fixture.owner.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_symlinked_model_directories() {
    let fixture = Fixture::new().await;
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("keep"), b"external").unwrap();
    std::fs::remove_dir_all(&fixture.path).unwrap();
    std::os::unix::fs::symlink(outside.path(), &fixture.path).unwrap();
    assert!(fixture.owner.remove(&fixture.record.id).await.is_err());
    assert_eq!(
        std::fs::read(outside.path().join("keep")).unwrap(),
        b"external"
    );
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn keeps_definitions_immutable_and_updates_quiet() {
    let fixture = Fixture::new().await;
    let settings = fixture.owner.settings();
    let updates = fixture.owner.subscribe();
    fixture
        .owner
        .configure(settings.clone(), std::slice::from_ref(&fixture.record))
        .unwrap();
    assert!(!updates.has_changed().unwrap());
    let mut changed = fixture.record.clone();
    changed.definition.name = "Changed".into();
    assert!(fixture.owner.configure(settings, &[changed]).is_err());
    assert!(!updates.has_changed().unwrap());
    fixture.owner.shutdown().await;
}

#[test]
fn preserves_builtin_capacity_beside_registered_models() {
    let (record, _) = record(2, &[100., 0., 1., 0., 0., 1.]);
    let records = (0..128)
        .map(|number| Registration {
            id: format!("model-{number}"),
            ..record.clone()
        })
        .collect::<Vec<_>>();
    assert_eq!(catalog::definitions(&records).unwrap().len(), 134);
    let mut excessive = records;
    excessive.push(Registration {
        id: "model-last".into(),
        ..record
    });
    assert!(catalog::definitions(&excessive).is_err());
}
