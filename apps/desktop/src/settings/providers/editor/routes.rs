//! Route-derived form values, independent of vendor identities and widgets.
use super::super::presets;
use crate::tr;
use gpui_kit::SharedString;
use sailry_protocol::conversation::presets::{Models, Preset, Route};
use std::collections::HashSet;

pub(super) fn metadata_only(preset: &Preset, route: Option<&Route>) -> bool {
    matches!(preset.models, Models::Static { .. })
        || route
            .and_then(|route| route.product)
            .is_some_and(|product| {
                preset
                    .products
                    .iter()
                    .any(|item| item.kind == product && !item.discover)
            })
}

pub(super) fn models(preset: &Preset, route: Option<&Route>) -> Vec<String> {
    let mut ids = match &preset.models {
        Models::Static { ids } => ids.clone(),
        Models::Discover => Vec::new(),
    };
    if let Some(product) = route.and_then(|route| route.product)
        && let Some(id) = preset
            .products
            .iter()
            .find(|item| item.kind == product)
            .and_then(|item| item.default_model.clone())
    {
        ids.insert(0, id);
    }
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
    ids
}

pub(super) fn label(
    preset: &sailry_protocol::conversation::presets::Preset,
    route: &Route,
    region: bool,
) -> SharedString {
    use sailry_protocol::conversation::{connection::Product, presets::Region};

    if region {
        tr(match route.region {
            Some(Region::China) => "provider_region_china",
            _ => "provider_region_global",
        })
    } else {
        presets::product_label(preset, route.product.unwrap_or(Product::Metered))
    }
}

pub(super) fn target(routes: &[Route], current: Option<&Route>, requested: Route) -> Route {
    current
        .filter(|current| current.product != requested.product)
        .and_then(|current| {
            routes.iter().find(|candidate| {
                candidate.protocol == requested.protocol
                    && candidate.product == requested.product
                    && candidate.region == current.region
            })
        })
        .cloned()
        .unwrap_or(requested)
}

pub(super) fn choices(routes: &[Route], current: &Route, region: bool) -> Vec<Route> {
    let mut choices: Vec<Route> = Vec::new();
    for route in routes {
        let applicable = if region {
            route.product == current.product && route.protocol == current.protocol
        } else {
            route.protocol == current.protocol
        };
        if applicable
            && !choices.iter().any(|other| {
                if region {
                    other.region == route.region
                } else {
                    other.product == route.product
                }
            })
        {
            choices.push(route.clone());
        }
    }
    choices
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use sailry_protocol::conversation::{connection::Product, presets::Region};

    #[test]
    fn derives_models_and_discovery_from_products() {
        let mut entry = presets::bundled()
            .into_iter()
            .find(|entry| entry.preset.id == "qwen")
            .unwrap();
        entry.preset.id = "fixture-vendor".into();
        let route = entry
            .preset
            .routes
            .iter()
            .find(|route| route.product == Some(Product::Coding))
            .unwrap()
            .clone();
        entry.preset.models = Models::Static {
            ids: vec!["fixture-a".into(), "fixture-default".into()],
        };
        let product = entry
            .preset
            .products
            .iter_mut()
            .find(|product| product.kind == Product::Coding)
            .unwrap();
        product.default_model = Some("fixture-default".into());
        assert_eq!(
            models(&entry.preset, Some(&route)),
            ["fixture-default", "fixture-a"]
        );
        assert!(metadata_only(&entry.preset, Some(&route)));
        entry.preset.models = Models::Discover;
        entry
            .preset
            .products
            .iter_mut()
            .find(|product| product.kind == Product::Coding)
            .unwrap()
            .discover = false;
        assert!(metadata_only(&entry.preset, Some(&route)));
        entry
            .preset
            .products
            .iter_mut()
            .find(|product| product.kind == Product::Coding)
            .unwrap()
            .discover = true;
        assert!(!metadata_only(&entry.preset, Some(&route)));
    }

    #[test]
    fn preserves_region_across_products() {
        let entry = presets::bundled()
            .into_iter()
            .find(|entry| entry.preset.id == "qwen")
            .unwrap();
        let current = entry
            .preset
            .routes
            .iter()
            .find(|route| {
                route.region == Some(Region::Global) && route.product == Some(Product::Metered)
            })
            .unwrap();
        let requested = entry
            .preset
            .routes
            .iter()
            .find(|route| {
                route.region == Some(Region::China) && route.product == Some(Product::Coding)
            })
            .unwrap()
            .clone();
        let target = target(&entry.preset.routes, Some(current), requested);
        assert_eq!(target.region, Some(Region::Global));
        assert_eq!(target.product, Some(Product::Coding));
        assert_eq!(
            target.endpoint,
            entry
                .preset
                .routes
                .iter()
                .find(|route| route.key() == target.key())
                .unwrap()
                .endpoint
        );
        let choices = choices(&entry.preset.routes, &target, true);
        assert_eq!(choices.len(), 2);
        assert!(
            choices
                .iter()
                .all(|route| route.product == Some(Product::Coding))
        );
    }

    #[test]
    fn keeps_explicit_missing_region_fallback() {
        let entry = presets::bundled()
            .into_iter()
            .find(|entry| entry.preset.id == "doubao")
            .unwrap();
        let current = entry
            .preset
            .routes
            .iter()
            .find(|route| route.product == Some(Product::Metered))
            .unwrap();
        let requested = entry
            .preset
            .routes
            .iter()
            .find(|route| route.product == Some(Product::Coding))
            .unwrap()
            .clone();
        assert_eq!(
            target(&entry.preset.routes, Some(current), requested.clone()),
            requested
        );
    }
}
