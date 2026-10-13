//! Offline ReUI Solid artwork generated from pinned, MIT-licensed source.
use gpui_kit::SharedString;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

const PREFIX: &str = "icons/reui-file-type/";

#[derive(Deserialize)]
pub(crate) struct Catalog {
    pub(crate) types: BTreeMap<String, Artwork>,
    pub(crate) mime: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub(crate) struct Artwork {
    pub(crate) category: String,
    compact: String,
    regular: String,
}

pub(crate) fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../../assets/icons/reui-file-type/catalog.json"
        ))
        .expect("embedded ReUI file types must be valid")
    })
}

pub(super) fn load(path: &str) -> Option<Vec<u8>> {
    let path = path.strip_prefix(PREFIX)?;
    let (edition, name) = path.split_once('/')?;
    let name = name.strip_suffix(".svg")?;
    let artwork = catalog().types.get(name)?;
    match edition {
        "compact" => Some(artwork.compact.as_bytes().to_vec()),
        "regular" => Some(artwork.regular.as_bytes().to_vec()),
        _ => None,
    }
}

pub(super) fn list(prefix: &str) -> Vec<SharedString> {
    catalog()
        .types
        .keys()
        .flat_map(|name| {
            ["compact", "regular"].map(|edition| format!("{PREFIX}{edition}/{name}.svg"))
        })
        .filter(|name| name.starts_with(prefix))
        .map(Into::into)
        .collect()
}
