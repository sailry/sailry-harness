use super::*;
use sha2::{Digest, Sha256};

fn seed(owner: &Registry) -> (Registration, PathBuf) {
    let mut definition = catalog::ocr()[0].clone();
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
    let record = Registration {
        id: catalog::id(&definition),
        definition,
    };
    let path = files::directory(&owner.root().unwrap(), &record.id, &record.definition).unwrap();
    std::fs::create_dir(&path).unwrap();
    for asset in &record.definition.assets {
        std::fs::write(
            path.join(&asset.name),
            if asset.name == "dictionary.txt" {
                b"\n".as_slice()
            } else {
                b"unvalidated graph".as_slice()
            },
        )
        .unwrap();
    }
    (record, path)
}

#[tokio::test]
async fn validates_preseeded_assets_before_installing() {
    let profile = tempfile::tempdir().unwrap();
    let owner = Registry::new(Some(profile.path().into()), CancellationToken::new());
    let (record, path) = seed(&owner);
    owner
        .configure(Settings::default(), std::slice::from_ref(&record))
        .unwrap();
    let error = owner
        .install(&record.id, &CancellationToken::new(), None)
        .await
        .unwrap_err();
    assert!(
        error.message.contains("OCR dictionary is invalid"),
        "{error}"
    );
    assert!(!path.join("validated").exists());
    assert_eq!(
        owner
            .models(Some(Kind::Ocr))
            .await
            .unwrap()
            .into_iter()
            .find(|model| model.id == record.id)
            .unwrap()
            .state,
        State::Missing
    );
    owner.shutdown().await;
}

#[tokio::test]
async fn restores_receipts_and_checks_live_assets() {
    let profile = tempfile::tempdir().unwrap();
    let owner = Registry::new(Some(profile.path().into()), CancellationToken::new());
    let (record, path) = seed(&owner);
    let before =
        sailry_model_assets::verified::candidate(&path, &files::assets(&record.definition))
            .unwrap();
    // Receipt persistence is separate from the installer's real engine validation.
    files::mark(&path, &record.definition, &before).unwrap();
    owner.shutdown().await;
    let restarted = Registry::new(Some(profile.path().into()), CancellationToken::new());
    restarted
        .configure(Settings::default(), std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(
        restarted.resource(Kind::Ocr, &record.id, None).unwrap().id,
        record.id
    );
    let service = crate::ocr::Service::new(restarted.clone());
    assert_eq!(
        service.signature().unwrap(),
        *blake3::hash(record.id.as_bytes()).as_bytes()
    );
    let mut settings = restarted.settings();
    settings.choices.insert(
        Kind::Ocr,
        sailry_protocol::models::Choice {
            enabled: false,
            model: Some(record.id.clone()),
        },
    );
    restarted
        .configure(settings, std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(service.signature().unwrap(), [0; 32]);
    assert_eq!(
        crate::ocr::Service::new(restarted.clone())
            .recognize(
                image::RgbImage::new(1, 1),
                Lane::Interactive,
                &CancellationToken::new()
            )
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
    std::fs::write(path.join("validated"), "0".repeat(64)).unwrap();
    assert_eq!(
        restarted
            .models(Some(Kind::Ocr))
            .await
            .unwrap()
            .into_iter()
            .find(|model| model.id == record.id)
            .unwrap()
            .state,
        State::Missing
    );
    files::mark(&path, &record.definition, &before).unwrap();
    std::fs::write(path.join("detector.onnx"), b"changed graph").unwrap();
    assert!(files::stamp(&path, &record.id, &record.definition, None).is_none());
    assert!(files::mark(&path, &record.definition, &before).is_err());
    restarted.shutdown().await;
}
