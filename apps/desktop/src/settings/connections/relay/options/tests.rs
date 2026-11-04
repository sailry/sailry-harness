use super::{Choice, Timing, initial, merge, retain, selection};
use core::prelude::v1::test;
use sailry_protocol::{
    network::{Preset, Selection},
    plugin::desktop::Navigation,
};

fn network(host: &str) -> Selection {
    Selection::Custom {
        relays: vec![format!("https://{host}.example")],
    }
}

fn preset(id: &str, selection: Selection) -> Preset {
    Preset {
        id: id.into(),
        label: Navigation {
            label: id.into(),
            locales: Default::default(),
            icon: None,
        },
        selection,
    }
}

#[test]
fn initial_and_retained_networks() {
    let empty = initial(None);
    assert_eq!(empty.len(), 1, "empty: count");
    assert_eq!(empty[0].choice, Choice::Custom, "empty: custom choice");
    let previous = initial(Some(&network("before")));
    for (case, selected) in [
        ("default", Selection::Iroh),
        ("saved", network("retained")),
        ("changed", network("after")),
    ] {
        let items = initial(Some(&selected));
        assert_eq!(items.len(), 2, "{case}: initial count");
        assert_eq!(
            items[0].choice,
            Choice::Retained(selected.clone()),
            "{case}: initial selection"
        );
        assert_eq!(items[1].choice, Choice::Custom, "{case}: initial custom");
        let (items, choice) = retain(&previous, Some(&selected));
        assert_eq!(items.len(), 2, "{case}: retained count");
        assert_eq!(
            choice,
            Choice::Retained(selected),
            "{case}: retained selection"
        );
        assert_eq!(items[1].choice, Choice::Custom, "{case}: retained custom");
    }
}

#[test]
fn catalog_refreshes() {
    let actual = network("retained");
    let previous = initial(Some(&actual));
    let (items, selected) = merge(vec![], &previous, Some(&previous[0].choice));
    assert_eq!(
        selected,
        Some(previous[0].choice.clone()),
        "empty catalog: selection"
    );
    assert_eq!(items.len(), 2, "empty catalog: count");
    assert_eq!(
        selection(selected.as_ref(), "", None),
        Some(actual.clone()),
        "empty catalog: destination"
    );
    let (items, selected) = merge(vec![], &items, Some(&Choice::Custom));
    assert_eq!(items.len(), 1, "custom selection: count");
    assert_eq!(selected, Some(Choice::Custom), "custom selection: identity");

    let actual = network("same");
    let previous = initial(Some(&actual));
    let mut updated = preset("dynamic", actual.clone());
    updated
        .label
        .locales
        .insert("zh-CN".into(), "动态服务".into());
    let (items, selected) = merge(vec![updated], &previous, Some(&previous[0].choice));
    assert!(
        matches!(selected, Some(Choice::Preset { .. })),
        "updated label: catalog identity"
    );
    assert_eq!(
        selection(selected.as_ref(), "", None),
        Some(actual.clone()),
        "updated label: destination"
    );
    assert_eq!(
        items[0].label.as_ref().unwrap().label("zh-CN"),
        "动态服务",
        "updated label: localization"
    );
    assert_eq!(items.len(), 2, "updated label: count");
    let presets = vec![preset("first", actual.clone()), preset("selected", actual)];
    let (previous, _) = merge(presets.clone(), &[], None);
    let selected = previous[1].choice.clone();
    let (_, current) = merge(presets, &previous, Some(&selected));
    assert_eq!(
        current,
        Some(selected),
        "equal destinations: selected identity"
    );

    let old = network("old");
    let (previous, _) = merge(vec![preset("service", old.clone())], &[], None);
    for (case, presets) in [
        ("removed", vec![]),
        ("changed", vec![preset("service", network("new"))]),
    ] {
        let (items, selected) = merge(presets.clone(), &previous, Some(&previous[0].choice));
        assert_eq!(
            selected,
            Some(Choice::Retained(old.clone())),
            "{case}: retained identity"
        );
        assert_eq!(
            selection(selected.as_ref(), "", None),
            Some(old.clone()),
            "{case}: retained destination"
        );
        assert_eq!(items.len(), presets.len() + 2, "{case}: count");
        if let Some(preset) = presets.first() {
            assert_eq!(
                items[1].choice.selection(),
                Some(preset.selection.clone()),
                "{case}: new destination"
            );
        }
    }
    let (items, _) = merge(
        vec![preset("custom", network("custom"))],
        &initial(None),
        Some(&Choice::Custom),
    );
    assert_ne!(
        items[0].choice,
        Choice::Custom,
        "custom preset: distinct identity"
    );
    assert_eq!(
        items[1].choice,
        Choice::Custom,
        "custom preset: editable choice"
    );
}

#[test]
fn keeps_catalog_order_during_latency_updates() {
    let presets = vec![
        preset("slow", network("first")),
        preset("fast", network("second")),
    ];
    let (mut previous, _) = merge(presets.clone(), &initial(None), Some(&Choice::Custom));
    previous[0].timing = Timing::Measured(Some(240));
    previous[1].timing = Timing::Measured(Some(4));
    previous[2].timing = Timing::Pending;
    let selected = previous[0].choice.clone();
    let (items, current) = merge(presets, &previous, Some(&selected));
    assert_eq!(current, Some(selected.clone()));
    assert_eq!(items[0].choice, selected);
    assert_eq!(items[1].choice, previous[1].choice);
    assert_eq!(items[0].timing, Timing::Measured(Some(240)));
    assert_eq!(items[1].timing, Timing::Measured(Some(4)));
    assert_eq!(items[2].timing, Timing::Pending);
}

#[test]
fn exact_destinations_and_custom_edits() {
    let saved = Selection::Custom {
        relays: vec![
            "https://first.example".into(),
            "https://second.example".into(),
        ],
    };
    let explicit_saved = Selection::Custom {
        relays: vec![
            "https://first.example".into(),
            "https://saved.example".into(),
        ],
    };
    let chosen = Selection::Custom {
        relays: vec![
            "https://first.example".into(),
            "https://selected.example".into(),
        ],
    };
    for (case, choice, address, previous, expected) in [
        (
            "unchanged custom",
            Some(Choice::Custom),
            "  https://first.example  ",
            &saved,
            Some(saved.clone()),
        ),
        (
            "edited custom",
            Some(Choice::Custom),
            "https://changed.example",
            &saved,
            Some(network("changed")),
        ),
        ("empty custom", Some(Choice::Custom), " ", &saved, None),
        ("no choice", None, "https://first.example", &saved, None),
        (
            "explicit preset",
            Some(Choice::Preset {
                id: "dynamic".into(),
                selection: chosen.clone(),
            }),
            "https://first.example",
            &explicit_saved,
            Some(chosen.clone()),
        ),
        (
            "retained default",
            Some(Choice::Retained(Selection::Iroh)),
            "https://first.example",
            &explicit_saved,
            Some(Selection::Iroh),
        ),
    ] {
        assert_eq!(
            selection(choice.as_ref(), address, Some(previous)),
            expected,
            "{case}"
        );
    }
}

#[test]
fn missing_latency_never_becomes_zero() {
    assert_eq!(Timing::Unknown.text(), crate::tr("composer_metric_unknown"));
    assert_eq!(Timing::Measured(None).text(), Timing::Unknown.text());
    assert_ne!(Timing::Measured(Some(0)).text(), Timing::Unknown.text());
    assert!(Timing::Measured(Some(0)).text().contains('0'));
}
