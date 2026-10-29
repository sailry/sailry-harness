use super::*;
use core::prelude::v1::test;

#[test]
fn preserves_builtin_icons_and_methods() {
    let entries = bundled();
    let groups = groups(&entries);
    let (_, openai) = groups.iter().find(|(key, _)| key == "openai").unwrap();
    assert!(
        openai
            .iter()
            .any(|entry| entry.preset.id == "chat-gpt-oauth")
    );
    assert!(
        openai
            .iter()
            .any(|entry| entry.preset.id == "compatible-responses")
    );
    for entry in entries.iter().filter(|entry| entry.preset.offered) {
        assert!(
            entry
                .preset
                .logo
                .as_ref()
                .is_some_and(|path| path.starts_with("icons/providers/"))
        );
        assert!(!entry.preset.routes.is_empty());
        for route in &entry.preset.routes {
            assert_eq!(kind(entry, route).api(), route.protocol.api());
        }
    }
}

#[test]
fn keeps_package_brands_separate() {
    let mut entries = bundled();
    let mut plugin = entries[0].clone();
    plugin.source = Source::Package {
        name: "sample.models".into(),
    };
    plugin.provenance = Provenance::ThirdParty;
    let key = brand(&plugin);
    entries.push(plugin);
    assert_eq!(
        groups(&entries)
            .iter()
            .filter(|(brand, _)| brand == &key)
            .count(),
        1
    );
    assert_ne!(key, brand(&entries[0]));
}

#[test]
fn prefers_declared_brand_glyphs() {
    let mut plugin = bundled().remove(0);
    plugin.source = Source::Package {
        name: "sample.models".into(),
    };
    plugin.preset.logo = None;
    assert_eq!(glyph(&plugin), Glyph::Fallback);
    plugin.icon = Some("data:image/svg+xml;base64,PHN2Zy8+".into());
    assert_eq!(
        glyph(&plugin),
        Glyph::Image(ImageFormat::Svg, b"<svg/>".to_vec())
    );
    let declared = sailry_protocol::plugin::desktop::Icon::Name("reicon:messages/chat".into());
    plugin.preset.brand_display = Some(sailry_protocol::plugin::desktop::Navigation {
        label: "Sample".into(),
        locales: Default::default(),
        icon: Some(declared.clone()),
    });
    assert_eq!(glyph(&plugin), Glyph::Declared(&declared));
}

#[test]
fn routes_outweigh_brand_glyphs() {
    let mut entry = bundled()
        .into_iter()
        .find(|entry| entry.preset.id == "qwen")
        .unwrap();
    entry.preset.id = "fixture-vendor".into();
    entry.preset.brand = Some(entry.preset.id.clone());
    entry.preset.name = "Fixture vendor".into();
    let mut route = entry.preset.routes[0].clone();
    for (protocol, expected) in [
        (Protocol::OpenAiChatCompletions, Kind::CompatibleChat),
        (Protocol::OpenAiResponses, Kind::CompatibleResponses),
        (Protocol::Anthropic, Kind::CompatibleAnthropic),
        (Protocol::Gemini, Kind::CompatibleGemini),
    ] {
        route.protocol = protocol;
        entry.preset.routes = vec![route.clone()];
        assert_eq!(kind(&entry, &route), expected);
        assert_eq!(kind(&entry, &route).api(), protocol.api());
        assert_eq!(groups(std::slice::from_ref(&entry))[0].0, "fixture-vendor");
        assert!(!custom_endpoint(&entry, &route));
    }
}

#[test]
fn preserves_existing_vendor_categories() {
    use super::super::{data::Category, vendors::Vendor};
    let entries = bundled();
    // Existing picker categories from Sailry 3ae18182; route data remains declarative.
    for vendor in Vendor::ALL {
        let entry = entries
            .iter()
            .find(|entry| entry.preset.id == vendor.id())
            .unwrap();
        let expected = if Vendor::FIRST_PARTY.contains(&vendor) {
            Category::Vendor(vendor)
        } else {
            Category::OpenAi
        };
        for route in &entry.preset.routes {
            assert_eq!(kind(entry, route).category(), expected, "{}", vendor.id());
            assert_eq!(kind(entry, route).api(), route.protocol.api());
        }
    }
}

#[test]
fn known_identity_never_overrides_protocols() {
    use super::super::vendors::Vendor;
    let entries = bundled();
    for vendor in Vendor::FIRST_PARTY {
        let mut entry = entries
            .iter()
            .find(|entry| entry.preset.id == vendor.id())
            .unwrap()
            .clone();
        entry.preset.logo = None;
        let mut route = entry.preset.routes[0].clone();
        for (protocol, expected) in [
            (Protocol::OpenAiResponses, Kind::CompatibleResponses),
            (Protocol::Anthropic, Kind::CompatibleAnthropic),
            (Protocol::Gemini, Kind::CompatibleGemini),
        ] {
            route.protocol = protocol;
            assert_eq!(kind(&entry, &route), expected, "{}", vendor.id());
            assert_eq!(kind(&entry, &route).api(), route.protocol.api());
        }
        route.protocol = Protocol::OpenAiChatCompletions;
        if vendor == Vendor::DeepSeek {
            assert_eq!(kind(&entry, &route), Kind::CompatibleChat);
            route.protocol = Protocol::DeepSeek;
        }
        assert_eq!(kind(&entry, &route), Kind::Hosted(vendor));
        route.protocol = Protocol::OpenAiChatCompletions;
        entry.source = Source::Package {
            name: "fixture".into(),
        };
        assert_eq!(kind(&entry, &route), Kind::CompatibleChat);
    }
}

#[test]
fn native_identity_requires_matching_protocol() {
    let mut entry = bundled().remove(0);
    assert_eq!(entry.preset.id, "openai");
    entry.preset.logo = None;
    let mut route = entry.preset.routes[0].clone();
    assert_eq!(kind(&entry, &route), Kind::OpenAi);
    route.protocol = Protocol::Anthropic;
    assert_eq!(kind(&entry, &route), Kind::CompatibleAnthropic);
    route.protocol = Protocol::OpenAiResponses;
    entry.source = Source::Package {
        name: "fixture".into(),
    };
    assert_eq!(kind(&entry, &route), Kind::CompatibleResponses);
}

#[test]
fn resolves_group_labels_from_snapshot() {
    let entries = bundled();
    let mut base = entries
        .iter()
        .find(|entry| entry.preset.id == "openai")
        .unwrap()
        .clone();
    let mut method = entries
        .iter()
        .find(|entry| entry.preset.id == "compatible-responses")
        .unwrap()
        .clone();
    base.preset.name = "Execution Node brand".into();
    base.preset.locales.clear();
    base.preset.brand_display = None;
    method.preset.brand_display = None;
    let mut foreign = base.clone();
    foreign.source = Source::Partner;
    foreign.preset.name = "Different source brand".into();
    let entries = presentation(vec![method, foreign, base]);
    assert_eq!(brand_label(&entries[0]), "Execution Node brand");
    assert_eq!(brand_label(&entries[1]), "Different source brand");
    assert_eq!(presentation(entries.clone()), entries);
}

#[test]
fn endpoint_access_uses_the_declaration() {
    let mut entry = bundled().remove(0);
    let mut route = entry.preset.routes[0].clone();
    assert!(!custom_endpoint(&entry, &route));
    route.endpoint.clear();
    assert!(custom_endpoint(&entry, &route));
    route.endpoint = "https://fixture.invalid/v1".into();
    entry.source = Source::Partner;
    assert!(custom_endpoint(&entry, &route));
}

#[test]
fn product_labels_use_declared_display() {
    use sailry_protocol::conversation::connection::Product;
    let mut entry = bundled()
        .into_iter()
        .find(|entry| entry.preset.id == "qwen")
        .unwrap();
    assert_eq!(
        product_label(&entry.preset, Product::Coding).as_ref(),
        "Qwen Coding Plan"
    );
    let product = entry
        .preset
        .products
        .iter_mut()
        .find(|product| product.kind == Product::Coding)
        .unwrap();
    product.display = None;
    assert_eq!(
        product_label(&entry.preset, Product::Coding),
        crate::tr("provider_coding")
    );
}
