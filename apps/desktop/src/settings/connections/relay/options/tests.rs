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
fn starts_with_only_the_actual_selection() {
    let empty = initial(None);
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[0].choice, Choice::Custom);
    for selection in [Selection::Iroh, network("retained")] {
        let items = initial(Some(&selection));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].choice, Choice::Retained(selection));
        assert_eq!(items[1].choice, Choice::Custom);
    }
}

#[test]
fn empty_catalog_preserves_selected_network() {
    let actual = network("retained");
    let previous = initial(Some(&actual));
    let (items, selected) = merge(vec![], &previous, Some(&previous[0].choice));
    assert_eq!(selected, Some(previous[0].choice.clone()));
    assert_eq!(items.len(), 2);
    assert_eq!(selection(selected.as_ref(), "", None), Some(actual));
    let (items, selected) = merge(vec![], &items, Some(&Choice::Custom));
    assert_eq!(items.len(), 1);
    assert_eq!(selected, Some(Choice::Custom));
}

#[test]
fn refreshes_labels_without_changing_destinations() {
    let actual = network("same");
    let previous = initial(Some(&actual));
    let mut updated = preset("dynamic", actual.clone());
    updated
        .label
        .locales
        .insert("zh-CN".into(), "动态服务".into());
    let (items, selected) = merge(vec![updated], &previous, Some(&previous[0].choice));
    assert!(matches!(selected, Some(Choice::Preset { .. })));
    assert_eq!(selection(selected.as_ref(), "", None), Some(actual));
    assert_eq!(items[0].label.as_ref().unwrap().label("zh-CN"), "动态服务");
    assert_eq!(items.len(), 2);
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
fn equal_destinations_keep_the_selected_identity() {
    let actual = network("same");
    let presets = vec![preset("first", actual.clone()), preset("selected", actual)];
    let (previous, _) = merge(presets.clone(), &[], None);
    let selected = previous[1].choice.clone();
    let (_, current) = merge(presets, &previous, Some(&selected));
    assert_eq!(current, Some(selected));
}

#[test]
fn preserves_removed_and_changed_destinations() {
    let old = network("old");
    let (previous, _) = merge(vec![preset("service", old.clone())], &[], None);
    for presets in [vec![], vec![preset("service", network("new"))]] {
        let (items, selected) = merge(presets.clone(), &previous, Some(&previous[0].choice));
        assert_eq!(selected, Some(Choice::Retained(old.clone())));
        assert_eq!(selection(selected.as_ref(), "", None), Some(old.clone()));
        assert_eq!(items.len(), presets.len() + 2);
        if let Some(preset) = presets.first() {
            assert_eq!(items[1].choice.selection(), Some(preset.selection.clone()));
        }
    }
}

#[test]
fn retains_only_the_current_network() {
    let previous = initial(Some(&network("before")));
    let actual = network("after");
    let (items, selected) = retain(&previous, Some(&actual));
    assert_eq!(items.len(), 2);
    assert_eq!(selected, Choice::Retained(actual));
    assert_eq!(items[1].choice, Choice::Custom);
}

#[test]
fn custom_keeps_full_saved_relay_set() {
    let saved = Selection::Custom {
        relays: vec![
            "https://first.example".into(),
            "https://second.example".into(),
        ],
    };
    assert_eq!(
        selection(
            Some(&Choice::Custom),
            "  https://first.example  ",
            Some(&saved)
        ),
        Some(saved.clone())
    );
    assert_eq!(
        selection(
            Some(&Choice::Custom),
            "https://changed.example",
            Some(&saved)
        ),
        Some(network("changed"))
    );
    assert_eq!(selection(Some(&Choice::Custom), " ", Some(&saved)), None);
    assert_eq!(selection(None, "https://first.example", Some(&saved)), None);
}

#[test]
fn explicit_preset_keeps_its_complete_set() {
    let saved = Selection::Custom {
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
    let value = Choice::Preset {
        id: "dynamic".into(),
        selection: chosen.clone(),
    };
    assert_eq!(
        selection(Some(&value), "https://first.example", Some(&saved)),
        Some(chosen)
    );
    assert_eq!(
        selection(
            Some(&Choice::Retained(Selection::Iroh)),
            "https://first.example",
            Some(&saved)
        ),
        Some(Selection::Iroh)
    );
}

#[test]
fn custom_choice_has_separate_identity() {
    let (items, _) = merge(
        vec![preset("custom", network("custom"))],
        &initial(None),
        Some(&Choice::Custom),
    );
    assert_ne!(items[0].choice, Choice::Custom);
    assert_eq!(items[1].choice, Choice::Custom);
}

#[test]
fn missing_latency_never_becomes_zero() {
    assert_eq!(Timing::Unknown.text(), crate::tr("composer_metric_unknown"));
    assert_eq!(Timing::Measured(None).text(), Timing::Unknown.text());
    assert_ne!(Timing::Measured(Some(0)).text(), Timing::Unknown.text());
    assert!(Timing::Measured(Some(0)).text().contains('0'));
}
