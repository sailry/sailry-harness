use super::*;
use sailry_protocol::models::{Asset, Definition as ModelDefinition, Format, Hub, Source};
use sha2::{Digest, Sha256};

pub(super) struct Assets {
    pub(super) definition: ModelDefinition,
    contents: Vec<(String, Vec<u8>)>,
}

impl Assets {
    pub(super) fn new(dimension: u16, values: &[f32]) -> Self {
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
            (
                "config.json".into(),
                serde_json::to_vec(&serde_json::json!({
                    "model_type": "model2vec", "normalize": true, "hidden_dim": dimension,
                    "sif_coefficient": null
                }))
                .unwrap(),
            ),
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
        let source = Source {
            hub: Hub::HuggingFace,
            repository: "owner/registered-static".into(),
            revision: "a".repeat(40),
        };
        let definition = ModelDefinition {
            source: source.clone(),
            name: format!("Static {dimension}"),
            license: "MIT".into(),
            format: Format::Static { dimension },
            assets: contents
                .iter()
                .map(|(name, bytes)| Asset {
                    source: source.clone(),
                    path: name.clone(),
                    name: name.clone(),
                    bytes: bytes.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                })
                .collect(),
        };
        definition.validate().unwrap();
        Self {
            definition,
            contents,
        }
    }

    pub(super) fn seed(&self, profile: &Path, model: Model) -> PathBuf {
        assert!(matches!(model, Model::Registered { .. }));
        let directory = profile
            .join("models/embedding")
            .join(model.key().as_ref())
            .join("files");
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory).unwrap();
        for (name, contents) in &self.contents {
            let path = directory.join(name);
            fs::write(&path, contents).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        assert!(!directory.join("validated").exists());
        directory
    }
}
