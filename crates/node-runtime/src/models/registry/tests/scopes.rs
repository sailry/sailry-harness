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
    let kind = state.definitions[id].format.kind();
    let cancel = state.scopes[&kind].child_token();
    state.transfers.insert(
        id.into(),
        Transfer {
            cancel: cancel.clone(),
            progress: Default::default(),
        },
    );
    cancel
}

#[tokio::test]
async fn preserves_downloads_until_their_kind_changes() {
    let fixture = Fixture::new().await;
    let decision = decision();
    let mut records = vec![fixture.record.clone(), decision.clone()];
    let mut settings = fixture.owner.settings();
    fixture.owner.configure(settings.clone(), &records).unwrap();
    let downloading = transfer(&fixture.owner, &decision.id);
    let embedding = fixture
        .owner
        .resource(Kind::Embedding, Some(&fixture.record.id), None)
        .unwrap();
    records.push(ocr());
    fixture.owner.configure(settings.clone(), &records).unwrap();
    assert!(!downloading.is_cancelled());
    fixture.owner.current(&embedding).unwrap();
    settings.revision += 1;
    fixture.owner.configure(settings.clone(), &records).unwrap();
    assert!(!downloading.is_cancelled());
    settings.choices.get_mut(&Kind::Ocr).unwrap().enabled = false;
    settings.revision += 1;
    fixture.owner.configure(settings.clone(), &records).unwrap();
    assert!(!downloading.is_cancelled());
    fixture.owner.current(&embedding).unwrap();
    let previous = settings.choice(Kind::Decision);
    settings.choices.get_mut(&Kind::Decision).unwrap().model = Some(decision.id.clone());
    settings.revision += 1;
    fixture.owner.configure(settings.clone(), &records).unwrap();
    assert!(downloading.is_cancelled());
    assert_eq!(
        fixture
            .owner
            .resource(Kind::Decision, Some(&decision.id), Some(&previous))
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let downloading = transfer(&fixture.owner, &decision.id);
    settings.choices.get_mut(&Kind::Decision).unwrap().enabled = false;
    settings.revision += 1;
    fixture.owner.configure(settings, &records).unwrap();
    assert!(downloading.is_cancelled());
    fixture.owner.current(&embedding).unwrap();
    fixture.owner.shutdown().await;
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
        .resource(Kind::Embedding, Some(&fixture.record.id), None)
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
    fixture.owner.current(&resource).unwrap();
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn retains_newer_catalog_snapshots() {
    let fixture = Fixture::new().await;
    let previous = fixture.owner.settings();
    let mut current = previous.clone();
    current.revision += 1;
    current.choices.get_mut(&Kind::Ocr).unwrap().enabled = false;
    let added = ocr();
    fixture
        .owner
        .configure(current.clone(), &[fixture.record.clone(), added.clone()])
        .unwrap();
    let scope = fixture.owner.state.lock().unwrap().scopes[&Kind::Ocr].clone();
    fixture
        .owner
        .configure(previous, std::slice::from_ref(&fixture.record))
        .unwrap();
    fixture
        .owner
        .configure(current.clone(), std::slice::from_ref(&fixture.record))
        .unwrap();
    assert_eq!(fixture.owner.settings(), current);
    assert_eq!(
        fixture.owner.definition(&added.id).unwrap().as_ref(),
        &added.definition
    );
    assert!(!scope.is_cancelled());
    fixture.owner.shutdown().await;
}
