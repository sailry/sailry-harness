use super::*;
use core::prelude::v1::test;

fn declared(package: &str, id: &str, brand: &str) -> PresetEntry {
    let mut entry = presets::bundled().remove(0);
    entry.source = Source::Package {
        name: package.into(),
    };
    entry.preset.id = id.into();
    entry.preset.name = format!("Method {id}");
    entry.preset.locales.clear();
    entry.preset.brand = Some(brand.into());
    entry.preset.brand_display = Some(sailry_protocol::plugin::desktop::Navigation {
        label: "Declared family".into(),
        locales: Default::default(),
        icon: None,
    });
    entry.icon = Some("data:image/svg+xml;base64,PHN2Zy8+".into());
    entry
}

#[test]
fn shares_add_groups_and_order() {
    let mut entries = presets::bundled();
    entries.extend([
        declared("first", "china", "provider"),
        declared("first", "global", "provider"),
        declared("second", "global", "provider"),
    ]);
    let groups = presets::groups(&entries);
    let choices = choices(&entries, &[], &Selection::All, &[]);
    assert_eq!(choices[0].selection, Selection::All);
    assert_eq!(choices.len(), groups.len() + 1);
    for (choice, (key, group)) in choices[1..].iter().zip(&groups) {
        assert_eq!(choice.selection, Selection::Group(key.clone()));
        assert_eq!(choice.label, presets::brand_label(&group[0]));
        assert_eq!(choice.entry.as_ref(), Some(&group[0]));
    }
    let (_, openai) = groups.iter().find(|(key, _)| key == "openai").unwrap();
    assert!(
        openai
            .iter()
            .any(|entry| entry.preset.id == "chat-gpt-oauth")
    );
    let (_, opencode) = groups.iter().find(|(key, _)| key == "opencode").unwrap();
    assert!(
        opencode
            .iter()
            .any(|entry| entry.preset.id == "opencode-go")
    );
    assert!(
        opencode
            .iter()
            .any(|entry| entry.preset.id == "opencode-zen")
    );
    assert_eq!(
        groups
            .iter()
            .filter(|(key, _)| key == "package:first:provider")
            .count(),
        1
    );
    assert_ne!(
        presets::brand(&entries[entries.len() - 2]),
        presets::brand(entries.last().unwrap())
    );
    assert_eq!(Store::default().category, Selection::All);
}

#[test]
fn matches_all_methods_in_declared_family() {
    let first = declared("sample", "china", "provider");
    let second = declared("sample", "global", "provider");
    let third = declared("other", "global", "provider");
    let mut channels = vec![Store::default().channels.remove(0); 3];
    for (channel, entry) in channels.iter_mut().zip([first, second, third]) {
        channel.directory = Some(entry);
    }
    let selected = selection(&channels[0], None);
    assert_eq!(
        channels
            .iter()
            .filter(|channel| selection(channel, None) == selected)
            .count(),
        2
    );
    assert_ne!(selected, selection(&channels[2], None));
    let mut legacy = channels[0].clone();
    legacy.directory = None;
    legacy.preset = Preset::CompatibleResponses;
    assert_eq!(selection(&legacy, None), Selection::Group("openai".into()));
    legacy.preset = Preset::OpenCodeZen;
    assert_eq!(
        selection(&legacy, None),
        Selection::Group("opencode".into())
    );
}

#[test]
fn preserves_selection_after_refresh() {
    let first = declared("sample", "china", "provider");
    let second = declared("sample", "global", "provider");
    let selected = Selection::Group(presets::brand(&first));
    for entries in [[first.clone(), second.clone()], [second, first]] {
        let choices = choices(&entries, &[], &selected, &[]);
        let matching = choices
            .iter()
            .filter(|choice| choice.selection == selected)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].label, "Declared family");
    }
    let missing = choices(&[], std::slice::from_ref(&selected), &selected, &[]);
    assert_eq!(missing.len(), 2);
    assert_eq!(missing[1].selection, selected);
}

#[test]
fn preserves_unknown_saved_identity() {
    let reference = sailry_protocol::conversation::presets::PresetRef {
        source: Source::Package {
            name: "sample".into(),
        },
        id: "unlisted".into(),
        route: None,
    };
    let provider: Provider = serde_json::from_value(serde_json::json!({
        "id":sailry_protocol::ProviderId::new(),"revision":1,"name":"Custom channel",
        "api":"chat_completions","authentication":"api_key","endpoint":"https://example.test/v1",
        "enabled":true,"models":[],"default_model":"","credential":null,"preset":reference,
    }))
    .unwrap();
    let mut channel = Store::default().channels.remove(0);
    channel.directory = Some(declared("different", "unlisted", "provider"));
    assert_eq!(
        selection(&channel, Some(&provider)),
        Selection::Group("package:sample:unlisted".into())
    );
}
