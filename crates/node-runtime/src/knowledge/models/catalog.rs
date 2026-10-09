use sailry_protocol::{
    Fault,
    knowledge::{Model, Registration},
    models::{Asset, Definition, Format, Hub, Pooling, Source},
};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn definitions(
    records: &[Registration],
) -> Result<BTreeMap<Model, Arc<Definition>>, Fault> {
    let mut definitions = Model::ALL
        .into_iter()
        .map(|model| {
            (
                model,
                Arc::new(builtin(model).expect("recommended model has a definition")),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for record in records {
        record.definition.validate()?;
        if !matches!(record.model, Model::Registered { .. })
            || record.definition.format.kind() != sailry_protocol::models::Kind::Embedding
            || definitions
                .insert(record.model, Arc::new(record.definition.clone()))
                .is_some()
        {
            return Err(Fault::invalid("invalid registered embedding model"));
        }
    }
    Ok(definitions)
}

pub(crate) fn builtin(model: Model) -> Option<Definition> {
    use sailry_embedding::ModelId;
    let id = match model {
        Model::GraniteMulti => ModelId::GraniteMulti,
        Model::BgeSmallZh => ModelId::BgeSmallZh,
        Model::PotionMulti => ModelId::PotionMulti,
        Model::Registered { .. } => return None,
    };
    let descriptor = id.descriptor();
    let source = Source {
        hub: Hub::HuggingFace,
        repository: descriptor.repository.into(),
        revision: descriptor.revision.into(),
    };
    Some(Definition {
        source: source.clone(),
        name: match model {
            Model::GraniteMulti => "Granite Multilingual",
            Model::BgeSmallZh => "BGE Small Chinese",
            Model::PotionMulti => "Potion Multilingual",
            Model::Registered { .. } => return None,
        }
        .into(),
        license: descriptor.license.into(),
        format: match model {
            Model::GraniteMulti | Model::BgeSmallZh => Format::Transformer {
                dimension: descriptor.dimension as u16,
                pooling: Pooling::Cls,
                token_types: model == Model::BgeSmallZh,
            },
            Model::PotionMulti => Format::Static {
                dimension: descriptor.dimension as u16,
            },
            Model::Registered { .. } => return None,
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
    })
}
