use super::*;
use core::prelude::v1::test;

#[test]
fn directory_and_routes() {
    for (scenario, check) in [
        (
            "bound_directory_never_uses_local_choices",
            (|_scenario: &str| {
                let local = available(None);
                assert!(
                    !groups(&local).is_empty(),
                    "bound directory never uses local choices"
                );
                let mut received = Vec::new();
                // A bound Node has no choices before its first directory response.
                assert!(
                    available(Some(&received)).is_empty(),
                    "bound directory never uses local choices"
                );
                let mut entry = local[0].clone();
                entry.source = Source::Partner;
                entry.preset.id = "node-only".into();
                entry.preset.brand = None;
                entry.preset.brand_display = None;
                entry.preset.name = "Execution Node method".into();
                entry.preset.routes.truncate(1);
                entry.preset.routes[0].endpoint = "https://node.example/v1".into();
                received.push(entry);
                assert_eq!(
                    available(Some(&received)),
                    received,
                    "bound directory never uses local choices"
                );
                // A successful empty response must not restore local choices either.
                received.clear();
                assert!(
                    available(Some(&received)).is_empty(),
                    "bound directory never uses local choices"
                );
            }) as fn(&str),
        ),
        (
            "preserves_builtin_icons_and_methods",
            (|_scenario: &str| {
                let entries = bundled();
                let groups = groups(&entries);
                let (_, openai) = groups.iter().find(|(key, _)| key == "openai").unwrap();
                assert!(
                    openai
                        .iter()
                        .any(|entry| entry.preset.id == "chat-gpt-oauth"),
                    "preserves builtin icons and methods"
                );
                assert!(
                    openai
                        .iter()
                        .any(|entry| entry.preset.id == "compatible-responses"),
                    "preserves builtin icons and methods"
                );
                for entry in &entries {
                    assert_eq!(
                        groups
                            .iter()
                            .flat_map(|(_, entries)| entries)
                            .any(|choice| choice.preset.id == entry.preset.id),
                        entry.preset.offered,
                        "preserves builtin icons and methods"
                    );
                    if entry.preset.offered {
                        assert!(
                            entry
                                .preset
                                .logo
                                .as_ref()
                                .is_some_and(|path| path.starts_with("icons/providers/")),
                            "preserves builtin icons and methods"
                        );
                    }
                    assert!(
                        !entry.preset.routes.is_empty(),
                        "preserves builtin icons and methods"
                    );
                    for route in &entry.preset.routes {
                        assert_eq!(
                            kind(entry, route).api(),
                            route.protocol.api(),
                            "preserves builtin icons and methods"
                        );
                    }
                }
            }),
        ),
        (
            "keeps_package_brands_separate",
            (|_scenario: &str| {
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
                    1,
                    "keeps package brands separate"
                );
                assert_ne!(key, brand(&entries[0]), "keeps package brands separate");
            }),
        ),
        (
            "prefers_declared_brand_glyphs",
            (|_scenario: &str| {
                let mut plugin = bundled().remove(0);
                plugin.source = Source::Package {
                    name: "sample.models".into(),
                };
                plugin.preset.logo = None;
                assert_eq!(
                    glyph(&plugin),
                    Glyph::Fallback,
                    "prefers declared brand glyphs"
                );
                plugin.icon = Some("data:image/svg+xml;base64,PHN2Zy8+".into());
                assert_eq!(
                    glyph(&plugin),
                    Glyph::Image(ImageFormat::Svg, b"<svg/>".to_vec()),
                    "prefers declared brand glyphs"
                );
                let declared =
                    sailry_protocol::plugin::desktop::Icon::Name("reicon:messages/chat".into());
                plugin.preset.brand_display = Some(sailry_protocol::plugin::desktop::Navigation {
                    label: "Sample".into(),
                    locales: Default::default(),
                    icon: Some(declared.clone()),
                });
                assert_eq!(
                    glyph(&plugin),
                    Glyph::Declared(&declared),
                    "prefers declared brand glyphs"
                );
            }),
        ),
        (
            "retains_intrinsic_and_neutral_paint",
            (|_scenario: &str| {
                let groups = groups(&bundled());
                assert_eq!(groups.len(), 14, "retains intrinsic and neutral paint");
                assert_eq!(
                    groups
                        .iter()
                        .filter(|(_, entries)| matches!(
                            glyph(&entries[0]),
                            Glyph::Image(ImageFormat::Svg, _)
                        ))
                        .count(),
                    10,
                    "retains intrinsic and neutral paint"
                );
                assert_eq!(
                    groups
                        .iter()
                        .filter(|(_, entries)| glyph(&entries[0]) == Glyph::Fallback)
                        .count(),
                    4,
                    "retains intrinsic and neutral paint"
                );
                let mut entry = groups[0].1[0].clone();
                entry.source = Source::Package {
                    name: "sample.models".into(),
                };
                entry.icon = None;
                for (svg, colored) in [
                    (
                        r##"<svg viewBox="0 0 24 24"><path fill="#6688ff" d="M0 0h24v24z"/></svg>"##,
                        true,
                    ),
                    (
                        r#"<svg viewBox="0 0 24 24"><path fill="none" stroke="currentColor" d="M0 0h24v24z"/></svg>"#,
                        false,
                    ),
                ] {
                    let declaration =
                        sailry_protocol::plugin::desktop::Icon::Svg { svg: svg.into() };
                    entry.preset.brand_display =
                        Some(sailry_protocol::plugin::desktop::Navigation {
                            label: "Sample".into(),
                            locales: Default::default(),
                            icon: Some(declaration.clone()),
                        });
                    assert_eq!(
                        intrinsic(svg),
                        colored,
                        "retains intrinsic and neutral paint"
                    );
                    assert_eq!(
                        glyph(&entry),
                        if colored {
                            Glyph::Image(ImageFormat::Svg, svg.as_bytes().to_vec())
                        } else {
                            Glyph::Declared(&declaration)
                        },
                        "retains intrinsic and neutral paint"
                    );
                }
            }),
        ),
        (
            "tints_only_vector_glyphs",
            (|_scenario: &str| {
                let declared =
                    sailry_protocol::plugin::desktop::Icon::Name("reicon:messages/chat".into());
                for (glyph, expected) in [
                    (Glyph::Declared(&declared), true),
                    (Glyph::Image(ImageFormat::Svg, b"<svg/>".to_vec()), true),
                    (Glyph::Fallback, true),
                    (Glyph::Image(ImageFormat::Png, vec![0]), false),
                    (Glyph::Remote("https://example.test/logo.png"), false),
                ] {
                    assert_eq!(tinted(&glyph), expected, "tints only vector glyphs: {glyph:?}");
                }
                assert!(
                    groups(&bundled())
                        .iter()
                        .all(|(_, entries)| tinted(&glyph(&entries[0]))),
                    "tints only vector glyphs"
                );
            }),
        ),
        (
            "routes_outweigh_brand_glyphs",
            (|_scenario: &str| {
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
                    assert_eq!(
                        kind(&entry, &route),
                        expected,
                        "routes outweigh brand glyphs"
                    );
                    assert_eq!(
                        kind(&entry, &route).api(),
                        protocol.api(),
                        "routes outweigh brand glyphs"
                    );
                    assert_eq!(
                        groups(std::slice::from_ref(&entry))[0].0,
                        "fixture-vendor",
                        "routes outweigh brand glyphs"
                    );
                    assert!(
                        !custom_endpoint(&entry, &route),
                        "routes outweigh brand glyphs"
                    );
                }
            }),
        ),
        (
            "preserves_existing_vendor_categories",
            (|_scenario: &str| {
                use super::super::{data::Category, vendors::Vendor};
                let entries = bundled();
                // Existing picker categories from Sailry 3ae18182; route data remains declarative.
                for vendor in Vendor::FIRST_PARTY {
                    let entry = entries
                        .iter()
                        .find(|entry| entry.preset.id == vendor.id())
                        .unwrap();
                    for route in &entry.preset.routes {
                        assert_eq!(
                            kind(entry, route).category(),
                            Category::Vendor(vendor),
                            "preserves existing vendor categories: {}",
                            vendor.id()
                        );
                        assert_eq!(
                            kind(entry, route).api(),
                            route.protocol.api(),
                            "preserves existing vendor categories"
                        );
                    }
                }
                for id in [
                    "groq",
                    "openrouter",
                    "fireworks",
                    "together",
                    "perplexity",
                    "cerebras",
                    "sambanova",
                ] {
                    let entry = entries.iter().find(|entry| entry.preset.id == id).unwrap();
                    assert!(
                        !entry.preset.offered,
                        "preserves existing vendor categories: {id}"
                    );
                    for route in &entry.preset.routes {
                        assert_eq!(
                            kind(entry, route),
                            Kind::CompatibleChat,
                            "preserves existing vendor categories: {id}"
                        );
                        assert_eq!(
                            kind(entry, route).category(),
                            Category::OpenAi,
                            "preserves existing vendor categories: {id}"
                        );
                        assert_eq!(
                            kind(entry, route).api(),
                            route.protocol.api(),
                            "preserves existing vendor categories"
                        );
                    }
                }
            }),
        ),
        (
            "known_identity_never_overrides_protocols",
            (|_scenario: &str| {
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
                        assert_eq!(
                            kind(&entry, &route),
                            expected,
                            "known identity never overrides protocols: {}",
                            vendor.id()
                        );
                        assert_eq!(
                            kind(&entry, &route).api(),
                            route.protocol.api(),
                            "known identity never overrides protocols"
                        );
                    }
                    route.protocol = Protocol::OpenAiChatCompletions;
                    if vendor == Vendor::DeepSeek {
                        assert_eq!(
                            kind(&entry, &route),
                            Kind::CompatibleChat,
                            "known identity never overrides protocols"
                        );
                        route.protocol = Protocol::DeepSeek;
                    }
                    assert_eq!(
                        kind(&entry, &route),
                        Kind::Hosted(vendor),
                        "known identity never overrides protocols"
                    );
                    route.protocol = Protocol::OpenAiChatCompletions;
                    entry.source = Source::Package {
                        name: "fixture".into(),
                    };
                    assert_eq!(
                        kind(&entry, &route),
                        Kind::CompatibleChat,
                        "known identity never overrides protocols"
                    );
                }
            }),
        ),
        (
            "native_identity_requires_matching_protocol",
            (|_scenario: &str| {
                let mut entry = bundled().remove(0);
                assert_eq!(
                    entry.preset.id, "openai",
                    "native identity requires matching protocol"
                );
                entry.preset.logo = None;
                let mut route = entry.preset.routes[0].clone();
                assert_eq!(
                    kind(&entry, &route),
                    Kind::OpenAi,
                    "native identity requires matching protocol"
                );
                route.protocol = Protocol::Anthropic;
                assert_eq!(
                    kind(&entry, &route),
                    Kind::CompatibleAnthropic,
                    "native identity requires matching protocol"
                );
                route.protocol = Protocol::OpenAiResponses;
                entry.source = Source::Package {
                    name: "fixture".into(),
                };
                assert_eq!(
                    kind(&entry, &route),
                    Kind::CompatibleResponses,
                    "native identity requires matching protocol"
                );
            }),
        ),
        (
            "resolves_group_labels_from_snapshot",
            (|_scenario: &str| {
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
                assert_eq!(
                    brand_label(&entries[0]),
                    "Execution Node brand",
                    "resolves group labels from snapshot"
                );
                assert_eq!(
                    brand_label(&entries[1]),
                    "Different source brand",
                    "resolves group labels from snapshot"
                );
                assert_eq!(
                    presentation(entries.clone()),
                    entries,
                    "resolves group labels from snapshot"
                );
            }),
        ),
        (
            "endpoint_access_uses_the_declaration",
            (|_scenario: &str| {
                let mut entry = bundled().remove(0);
                let mut route = entry.preset.routes[0].clone();
                assert!(
                    !custom_endpoint(&entry, &route),
                    "endpoint access uses the declaration"
                );
                route.endpoint.clear();
                assert!(
                    custom_endpoint(&entry, &route),
                    "endpoint access uses the declaration"
                );
                route.endpoint = "https://fixture.invalid/v1".into();
                entry.source = Source::Partner;
                assert!(
                    custom_endpoint(&entry, &route),
                    "endpoint access uses the declaration"
                );
            }),
        ),
        (
            "product_labels_use_declared_display",
            (|_scenario: &str| {
                use sailry_protocol::conversation::connection::Product;
                let mut entry = bundled()
                    .into_iter()
                    .find(|entry| entry.preset.id == "qwen")
                    .unwrap();
                assert_eq!(
                    product_label(&entry.preset, Product::Coding).as_ref(),
                    "Qwen Coding Plan",
                    "product labels use declared display"
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
                    crate::tr("provider_coding"),
                    "product labels use declared display"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
