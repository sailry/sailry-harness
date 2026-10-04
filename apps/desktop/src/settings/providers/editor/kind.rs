use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
mod picker;

fn groups() -> Vec<(&'static str, Vec<Preset>)> {
    use super::super::data::Family;
    let mut groups: Vec<(&'static str, Vec<Preset>)> = Vec::new();
    for family in Family::all() {
        let presets = Preset::all()
            .filter(|preset| {
                preset.offered() && preset.family() == family && preset.kind() == *preset
            })
            .collect::<Vec<_>>();
        if presets.is_empty() {
            continue;
        }
        let key = match family {
            Family::OpenCodeGo | Family::OpenCodeZen => "provider_opencode",
            _ => family.key(),
        };
        if let Some((_, items)) = groups.iter_mut().find(|(brand, _)| *brand == key) {
            items.extend(presets);
        } else {
            groups.push((key, presets));
        }
    }
    groups
}

impl Editor {
    pub(super) fn kind_control(&self, cx: &Context<Self>) -> impl IntoElement {
        let preset = self.preset;
        let owner = cx.entity();
        let disabled = self.editing.is_some() || self.pending;
        let button = Button::new("provider-type")
            .debug_selector(|| "provider-type".into())
            .w_full()
            .min_w_0()
            .label(tr(preset.key()))
            .dropdown_caret(true)
            .disabled(disabled);
        let control = if disabled {
            button.into_any_element()
        } else {
            picker::popover(owner.downgrade(), button).into_any_element()
        };

        h_flex()
            .debug_selector(|| "provider-kind-row".into())
            .w_full()
            .gap_2()
            .child(div().flex_1().min_w_0().child(control))
            .when(!preset.protocols().is_empty(), |row| {
                let owner = cx.entity();
                row.child(
                    Button::new("provider-api")
                        .debug_selector(|| "provider-api".into())
                        .w(rems(9.))
                        .flex_shrink_0()
                        .label(tr(preset.protocol_key()))
                        .accessibility_label(tr("provider_api"))
                        .dropdown_caret(true)
                        .disabled(self.editing.is_some() || self.pending)
                        .dropdown_menu(move |menu, _, _| {
                            preset.protocols().iter().fold(menu, |menu, &p| {
                                let owner = owner.clone();
                                menu.item(
                                    PopupMenuItem::new(tr(p.protocol_key()))
                                        .checked(p == preset)
                                        .on_click(move |_, window, cx| {
                                            owner.update(cx, |editor, cx| {
                                                editor.select_preset(p, window, cx)
                                            })
                                        }),
                                )
                            })
                        }),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn native_and_compatible_channels() {
        let brands = groups();
        assert_eq!(
            brands
                .iter()
                .find(|(key, _)| *key == "provider_opencode")
                .unwrap()
                .1,
            [Preset::OpenCodeGo, Preset::OpenCodeZen]
        );
        assert!(!brands.iter().any(|(key, _)| *key == "provider_group_other"));
        for vendor in super::super::super::vendors::Vendor::FIRST_PARTY {
            assert!(brands.iter().any(|(key, presets)| {
                *key == vendor.key() && presets == &[Preset::Hosted(vendor)]
            }));
        }
        let offered: Vec<_> = groups()
            .into_iter()
            .flat_map(|(_, presets)| presets)
            .collect();
        for kind in Preset::all().filter(|preset| preset.kind() == *preset) {
            assert_eq!(
                offered.iter().filter(|&&preset| preset == kind).count(),
                usize::from(kind.offered()),
                "{kind:?}"
            );
        }
        assert!(!offered.iter().any(|preset| preset.cloud()));
        assert!(offered.contains(&Preset::CompatibleResponses));
        assert!(offered.contains(&Preset::ChatGpt));
        assert!(offered.contains(&Preset::OpenCodeGo));
        assert!(offered.contains(&Preset::OpenCodeZen));
        for vendor in super::super::super::vendors::Vendor::FIRST_PARTY {
            assert!(offered.contains(&Preset::Hosted(vendor)));
        }
    }
}
