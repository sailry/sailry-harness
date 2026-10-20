//! Real, tiny static embeddings keep registry and scheduler checks independent of downloads.
use super::registry::{Registration, Registry, catalog, files};
use sailry_link::CancellationToken;
use sailry_protocol::models::{Asset, Choice, Definition, Format, Hub, Kind, Settings, Source};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc};

pub(crate) fn record(dimension: u16, values: &[f32]) -> (Registration, Vec<(String, Vec<u8>)>) {
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
    let contents = vec![
        ("config.json".into(), br#"{"normalize":true}"#.to_vec()),
        ("tokenizer.json".into(), serde_json::to_vec(&serde_json::json!({
            "version":"1.0", "truncation":null, "padding":null, "added_tokens":[],
            "normalizer":null, "pre_tokenizer":{"type":"WhitespaceSplit"},
            "post_processor":null, "decoder":null,
            "model":{"type":"WordLevel", "vocab":{"[UNK]":0,"alpha":1,"beta":2}, "unk_token":"[UNK]"}
        })).unwrap()),
        ("model.safetensors".into(), table),
    ];
    let source = Source {
        hub: Hub::HuggingFace,
        repository: "owner/static-fixture".into(),
        revision: "a".repeat(40),
    };
    let definition = Definition {
        source: source.clone(),
        name: "Static fixture".into(),
        license: "MIT".into(),
        format: Format::Static { dimension },
        query_prefix: None,
        document_prefix: None,
        assets: contents
            .iter()
            .map(|(name, body): &(String, Vec<u8>)| Asset {
                source: source.clone(),
                path: name.clone(),
                name: name.clone(),
                bytes: body.len() as u64,
                sha256: format!("{:x}", Sha256::digest(body)),
            })
            .collect(),
    };
    definition.validate().unwrap();
    (
        Registration {
            id: catalog::id(&definition),
            definition,
        },
        contents,
    )
}

pub(super) struct Fixture {
    pub _profile: tempfile::TempDir,
    pub owner: Arc<Registry>,
    pub record: Registration,
    pub path: PathBuf,
}

impl Fixture {
    pub async fn new() -> Self {
        let profile = tempfile::tempdir().unwrap();
        let owner = Registry::new(Some(profile.path().into()), CancellationToken::new());
        let (record, contents) = record(2, &[100., 0., 1., 0., 0., 1.]);
        owner
            .configure(
                Settings {
                    choices: std::collections::BTreeMap::from([(
                        Kind::Embedding,
                        Choice {
                            enabled: true,
                            model: Some(record.id.clone()),
                        },
                    )]),
                    ..Settings::default()
                },
                std::slice::from_ref(&record),
            )
            .unwrap();
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
        owner
            .install(&record.id, &CancellationToken::new(), None)
            .await
            .unwrap();
        Self {
            _profile: profile,
            owner,
            record,
            path,
        }
    }
}
