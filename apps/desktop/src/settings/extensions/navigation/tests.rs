use super::*;
use crate::settings::extensions::modal;
use sailry_protocol::plugin::desktop::{Icon, SettingsGroup};
use serde_json::json;

fn info(entry: Option<&str>) -> Info {
    Info {
        summary: Summary {
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
fn registers_declared_pages() {
    let node = NodeId([1; 32]);
    for entry in [Some("dev.sailry.platform/settings.js"), None] {
        let mut page = info(entry);
        assert!(modal::has_panel(&page));
        assert!(modal::configurable(&page));
        assert!(Entry::new(node, &page, &page.summary).is_some());
        page.settings = None;
        assert!(modal::configurable(&page));
        assert!(Entry::new(node, &page, &page.summary).is_some());
    }
    let mut dialog = info(None);
    dialog.extension.as_mut().unwrap().settings_page = None;
    assert!(modal::configurable(&dialog));
    assert!(!modal::has_panel(&dialog));
    assert!(Entry::new(node, &dialog, &dialog.summary).is_none());
    dialog.settings = None;
    assert!(!modal::configurable(&dialog));
}

#[test]
fn preserves_navigation_and_enable_choices() {
    let node = NodeId([1; 32]);
    let mut page = info(None);
    page.summary.enabled = false;
    let entry = Entry::new(node, &page, &page.summary).unwrap();
    assert_eq!(entry.placement.group, SettingsGroup::Ai);
    assert_eq!(entry.placement.order, 500);
    assert_eq!(entry.navigation.label("en"), "Example");
    assert_eq!(entry.navigation.label("zh-CN"), "Example localized");
    assert_eq!(
        entry.navigation.icon,
        Some(serde_json::from_value::<Icon>(json!("reicon:devices/cpu")).unwrap())
    );
    assert!(entry.available(node, &[page.summary.clone()]));
    page.summary.enabled = true;
    page.summary.revision += 1;
    assert_eq!(Entry::new(node, &page, &page.summary), Some(entry));
}

#[test]
fn merges_declared_group_and_order() {
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
    assert!(matches!(
        items.as_slice(),
        [
            Item::Core(Section::Providers),
            Item::Plugin(_),
            Item::Core(Section::Knowledge)
        ]
    ));
    assert!(group(SettingsGroup::Tools, &[], &pages).is_empty());
}

#[test]
fn selects_its_own_menu_entry() {
    let node = NodeId([1; 32]);
    for renderer in [Some("dev.sailry.platform/settings.js"), None] {
        let page = info(renderer);
        let entry = Entry::new(node, &page, &page.summary).unwrap();
        let item = Item::Plugin(entry);
        assert!(item.selected(Section::Plugin, Some("example")));
        assert!(!item.selected(Section::Plugin, Some("another")));
        assert!(!item.selected(Section::Plugins, Some("example")));
        assert!(!Item::Core(Section::Plugins).selected(Section::Plugin, Some("example")));
    }
}

#[test]
fn rejects_stale_routes_and_scopes() {
    let node = NodeId([1; 32]);
    let page = info(None);
    let entry = Entry::new(node, &page, &page.summary).unwrap();
    assert!(!entry.available(NodeId([2; 32]), std::slice::from_ref(&page.summary)));
    assert!(!entry.available(node, &[]));
    let mut current = page.summary.clone();
    current.settings_revision += 1;
    current.revision += 1;
    assert!(!entry.available(node, &[current.clone()]));
    let updated = Entry::new(node, &page, &current).unwrap();
    assert_eq!(updated.package, current.reference());
    assert!(updated.available(node, &[current.clone()]));
    current.digest = "replacement-digest".into();
    assert!(Entry::new(node, &page, &current).is_none());
    assert!(!entry.available(node, &[current.clone()]));
    current = page.summary.clone();
    current.name = "replacement".into();
    assert!(Entry::new(node, &page, &current).is_none());
}
