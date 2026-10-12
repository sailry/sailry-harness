use super::*;

#[test]
fn catalog_has_immutable_complete_assets() {
    let definitions = catalog::recommended();
    assert_eq!(definitions.len(), 3);
    for definition in definitions {
        definition.validate().unwrap();
        assert_eq!(definition.assets.len(), 4);
        assert!(
            definition
                .assets
                .iter()
                .all(|asset| asset.source.revision.len() == 40)
        );
        let id = catalog::id(definition);
        assert!(sailry_protocol::ocr::valid_id(&id));
        let mut reordered = definition.clone();
        reordered.assets.reverse();
        assert_eq!(catalog::id(&reordered), id);
    }
}

#[tokio::test]
async fn starts_without_loading_or_downloading() {
    let profile = tempfile::tempdir().unwrap();
    let owner = Service::new(Some(profile.path().into()), CancellationToken::new());
    assert!(
        owner
            .models()
            .await
            .unwrap()
            .iter()
            .all(|model| model.state == State::Missing)
    );
    assert!(owner.engine.lock().unwrap().is_none());
    assert_eq!(owner.signature(), [0; 32]);
    let image = image::RgbImage::new(2, 2);
    assert_eq!(
        owner
            .recognize(&image, &CancellationToken::new())
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
    owner.shutdown();
}

#[tokio::test]
async fn rejects_unregistered_selection_and_unsafe_ids() {
    let profile = tempfile::tempdir().unwrap();
    let owner = Service::new(Some(profile.path().into()), CancellationToken::new());
    assert!(
        owner
            .configure(
                Settings {
                    model: Some("0".repeat(64)),
                    ..Settings::default()
                },
                &[]
            )
            .is_err()
    );
    assert!(owner.remove("../outside").await.is_err());
    assert!(
        owner
            .install("../outside", None, &CancellationToken::new(), None)
            .await
            .is_err()
    );
    owner.shutdown();
}

#[tokio::test]
async fn refuses_unverified_files() {
    let profile = tempfile::tempdir().unwrap();
    let owner = Service::new(Some(profile.path().into()), CancellationToken::new());
    let definition = catalog::recommended()[0].clone();
    let directory = owner.root().unwrap().join(catalog::id(&definition));
    std::fs::create_dir(&directory).unwrap();
    for asset in &definition.assets {
        std::fs::File::create(directory.join(&asset.name))
            .unwrap()
            .set_len(asset.bytes)
            .unwrap();
    }
    assert!(files::candidate(&directory, &definition).is_some());
    assert!(files::stamp(&directory, &definition).is_none());
    assert!(files::verified(&directory, &definition).is_none());
    assert!(
        owner
            .models()
            .await
            .unwrap()
            .iter()
            .all(|model| model.state == State::Missing)
    );
    owner.shutdown();
}

fn seed(owner: &Service) -> (Definition, PathBuf) {
    use sha2::{Digest, Sha256};
    let mut definition = catalog::recommended()[0].clone();
    definition.name = "Unvalidated fixture".into();
    for asset in &mut definition.assets {
        let bytes = if asset.name == "dictionary.txt" {
            b"\n".as_slice()
        } else {
            b"unvalidated graph".as_slice()
        };
        asset.bytes = bytes.len() as u64;
        asset.sha256 = format!("{:x}", Sha256::digest(bytes));
    }
    let directory = owner.root().unwrap().join(catalog::id(&definition));
    std::fs::create_dir(&directory).unwrap();
    for asset in &definition.assets {
        std::fs::write(
            directory.join(&asset.name),
            if asset.name == "dictionary.txt" {
                b"\n".as_slice()
            } else {
                b"unvalidated graph".as_slice()
            },
        )
        .unwrap();
    }
    (definition, directory)
}

#[tokio::test]
async fn validates_exact_preseeded_assets_before_installing() {
    let profile = tempfile::tempdir().unwrap();
    let owner = Service::new(Some(profile.path().into()), CancellationToken::new());
    let (definition, directory) = seed(&owner);
    let id = catalog::id(&definition);
    owner
        .configure(Settings::default(), std::slice::from_ref(&definition))
        .unwrap();
    assert!(files::candidate(&directory, &definition).is_some());
    assert!(files::verified(&directory, &definition).is_none());
    assert_eq!(
        owner
            .models()
            .await
            .unwrap()
            .into_iter()
            .find(|model| model.id == id)
            .unwrap()
            .state,
        State::Missing
    );
    let failure = owner
        .install(&id, None, &CancellationToken::new(), None)
        .await
        .unwrap_err();
    assert!(
        failure.message.contains("OCR dictionary is invalid"),
        "{failure}"
    );
    assert!(!directory.join("validated").exists());
    owner.shutdown();
    drop(owner);
    let restarted = Service::new(Some(profile.path().into()), CancellationToken::new());
    restarted
        .configure(Settings::default(), &[definition])
        .unwrap();
    assert_eq!(
        restarted
            .models()
            .await
            .unwrap()
            .into_iter()
            .find(|model| model.id == id)
            .unwrap()
            .state,
        State::Missing
    );
    restarted.shutdown();
}

#[tokio::test]
async fn restores_validation_receipts_and_rechecks_assets() {
    let profile = tempfile::tempdir().unwrap();
    let owner = Service::new(Some(profile.path().into()), CancellationToken::new());
    let (definition, directory) = seed(&owner);
    let id = catalog::id(&definition);
    let before = files::candidate(&directory, &definition).unwrap();
    // This unit exercises receipt persistence; only the installer calls mark
    // after successful real Engine validation in production.
    files::mark(&directory, &definition, &before).unwrap();
    owner.shutdown();
    drop(owner);
    let restarted = Service::new(Some(profile.path().into()), CancellationToken::new());
    restarted
        .configure(Settings::default(), std::slice::from_ref(&definition))
        .unwrap();
    assert_eq!(
        restarted
            .models()
            .await
            .unwrap()
            .into_iter()
            .find(|model| model.id == id)
            .unwrap()
            .state,
        State::Installed
    );
    std::fs::write(directory.join("validated"), "0".repeat(64)).unwrap();
    assert_eq!(
        restarted
            .models()
            .await
            .unwrap()
            .into_iter()
            .find(|model| model.id == id)
            .unwrap()
            .state,
        State::Missing
    );
    files::mark(&directory, &definition, &before).unwrap();
    let file = directory.join("detector.onnx");
    let changed = vec![b'x'; std::fs::metadata(&file).unwrap().len() as usize];
    std::fs::write(file, changed).unwrap();
    assert!(files::verified(&directory, &definition).is_none());
    assert!(files::mark(&directory, &definition, &before).is_err());
    assert_eq!(
        restarted
            .models()
            .await
            .unwrap()
            .into_iter()
            .find(|model| model.id == id)
            .unwrap()
            .state,
        State::Missing
    );
    restarted.shutdown();
}
