use sailry_protocol::{
    Fault,
    models::{Asset, Definition, Format, Hub, Kind, Pooling, Source},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Registration {
    pub id: String,
    pub definition: Definition,
}

pub(crate) fn id(definition: &Definition) -> String {
    let mut definition = definition.clone();
    definition
        .assets
        .sort_by(|left, right| left.name.cmp(&right.name));
    blake3::hash(&serde_json::to_vec(&definition).expect("model definition is serializable"))
        .to_hex()
        .to_string()
}

pub(crate) fn ocr() -> &'static [Definition] {
    static MODELS: LazyLock<Vec<Definition>> = LazyLock::new(|| {
        serde_json::from_str(include_str!("ocr.json")).expect("pinned OCR catalog is valid")
    });
    &MODELS
}

pub(crate) fn definitions(
    records: &[Registration],
) -> Result<BTreeMap<String, Arc<Definition>>, Fault> {
    let mut definitions = BTreeMap::new();
    for (id, definition) in embedding().into_iter().chain(
        ocr()
            .iter()
            .map(|definition| (id(definition), definition.clone())),
    ) {
        definition.validate()?;
        definitions.insert(id, Arc::new(definition));
    }
    for record in records {
        record.definition.validate()?;
        if !sailry_protocol::models::valid_id(&record.id)
            || record.definition.format.kind() == Kind::Speech
        {
            return Err(Fault::invalid("invalid Node model registration"));
        }
        if let Some(previous) =
            definitions.insert(record.id.clone(), Arc::new(record.definition.clone()))
            && previous.as_ref() != &record.definition
        {
            return Err(Fault::invalid("model identity has conflicting definitions"));
        }
    }
    for kind in [Kind::Embedding, Kind::Ocr, Kind::Decision] {
        if records
            .iter()
            .filter(|record| record.definition.format.kind() == kind)
            .count()
            > 128
        {
            return Err(Fault::invalid("local model catalog exceeds its limit"));
        }
    }
    Ok(definitions)
}

pub(crate) fn builtin(id: &str) -> bool {
    matches!(id, "granite-multi" | "bge-small-zh" | "potion-multi")
}

fn embedding() -> Vec<(String, Definition)> {
    sailry_embedding::catalog::MODELS
        .iter()
        .map(|descriptor| {
            use sailry_embedding::ModelId;
            let (id, name) = match descriptor.id {
                ModelId::GraniteMulti => ("granite-multi", "Granite Multilingual"),
                ModelId::BgeSmallZh => ("bge-small-zh", "BGE Small Chinese"),
                ModelId::PotionMulti => ("potion-multi", "Potion Multilingual"),
            };
            let source = Source {
                hub: Hub::HuggingFace,
                repository: descriptor.repository.into(),
                revision: descriptor.revision.into(),
            };
            (
                id.into(),
                Definition {
                    source,
                    name: name.into(),
                    license: descriptor.license.into(),
                    query_prefix: None,
                    document_prefix: None,
                    format: match descriptor.id {
                        ModelId::GraniteMulti | ModelId::BgeSmallZh => Format::Transformer {
                            dimension: descriptor.dimension as u16,
                            pooling: Pooling::Cls,
                            token_types: descriptor.id == ModelId::BgeSmallZh,
                        },
                        ModelId::PotionMulti => Format::Static {
                            dimension: descriptor.dimension as u16,
                        },
                    },
                    assets: descriptor
                        .assets
                        .iter()
                        .map(|asset| {
                            let origin = asset.source;
                            Asset {
                                source: Source {
                                    hub: Hub::HuggingFace,
                                    repository: origin
                                        .map_or(descriptor.repository, |source| source.repository)
                                        .into(),
                                    revision: origin
                                        .map_or(descriptor.revision, |source| source.revision)
                                        .into(),
                                },
                                path: origin.map_or(asset.name, |source| source.path).into(),
                                name: asset.name.into(),
                                bytes: asset.bytes,
                                sha256: asset.sha256.into(),
                            }
                        })
                        .collect(),
                },
            )
        })
        .collect()
}
