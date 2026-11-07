use super::*;
use crate::models::registry::files::probe;
use std::{path::Path, sync::mpsc};
use tokio::time::{Duration, timeout};

fn hashed(path: &Path) -> usize {
    probe::HASHED
        .lock()
        .unwrap()
        .iter()
        .filter(|hashed| *hashed == path)
        .count()
}

fn restart(fixture: &Fixture) -> Arc<Registry> {
    let owner = Registry::new(
        Some(fixture._profile.path().into()),
        CancellationToken::new(),
    );
    owner
        .configure(
            fixture.owner.settings(),
            std::slice::from_ref(&fixture.record),
        )
        .unwrap();
    owner
}

async fn state(owner: &Arc<Registry>, id: &str) -> State {
    owner
        .models(None)
        .await
        .unwrap()
        .into_iter()
        .find(|model| model.id == id)
        .unwrap()
        .state
}

#[tokio::test]
async fn reuses_installed_and_recorded_stamps() {
    let fixture = Fixture::new().await;
    assert_eq!(
        state(&fixture.owner, &fixture.record.id).await,
        State::Installed
    );
    assert_eq!(hashed(&fixture.path), 0);
    let restarted = restart(&fixture);
    assert_eq!(
        state(&restarted, &fixture.record.id).await,
        State::Installed
    );
    assert_eq!(hashed(&fixture.path), 0);
    restarted.shutdown().await;
    // Without a receipt, one start hashes the files and records them again.
    std::fs::remove_file(fixture.path.join(".verified")).unwrap();
    let restarted = restart(&fixture);
    assert_eq!(
        state(&restarted, &fixture.record.id).await,
        State::Installed
    );
    assert_eq!(hashed(&fixture.path), 1);
    restarted.shutdown().await;
    let restarted = restart(&fixture);
    assert_eq!(
        state(&restarted, &fixture.record.id).await,
        State::Installed
    );
    assert_eq!(hashed(&fixture.path), 1);
    restarted.shutdown().await;
    fixture.owner.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn hashes_outside_configuration_lock() {
    let fixture = Fixture::new().await;
    std::fs::remove_file(fixture.path.join(".verified")).unwrap();
    let (entered, entering) = mpsc::channel();
    let (release, waiting) = mpsc::channel();
    *probe::GATE.lock().unwrap() = Some((fixture.path.clone(), entered, waiting));
    let restarted = Registry::new(
        Some(fixture._profile.path().into()),
        CancellationToken::new(),
    );
    let updates = restarted.subscribe();
    let owner = restarted.clone();
    let settings = fixture.owner.settings();
    let record = fixture.record.clone();
    let listing = tokio::task::spawn_blocking(move || {
        owner
            .configure(settings, std::slice::from_ref(&record))
            .unwrap();
        owner.list(None)
    });
    tokio::task::spawn_blocking(move || entering.recv_timeout(Duration::from_secs(10)))
        .await
        .unwrap()
        .unwrap();
    let owner = restarted.clone();
    let mut settings = fixture.owner.settings();
    settings.revision += 1;
    let record = fixture.record.clone();
    let configured = tokio::task::spawn_blocking(move || {
        owner.configure(settings, std::slice::from_ref(&record))?;
        Ok::<_, Fault>(owner.cached(Some(Kind::Embedding)))
    });
    let models = timeout(Duration::from_secs(5), configured)
        .await
        .expect("configuration waited for hashing")
        .unwrap()
        .unwrap();
    assert!(models.iter().all(|model| model.state == State::Missing));
    release.send(()).unwrap();
    assert!(
        timeout(Duration::from_secs(10), listing)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .iter()
            .any(|model| model.id == fixture.record.id && model.state == State::Installed)
    );
    assert!(
        updates
            .borrow()
            .models
            .iter()
            .any(|model| model.id == fixture.record.id && model.state == State::Installed)
    );
    restarted.shutdown().await;
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn publishes_files_found_after_registration() {
    let fixture = Fixture::new().await;
    let restarted = Registry::new(
        Some(fixture._profile.path().into()),
        CancellationToken::new(),
    );
    let mut updates = restarted.subscribe();
    restarted
        .configure(
            fixture.owner.settings(),
            std::slice::from_ref(&fixture.record),
        )
        .unwrap();
    timeout(
        Duration::from_secs(10),
        updates.wait_for(|update| {
            update
                .models
                .iter()
                .any(|model| model.id == fixture.record.id && model.state == State::Installed)
        }),
    )
    .await
    .unwrap()
    .unwrap();
    restarted.shutdown().await;
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn publishes_external_deletion() {
    let fixture = Fixture::new().await;
    let resource = fixture
        .owner
        .resource(Kind::Embedding, &fixture.record.id, None)
        .unwrap();
    std::fs::remove_file(fixture.path.join("config.json")).unwrap();
    assert_eq!(
        fixture
            .owner
            .execute(
                resource,
                Lane::Interactive,
                &CancellationToken::new(),
                |_, _| Ok(())
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert!(
        fixture
            .owner
            .subscribe()
            .borrow()
            .models
            .iter()
            .any(|model| model.id == fixture.record.id && model.state == State::Missing)
    );
    assert_eq!(
        fixture
            .owner
            .resource(Kind::Embedding, &fixture.record.id, None)
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
    fixture.owner.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn executes_read_only_imported_models() {
    use std::os::unix::fs::PermissionsExt;

    let profile = tempfile::tempdir().unwrap();
    let owner = Registry::new(Some(profile.path().into()), CancellationToken::new());
    let (mut record, contents) = record(2, &[100., 0., 1., 0., 0., 1.]);
    record.id = "model-imported".into();
    record.definition.assets.reverse();
    let path = files::directory(
        &files::root(profile.path()).unwrap(),
        &record.id,
        &record.definition,
    )
    .unwrap();
    std::fs::create_dir_all(&path).unwrap();
    for (name, body) in contents {
        std::fs::write(path.join(name), body).unwrap();
    }
    let previous = blake3::hash(&serde_json::to_vec(&record.definition).unwrap())
        .to_hex()
        .to_string();
    assert_ne!(previous, catalog::id(&record.definition));
    sailry_model_assets::verified::write_marker(
        &crate::files::path::root(&path).unwrap(),
        &previous,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
    owner
        .configure(Settings::default(), std::slice::from_ref(&record))
        .unwrap();
    let result = async {
        let models = owner.models(None).await?;
        let resource = owner.resource(Kind::Embedding, &record.id, None)?;
        let vectors = owner
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
                        .map_err(super::super::super::runtime::embedding_error)
                },
            )
            .await?;
        Ok::<_, Fault>((models, vectors))
    }
    .await;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (models, vectors) = result.unwrap();
    assert!(
        models
            .iter()
            .any(|model| model.id == record.id && model.state == State::Installed)
    );
    assert_eq!(vectors, [vec![1., 0.]]);
    owner.shutdown().await;
}
