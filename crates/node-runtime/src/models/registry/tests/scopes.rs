use super::*;
use sailry_protocol::models::{Asset, Format, Hub};

mod service;

fn decision() -> Registration {
    let mut definition = record(2, &[100., 0., 1., 0., 0., 1.]).0.definition;
    definition.format = Format::Decision {};
    definition.assets = vec![Asset {
        source: definition.source.clone(),
        path: "model.gguf".into(),
        name: "model.gguf".into(),
        bytes: 1,
        sha256: "a".repeat(64),
    }];
    definition.validate().unwrap();
    Registration {
        id: catalog::id(&definition),
        definition,
    }
}

fn ocr() -> Registration {
    let mut definition = catalog::ocr()[0].clone();
    definition.name = "Additional OCR".into();
    Registration {
        id: catalog::id(&definition),
        definition,
    }
}

fn transfer(owner: &Registry, id: &str) -> CancellationToken {
    let mut state = owner.state.lock().unwrap();
    let cancel = owner.stop.child_token();
    state.transfers.insert(id.into(), cancel.clone());
    cancel
}

#[tokio::test]
async fn preserves_downloads_across_kind_changes() {
    let fixture = Fixture::new().await;
    let decision = decision();
    let mut records = vec![fixture.record.clone(), decision.clone()];
    let mut settings = fixture.owner.settings();
    fixture.owner.configure(settings.clone(), &records).unwrap();
    let downloading = transfer(&fixture.owner, &decision.id);
    let embedding = fixture
        .owner
        .resource(Kind::Embedding, &fixture.record.id, None)
        .unwrap();
    records.push(ocr());
    fixture.owner.configure(settings.clone(), &records).unwrap();
    assert!(!downloading.is_cancelled());
    fixture.owner.current(&embedding, false).unwrap();
    settings.revision += 1;
    fixture.owner.configure(settings.clone(), &records).unwrap();
    assert!(!downloading.is_cancelled());
    settings.choices.get_mut(&Kind::Ocr).unwrap().enabled = false;
    settings.revision += 1;
    fixture.owner.configure(settings.clone(), &records).unwrap();
    assert!(!downloading.is_cancelled());
    fixture.owner.current(&embedding, false).unwrap();
    let previous = settings.choice(Kind::Decision);
    settings.choices.get_mut(&Kind::Decision).unwrap().model = Some(decision.id.clone());
    settings.revision += 1;
    fixture.owner.configure(settings.clone(), &records).unwrap();
    assert!(!downloading.is_cancelled());
    assert_eq!(
        fixture
            .owner
            .resource(Kind::Decision, &decision.id, Some(&previous))
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    settings.choices.get_mut(&Kind::Decision).unwrap().enabled = false;
    settings.revision += 1;
    fixture.owner.configure(settings, &records).unwrap();
    assert!(!downloading.is_cancelled());
    fixture.owner.current(&embedding, false).unwrap();
    assert!(fixture.owner.cancel(&decision.id));
    assert!(downloading.is_cancelled());
    let downloading = transfer(&fixture.owner, &decision.id);
    fixture.owner.shutdown().await;
    assert!(downloading.is_cancelled());
}

#[tokio::test]
async fn limits_mirror_changes_to_affected_downloads() {
    let fixture = Fixture::new().await;
    let decision = decision();
    let mut ocr = ocr();
    for asset in &mut ocr.definition.assets {
        asset.source.hub = Hub::ModelScope;
    }
    ocr.id = catalog::id(&ocr.definition);
    let records = vec![fixture.record.clone(), decision.clone(), ocr.clone()];
    let mut settings = fixture.owner.settings();
    fixture.owner.configure(settings.clone(), &records).unwrap();
    let mirrored = transfer(&fixture.owner, &decision.id);
    let direct = transfer(&fixture.owner, &ocr.id);
    let resource = fixture
        .owner
        .resource(Kind::Embedding, &fixture.record.id, None)
        .unwrap();
    settings.mirror = Some("https://huggingface.co/".into());
    settings.revision += 1;
    fixture.owner.configure(settings.clone(), &records).unwrap();
    assert!(!mirrored.is_cancelled());
    assert!(!direct.is_cancelled());
    settings.mirror = Some("https://mirror.example.com/".into());
    settings.revision += 1;
    fixture.owner.configure(settings, &records).unwrap();
    assert!(mirrored.is_cancelled());
    assert!(!direct.is_cancelled());
    fixture.owner.current(&resource, false).unwrap();
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn publishes_only_catalog_and_retrieval_changes() {
    let fixture = Fixture::new().await;
    let mut settings = fixture.owner.settings();
    let mut updates = fixture.owner.subscribe();
    settings.revision += 1;
    settings.choices.get_mut(&Kind::Ocr).unwrap().enabled = false;
    fixture
        .owner
        .configure(settings.clone(), std::slice::from_ref(&fixture.record))
        .unwrap();
    assert!(updates.has_changed().unwrap());
    assert!(!updates.borrow_and_update().ocr.enabled);
    settings.mirror = Some("https://mirror.example.com/".into());
    settings.revision += 1;
    fixture
        .owner
        .configure(settings.clone(), std::slice::from_ref(&fixture.record))
        .unwrap();
    assert!(!updates.has_changed().unwrap());
    let added = ocr();
    let records = [fixture.record.clone(), added.clone()];
    fixture.owner.configure(settings.clone(), &records).unwrap();
    assert!(updates.has_changed().unwrap());
    assert!(
        updates
            .borrow_and_update()
            .models
            .iter()
            .any(|model| model.id == added.id)
    );
    settings.choices.get_mut(&Kind::Embedding).unwrap().enabled = false;
    settings.revision += 1;
    fixture.owner.configure(settings, &records).unwrap();
    assert!(updates.has_changed().unwrap());
    assert!(!updates.borrow_and_update().embedding.enabled);
    fixture.owner.shutdown().await;
}
