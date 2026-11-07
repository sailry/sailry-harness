//! Declared model choices share the revision-checked composer configuration.
use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use sailry_protocol::conversation::features::{Feature, FeatureOption};
use std::collections::BTreeMap;

pub(in crate::conversation::live) fn retain(
    features: &[Feature],
    selected: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    selected
        .iter()
        .filter(|(id, value)| {
            features.iter().any(|feature| {
                feature.id == **id && feature.options.iter().any(|option| option.id == **value)
            })
        })
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect()
}

pub(super) struct Choice {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: Option<Icon>,
    pub fast: bool,
}

pub(super) fn selected(features: &[Feature], selected: &BTreeMap<String, String>) -> Vec<Choice> {
    features
        .iter()
        .filter_map(|feature| {
            let selected = selected.get(&feature.id)?;
            let option = feature
                .options
                .iter()
                .find(|option| option.id == *selected)?;
            Some(Choice {
                id: feature.id.clone().into(),
                label: label(feature, option),
                icon: if fast(feature, option) || speed(feature) {
                    Some(icon(feature, Some(option)))
                } else {
                    feature.icon.as_ref().map(crate::assets::icons::icon)
                },
                fast: speed(feature),
            })
        })
        .collect()
}

/// Speed choices from any provider cycle on one button instead of opening a menu.
fn speed(feature: &Feature) -> bool {
    feature.id == "speed"
}

fn fast(feature: &Feature, option: &FeatureOption) -> bool {
    speed(feature) && option.id == "fast"
}

fn label(feature: &Feature, option: &FeatureOption) -> SharedString {
    if fast(feature, option) {
        tr("composer_feature_fast")
    } else {
        option.name.as_ref().unwrap_or(&option.id).clone().into()
    }
}

/// The option's own icon, else the feature's; Fast keeps the standard Zap.
fn icon(feature: &Feature, option: Option<&FeatureOption>) -> Icon {
    let option = option.or_else(|| feature.options.first().filter(|_| speed(feature)));
    match option {
        Some(option) if fast(feature, option) => Icon::new(gpui_kit::assets::IconName::Zap),
        Some(option) if option.icon.is_some() => {
            crate::assets::icons::icon(option.icon.as_ref().unwrap())
        }
        _ => feature
            .icon
            .as_ref()
            .map(crate::assets::icons::icon)
            .unwrap_or_else(|| IconName::Settings2.into()),
    }
}

/// The choice after `selected` in declared order; the last one turns the feature off.
pub(super) fn next<'a>(feature: &'a Feature, selected: Option<&str>) -> Option<&'a str> {
    let index = match selected {
        None => 0,
        Some(selected) => feature
            .options
            .iter()
            .position(|option| option.id == selected)
            .map_or(feature.options.len(), |index| index + 1),
    };
    feature.options.get(index).map(|option| option.id.as_str())
}

/// Keeps the Fast slot visible for models that do not advertise it.
pub(super) fn unsupported() -> AnyElement {
    Button::new("model-feature-speed-unsupported")
        .rounded_full()
        .ghost()
        .small()
        .size_6()
        .icon(Icon::new(gpui_kit::assets::IconName::Zap))
        .accessibility_label(tr("composer_feature_fast"))
        .tooltip(tr("composer_feature_fast_unsupported"))
        .disabled(true)
        .into_any_element()
}

/// Edits the lead's configuration, or the member's model when `member` is set.
pub(super) fn control(
    feature: &Feature,
    selected: Option<&str>,
    disabled: bool,
    member: Option<sailry_protocol::RoleId>,
    owner: WeakEntity<View>,
) -> AnyElement {
    let current =
        selected.and_then(|selected| feature.options.iter().find(|option| option.id == selected));
    let cycles = speed(feature) || feature.options.len() == 1;
    let label = match (cycles, current) {
        (true, Some(option)) => label(feature, option),
        (true, None) if feature.options.len() == 1 => label(feature, &feature.options[0]),
        _ => feature.name.as_ref().unwrap_or(&feature.id).clone().into(),
    };
    let icon = if cycles {
        icon(feature, current)
    } else {
        icon(feature, None)
    };
    let select = move |id: &str, value: Option<&str>, window: &mut Window, cx: &mut App| {
        let _ = owner.update(cx, |view, cx| match member {
            Some(role) => view.select_member_feature(role, id, value, window, cx),
            None => view.select_feature(id, value, window, cx),
        });
    };
    let tooltip = if cycles && feature.options.len() > 1 && current.is_none() {
        format!("{} · {}", tr("composer_feature_off"), label)
    } else {
        format!("{label} · {}", tr("composer_feature_description"))
    };
    let button = Button::new(SharedString::from(format!("model-feature-{}", feature.id)))
        .rounded_full()
        .ghost()
        .small()
        .size_6()
        .icon(icon)
        .selected(selected.is_some())
        .accessibility_label(label.clone())
        .tooltip(tooltip)
        .disabled(disabled);
    if cycles {
        let id = feature.id.clone();
        let value = next(feature, selected).map(str::to_owned);
        return button
            .toggled(selected.is_some())
            .on_click(move |_, window, cx| select(&id, value.as_deref(), window, cx))
            .into_any_element();
    }
    let feature = feature.clone();
    let selected = selected.map(str::to_owned);
    let select = std::rc::Rc::new(select);
    button
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
            let off = select.clone();
            let id = feature.id.clone();
            let menu = menu.item(
                PopupMenuItem::new(tr("composer_feature_off"))
                    .checked(selected.is_none())
                    .on_click(move |_, window, cx| off(&id, None, window, cx)),
            );
            feature.options.iter().fold(menu, |menu, option| {
                let select = select.clone();
                let id = feature.id.clone();
                let value = option.id.clone();
                menu.item(
                    PopupMenuItem::new(self::label(&feature, option))
                        .checked(selected.as_deref() == Some(option.id.as_str()))
                        .on_click(move |_, window, cx| select(&id, Some(&value), window, cx)),
                )
            })
        })
        .into_any_element()
}
