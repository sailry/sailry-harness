//! Declared model choices share the revision-checked composer configuration.
use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use sailry_protocol::conversation::features::Feature;
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

pub(super) fn labels(
    features: &[Feature],
    selected: &BTreeMap<String, String>,
) -> Vec<SharedString> {
    features
        .iter()
        .filter_map(|feature| {
            let selected = selected.get(&feature.id)?;
            let option = feature
                .options
                .iter()
                .find(|option| option.id == *selected)?;
            Some(option.name.as_ref().unwrap_or(&option.id).clone().into())
        })
        .collect()
}

pub(super) fn control(
    feature: &Feature,
    selected: Option<&str>,
    disabled: bool,
    owner: WeakEntity<View>,
) -> AnyElement {
    let label = if feature.options.len() == 1 {
        feature.options[0]
            .name
            .as_ref()
            .unwrap_or(&feature.options[0].id)
    } else {
        feature.name.as_ref().unwrap_or(&feature.id)
    };
    let icon = feature
        .icon
        .as_ref()
        .map(crate::assets::icons::icon)
        .unwrap_or_else(|| IconName::Settings2.into());
    let button = Button::new(SharedString::from(format!("model-feature-{}", feature.id)))
        .rounded_full()
        .text()
        .small()
        .size_6()
        .icon(icon)
        .selected(selected.is_some())
        .accessibility_label(label.clone())
        .tooltip(format!(
            "{} · {}",
            label,
            tr("composer_feature_description")
        ))
        .disabled(disabled);
    if let [option] = feature.options.as_slice() {
        let id = feature.id.clone();
        let value = (selected != Some(option.id.as_str())).then(|| option.id.clone());
        return button
            .toggled(selected.is_some())
            .on_click(move |_, window, cx| {
                let _ = owner.update(cx, |view, cx| {
                    view.select_feature(&id, value.as_deref(), window, cx)
                });
            })
            .into_any_element();
    }
    let feature = feature.clone();
    let selected = selected.map(str::to_owned);
    button
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
            let off_owner = owner.clone();
            let id = feature.id.clone();
            let menu = menu.item(
                PopupMenuItem::new(tr("composer_feature_off"))
                    .checked(selected.is_none())
                    .on_click(move |_, window, cx| {
                        let _ = off_owner
                            .update(cx, |view, cx| view.select_feature(&id, None, window, cx));
                    }),
            );
            feature.options.iter().fold(menu, |menu, option| {
                let owner = owner.clone();
                let id = feature.id.clone();
                let value = option.id.clone();
                menu.item(
                    PopupMenuItem::new(option.name.as_ref().unwrap_or(&option.id).clone())
                        .checked(selected.as_deref() == Some(option.id.as_str()))
                        .on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |view, cx| {
                                view.select_feature(&id, Some(&value), window, cx)
                            });
                        }),
                )
            })
        })
        .into_any_element()
}
