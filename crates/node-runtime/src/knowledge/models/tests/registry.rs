use super::*;
use sailry_protocol::{ModelId, models::Asset};
use sha2::{Digest, Sha256};

fn record(dimension: u16, values: &[f32]) -> (Registration, Vec<(String, Vec<u8>)>) {
    assert_eq!(values.len(), 3 * usize::from(dimension));
    let header = serde_json::to_vec(&serde_json::json!({"embeddings": {
        "dtype": "F32", "shape": [3, dimension], "data_offsets": [0, values.len() * 4]
    }}))
    .unwrap();
    let mut table = (header.len() as u64).to_le_bytes().to_vec();
    table.extend(header);
    for value in values {
        table.extend(value.to_le_bytes());
    }
    let contents: Vec<(String, Vec<u8>)> = vec![
        ("config.json".into(), br#"{"normalize":true}"#.to_vec()),
        (
            "tokenizer.json".into(),
            serde_json::to_vec(&serde_json::json!({
                "version": "1.0", "truncation": null, "padding": null,
                "added_tokens": [], "normalizer": null,
                "pre_tokenizer": {"type": "WhitespaceSplit"},
                "post_processor": null, "decoder": null,
                "model": {"type": "WordLevel", "vocab": {"[UNK]": 0, "alpha": 1, "beta": 2}, "unk_token": "[UNK]"}
            }))
            .unwrap(),
        ),
        ("model.safetensors".into(), table),
    ];
    let mut definition = catalog::builtin(Model::PotionMulti).unwrap();
    definition.name = "Static fixture".into();
    definition.source.repository = "owner/static-fixture".into();
    definition.source.revision = "a".repeat(40);
    definition.format = Format::Static { dimension };
    definition.assets = contents
        .iter()
        .map(|(name, bytes)| Asset {
            source: definition.source.clone(),
            path: name.clone(),
            name: name.clone(),
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        })
        .collect();
    definition.validate().unwrap();
    (
        Registration {
            model: Model::Registered { id: ModelId::new() },
            definition,
        },
        contents,
    )
}

fn write(root: &Path, record: &Registration, contents: &[(String, Vec<u8>)]) {
    let path = files::directory(root, record.model);
    std::fs::create_dir_all(&path).unwrap();
    for (name, contents) in contents {
        std::fs::write(path.join(name), contents).unwrap();
    }
}

fn selected(model: Model, revision: u64) -> Settings {
    Settings {
        model: Some(model),
        revision,
        mirror: None,
    }
}

#[tokio::test]
async fn uses_captured_prefixes_independent_of_priority() {
    let profile = tempfile::tempdir().unwrap();
    let (mut record, contents) = record(2, &[100., 0., 1., 0., 0., 1.]);
    record.definition.query_prefix = Some("alpha ".into());
    record.definition.document_prefix = Some("beta ".into());
    let (root, _) = files::open(profile.path()).unwrap();
    write(&root, &record, &contents);
    let owner = Models::open(
        profile.path(),
        selected(record.model, 0),
        vec![record.clone()],
    )
    .await
    .unwrap();
    owner
        .install(record.model, &CancellationToken::new())
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    for (purpose, lane, expected) in [
        (sailry_embedding::Input::Query, Lane::Index, vec![1., 0.]),
        (sailry_embedding::Input::Document, Lane::Query, vec![0., 1.]),
    ] {
        assert_eq!(
            owner
                .encode(record.model, 0, vec!["".into()], purpose, lane, &cancel)
                .await
                .unwrap(),
            [expected]
        );
    }
    assert_eq!(
        owner
            .offsets(record.model, 0, "alpha beta".into(), &cancel)
            .await
            .unwrap(),
        [(0, 5), (6, 10)]
    );
    owner.shutdown().await;
}

#[tokio::test]
async fn exposes_metadata_without_installing() {
    let profile = tempfile::tempdir().unwrap();
    let (record, _) = record(3, &[100., 0., 0., 1., 0., 0., 0., 1., 0.]);
    let owner = Models::open(
        profile.path(),
        selected(record.model, 0),
        vec![record.clone()],
    )
    .await
    .unwrap();
    let infos = owner.infos().await.unwrap();
    assert_eq!(infos.len(), Model::ALL.len() + 1);
    let info = infos
        .iter()
        .find(|info| info.model == record.model)
        .unwrap();
    assert_eq!(info.definition, record.definition);
    assert_eq!(info.repository, record.definition.source.repository);
    assert_eq!(info.revision, record.definition.source.revision);
    assert_eq!(info.dimension, 3);
    assert_eq!(info.license, record.definition.license);
    assert_eq!(
        info.size_bytes,
        record
            .definition
            .assets
            .iter()
            .map(|asset| asset.bytes)
            .sum::<u64>()
    );
    assert!(infos.iter().all(|info| info.state == ModelState::Missing));
    assert!(!files::directory(&owner.root, record.model).exists());
    assert_eq!(
        owner
            .encode(
                record.model,
                0,
                vec!["alpha".into()],
                sailry_embedding::Input::Query,
                Lane::Query,
                &CancellationToken::new()
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
    owner.shutdown().await;
}

#[tokio::test]
async fn refuses_absent_selections() {
    let profile = tempfile::tempdir().unwrap();
    let unknown = Model::Registered { id: ModelId::new() };
    let error = Models::open(profile.path(), selected(unknown, 0), vec![])
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::NotFound);
    let fixture = Fixture::new();
    let previous = fixture.owner.infos().await.unwrap();
    let mut updates = fixture.owner.subscribe();
    assert_eq!(
        fixture
            .owner
            .configure(selected(unknown, 1), &[])
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(fixture.owner.infos().await.unwrap(), previous);
    assert!(!updates.has_changed().unwrap());
    updates.borrow_and_update();
    fixture
        .owner
        .current(&fixture.work("original capture"))
        .unwrap();
    fixture.owner.shutdown().await;
}

#[tokio::test]
async fn preserves_registered_definitions() {
    let fixture = Fixture::new();
    let (original, _) = record(3, &[100., 0., 0., 1., 0., 0., 0., 1., 0.]);
    let mut updates = fixture.owner.subscribe();
    fixture
        .owner
        .configure(selected(original.model, 1), std::slice::from_ref(&original))
        .unwrap();
    assert!(updates.has_changed().unwrap());
    updates.borrow_and_update();
    let infos = fixture.owner.infos().await.unwrap();
    assert_eq!(infos.last().unwrap().definition, original.definition);
    let mut changed = original.clone();
    changed.definition.name = "Changed metadata".into();
    assert_eq!(
        fixture
            .owner
            .configure(selected(original.model, 2), &[changed])
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        fixture
            .owner
            .configure(Settings::default(), &[])
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(fixture.owner.infos().await.unwrap(), infos);
    assert!(!updates.has_changed().unwrap());
    let mut captured = fixture.work("collection override");
    captured.revision = 1;
    captured.generation = 1;
    fixture.owner.current(&captured).unwrap();
    fixture.owner.shutdown().await;
}

#[test]
fn rejects_invalid_catalog_entries() {
    let (record, _) = record(3, &[100., 0., 0., 1., 0., 0., 0., 1., 0.]);
    let mut malformed = record.clone();
    malformed.definition.assets[0].path = "../config.json".into();
    let mut recommended = record.clone();
    recommended.model = Model::PotionMulti;
    let mut speech = record.clone();
    speech.definition.format = Format::SenseVoice {};
    speech.definition.assets[0].name = "tokens.txt".into();
    speech.definition.assets[1].name = "model.onnx".into();
    speech.definition.assets.pop();
    speech.definition.validate().unwrap();
    for records in [
        vec![record.clone(), record],
        vec![malformed],
        vec![recommended],
        vec![speech],
    ] {
        assert_eq!(
            catalog::definitions(&records).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}

#[test]
fn binds_validation_to_complete_definition() {
    let profile = tempfile::tempdir().unwrap();
    let (root, handle) = files::open(profile.path()).unwrap();
    let (record, contents) = record(3, &[100., 0., 0., 1., 0., 0., 0., 1., 0.]);
    write(&root, &record, &contents);
    assert!(files::stamp(&root, record.model, &record.definition).is_none());
    assert!(files::verified(&root, record.model, &record.definition).is_none());
    files::mark(&handle, record.model, &record.definition).unwrap();
    assert!(files::verified(&root, record.model, &record.definition).is_some());
    let mut changed = record.definition.clone();
    changed.format = Format::Static { dimension: 2 };
    assert!(files::stamp(&root, record.model, &changed).is_none());
    files::mark(&handle, record.model, &changed).unwrap();
    assert!(files::stamp(&root, record.model, &record.definition).is_none());
    files::mark(&handle, record.model, &record.definition).unwrap();
    let path = files::directory(&root, record.model).join("tokenizer.json");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[0] = b'!';
    std::fs::write(path, bytes).unwrap();
    assert!(files::stamp(&root, record.model, &record.definition).is_some());
    assert!(files::verified(&root, record.model, &record.definition).is_none());
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_validation_marker() {
    let profile = tempfile::tempdir().unwrap();
    let (root, handle) = files::open(profile.path()).unwrap();
    let (record, contents) = record(3, &[100., 0., 0., 1., 0., 0., 0., 1., 0.]);
    write(&root, &record, &contents);
    let outside = profile.path().join("outside-marker");
    std::fs::write(&outside, "a".repeat(64)).unwrap();
    std::os::unix::fs::symlink(
        &outside,
        files::directory(&root, record.model).join("validated"),
    )
    .unwrap();
    assert!(files::mark(&handle, record.model, &record.definition).is_err());
    assert!(files::stamp(&root, record.model, &record.definition).is_none());
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "a".repeat(64));
}

#[tokio::test]
async fn isolates_registered_inference_and_removal() {
    let profile = tempfile::tempdir().unwrap();
    let (root, handle) = files::open(profile.path()).unwrap();
    let (first, contents) = record(3, &[100., 0., 0., 1., 0., 0., 0., 1., 0.]);
    write(&root, &first, &contents);
    files::mark(&handle, first.model, &first.definition).unwrap();
    let (second, contents) = record(2, &[100., 0., 0., 1., 1., 0.]);
    write(&root, &second, &contents);
    files::mark(&handle, second.model, &second.definition).unwrap();
    assert_ne!(
        files::directory(&root, first.model),
        files::directory(&root, second.model)
    );
    let owner = Models::open(
        profile.path(),
        selected(first.model, 0),
        vec![first.clone(), second.clone()],
    )
    .await
    .unwrap();
    let cancel = CancellationToken::new();
    for (model, expected) in [
        (first.model, vec![1., 0., 0.]),
        (second.model, vec![0., 1.]),
        (first.model, vec![1., 0., 0.]),
    ] {
        assert_eq!(
            owner
                .encode(
                    model,
                    0,
                    vec!["alpha".into()],
                    sailry_embedding::Input::Query,
                    Lane::Query,
                    &cancel
                )
                .await
                .unwrap(),
            [expected]
        );
    }
    assert_eq!(
        owner
            .offsets(first.model, 0, "alpha beta".into(), &cancel)
            .await
            .unwrap(),
        [(0, 5), (6, 10)]
    );
    owner.remove(first.model).await.unwrap();
    assert!(!files::directory(&root, first.model).exists());
    assert!(files::verified(&root, second.model, &second.definition).is_some());
    assert_eq!(
        owner
            .encode(
                first.model,
                0,
                vec!["alpha".into()],
                sailry_embedding::Input::Query,
                Lane::Query,
                &cancel
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
    assert_eq!(
        owner
            .encode(
                second.model,
                0,
                vec!["alpha".into()],
                sailry_embedding::Input::Query,
                Lane::Query,
                &cancel
            )
            .await
            .unwrap(),
        [vec![0., 1.]]
    );
    owner.shutdown().await;
}
