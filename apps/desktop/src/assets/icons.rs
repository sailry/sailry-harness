//! Reicon's pinned Outline catalog and manifest-provided vector glyphs.
use gpui_kit::{SharedString, component::Icon};
use sailry_protocol::plugin::desktop::Icon as Declaration;
use std::{collections::BTreeMap, sync::OnceLock};

fn catalog() -> &'static BTreeMap<String, String> {
    static CATALOG: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../assets/icons/reicon/catalog.json"))
            .expect("embedded Reicon catalog must be valid")
    })
}

pub(super) fn load(path: &str) -> Option<Vec<u8>> {
    let name = path.strip_prefix("reicon:")?;
    let paths = catalog().get(name)?;
    Some(
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none">{paths}</svg>"#)
            .into_bytes(),
    )
}

pub(super) fn list(prefix: &str) -> Vec<SharedString> {
    catalog()
        .keys()
        .map(|name| format!("reicon:{name}"))
        .filter(|name| name.starts_with(prefix))
        .map(Into::into)
        .collect()
}

pub(crate) fn icon(declaration: &Declaration) -> Icon {
    match declaration {
        Declaration::Name(name) => Icon::default().path(name.clone()),
        Declaration::Svg { svg } => Icon::default().data(svg.as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::AssetSource;

    #[test]
    fn portable_catalog_matches_embedded_assets() {
        let names: Vec<_> = sailry_protocol::plugin::desktop::icon::NAMES
            .lines()
            .collect();
        assert_eq!(
            names,
            catalog().keys().map(String::as_str).collect::<Vec<_>>()
        );
        for name in names {
            let bytes = super::super::Assets
                .load(&format!("reicon:{name}"))
                .unwrap()
                .unwrap();
            assert!(
                std::str::from_utf8(&bytes)
                    .unwrap()
                    .contains("viewBox=\"0 0 24 24\"")
            );
        }
        assert!(load("reicon:missing").is_none());
        assert_eq!(list("reicon:").len(), 2676);
    }
}
