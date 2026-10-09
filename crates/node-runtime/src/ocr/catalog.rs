use sailry_protocol::{
    Fault,
    models::{Definition, Kind},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

pub(crate) fn recommended() -> &'static [Definition] {
    static MODELS: LazyLock<Vec<Definition>> = LazyLock::new(|| {
        serde_json::from_str(include_str!("catalog.json")).expect("pinned OCR catalog is valid")
    });
    &MODELS
}

pub(crate) fn id(definition: &Definition) -> String {
    let mut normalized = definition.clone();
    normalized
        .assets
        .sort_by(|left, right| left.name.cmp(&right.name));
    blake3::hash(&serde_json::to_vec(&normalized).expect("model metadata is serializable"))
        .to_hex()
        .to_string()
}

pub(crate) fn definitions(
    records: &[Definition],
) -> Result<BTreeMap<String, Arc<Definition>>, Fault> {
    let mut result = BTreeMap::new();
    for definition in recommended().iter().cloned().chain(records.iter().cloned()) {
        definition.validate()?;
        if definition.format.kind() != Kind::Ocr {
            return Err(Fault::invalid("OCR model format required"));
        }
        result.insert(id(&definition), Arc::new(definition));
    }
    if result.len() > 128 {
        return Err(Fault::invalid("OCR model catalog exceeds its limit"));
    }
    Ok(result)
}
