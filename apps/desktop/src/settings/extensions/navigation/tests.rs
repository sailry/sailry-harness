use super::*;
use crate::settings::extensions::modal;
use sailry_protocol::plugin::desktop::{Icon, SettingsGroup};
use serde_json::json;

fn info(entry: Option<&str>) -> Info {
    Info {
        versions: Vec::new(),
        summary: Summary {
            compatibility: Default::default(),
            name: "example".into(),
            revision: 1,
            digest: "example-digest".into(),
            settings_revision: 2,
            enabled: true,
            version: Some("0.1.0".into()),
            description: None,
        },
        origin: None,
        release_notes: None,
        icon: None,
        skill: None,
        mcp_source: None,
        extension: Some(
            serde_json::from_value(json!({
                "api_version": "v1",
                "settings_page": {
                    "navigation": {
                        "label": "Example",
                        "icon": "reicon:devices/cpu",
                        "locales": {"zh-CN": "Example localized"}
                    },
                    "placement": {"group": "ai", "order": 500},
                    "entry": entry
                }
            }))
            .unwrap(),
        ),
        skills: vec![],
        mcp: vec![],
        settings: Some(
            serde_json::from_value(json!({
                "$schema": sailry_protocol::plugin::settings::SCHEMA,
                "type": "object",
                "properties": {"enabled": {"type": "boolean"}},
                "additionalProperties": false
            }))
            .unwrap(),
        ),
        issues: vec![],
    }
}

#[test]
fn declared_pages_and_scope() {
    for (scenario, check) in [
        (
            "registers_declared_pages",
            (|_scenario: &str| {
                let node = NodeId([1; 32]);
                for entry in [Some("dev.sailry.platform/settings.js"), None] {
                    let mut page = info(entry);
                    assert!(modal::has_panel(&page), "registers declared pages");
                    assert!(modal::configurable(&page), "registers declared pages");
                    assert!(
                        Entry::new(node, &page, &page.summary).is_some(),
                        "registers declared pages"
                    );
                    page.settings = None;
                    assert!(modal::configurable(&page), "registers declared pages");
                    assert!(
                        Entry::new(node, &page, &page.summary).is_some(),
                        "registers declared pages"
                    );
                }
                let mut dialog = info(None);
                dialog.extension.as_mut().unwrap().settings_page = None;
                assert!(modal::configurable(&dialog), "registers declared pages");
                assert!(!modal::has_panel(&dialog), "registers declared pages");
                assert!(
                    Entry::new(node, &dialog, &dialog.summary).is_none(),
                    "registers declared pages"
                );
                dialog.settings = None;
                assert!(!modal::configurable(&dialog), "registers declared pages");
            }) as fn(&str),
        ),
        (
            "preserves_navigation_and_enable_choices",
            (|_scenario: &str| {
                let node = NodeId([1; 32]);
                let mut page = info(None);
                page.summary.enabled = false;
                let entry = Entry::new(node, &page, &page.summary).unwrap();
                assert_eq!(
                    entry.placement.group,
                    SettingsGroup::Ai,
                    "preserves navigation and enable choices"
                );
                assert_eq!(
                    entry.placement.order, 500,
                    "preserves navigation and enable choices"
                );
                assert_eq!(
                    entry.navigation.label("en"),
                    "Example",
                    "preserves navigation and enable choices"
                );
                assert_eq!(
                    entry.navigation.label("zh-CN"),
                    "Example localized",
                    "preserves navigation and enable choices"
                );
                assert_eq!(
                    entry.navigation.icon,
                    Some(serde_json::from_value::<Icon>(json!("reicon:devices/cpu")).unwrap()),
                    "preserves navigation and enable choices"
                );
                assert!(
                    entry.available(node, &[page.summary.clone()]),
                    "preserves navigation and enable choices"
                );
                page.summary.enabled = true;
                page.summary.revision += 1;
                assert_eq!(
                    Entry::new(node, &page, &page.summary),
                    Some(entry),
                    "preserves navigation and enable choices"
                );
            }),
        ),
        (
            "merges_declared_group_and_order",
            (|_scenario: &str| {
                let node = NodeId([1; 32]);
                let page = info(Some("dev.sailry.platform/settings.js"));
                let mut entry = Entry::new(node, &page, &page.summary).unwrap();
                entry.placement.order = 150;
                let pages = [entry];
                let items = group(
                    SettingsGroup::Ai,
                    &[Section::Knowledge, Section::Providers],
                    &pages,
                );
                assert!(
                    matches!(
                        items.as_slice(),
                        [
                            Item::Core(Section::Providers),
                            Item::Plugin(_),
                            Item::Core(Section::Knowledge)
                        ]
                    ),
                    "merges declared group and order"
                );
                assert!(
                    group(SettingsGroup::Tools, &[], &pages).is_empty(),
                    "merges declared group and order"
                );
            }),
        ),
        (
            "selects_its_own_menu_entry",
            (|_scenario: &str| {
                let node = NodeId([1; 32]);
                for renderer in [Some("dev.sailry.platform/settings.js"), None] {
                    let page = info(renderer);
                    let entry = Entry::new(node, &page, &page.summary).unwrap();
                    let item = Item::Plugin(entry);
                    assert!(
                        item.selected(Section::Plugin, Some("example")),
                        "selects its own menu entry"
                    );
                    assert!(
                        !item.selected(Section::Plugin, Some("another")),
                        "selects its own menu entry"
                    );
                    assert!(
                        !item.selected(Section::Plugins, Some("example")),
                        "selects its own menu entry"
                    );
                    assert!(
                        !Item::Core(Section::Plugins).selected(Section::Plugin, Some("example")),
                        "selects its own menu entry"
                    );
                }
            }),
        ),
        (
            "rejects_stale_routes_and_scopes",
            (|_scenario: &str| {
                let node = NodeId([1; 32]);
                let page = info(None);
                let entry = Entry::new(node, &page, &page.summary).unwrap();
                assert!(
                    !entry.available(NodeId([2; 32]), std::slice::from_ref(&page.summary)),
                    "rejects stale routes and scopes"
                );
                assert!(
                    !entry.available(node, &[]),
                    "rejects stale routes and scopes"
                );
                let mut current = page.summary.clone();
                current.settings_revision += 1;
                current.revision += 1;
                assert!(
                    !entry.available(node, &[current.clone()]),
                    "rejects stale routes and scopes"
                );
                let updated = Entry::new(node, &page, &current).unwrap();
                assert_eq!(
                    updated.package,
                    current.reference(),
                    "rejects stale routes and scopes"
                );
                assert!(
                    updated.available(node, &[current.clone()]),
                    "rejects stale routes and scopes"
                );
                current.digest = "replacement-digest".into();
                assert!(
                    Entry::new(node, &page, &current).is_none(),
                    "rejects stale routes and scopes"
                );
                assert!(
                    !entry.available(node, &[current.clone()]),
                    "rejects stale routes and scopes"
                );
                current = page.summary.clone();
                current.name = "replacement".into();
                assert!(
                    Entry::new(node, &page, &current).is_none(),
                    "rejects stale routes and scopes"
                );
            }),
        ),
        (
            "excludes_incompatible_pages",
            (|_scenario: &str| {
                let node = NodeId([1; 32]);
                let mut page = info(Some("dev.sailry.platform/settings.js"));
                let entry = Entry::new(node, &page, &page.summary).unwrap();
                page.summary.compatibility =
                    sailry_protocol::plugin::Compatibility::RequiresNewer {
                        required: "0.1.0-alpha.5".into(),
                    };
                assert!(
                    Entry::new(node, &page, &page.summary).is_none(),
                    "excludes incompatible pages"
                );
                assert!(
                    !entry.available(node, &[page.summary]),
                    "excludes incompatible pages"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
