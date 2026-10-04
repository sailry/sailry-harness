use super::*;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        menu::{DropdownMenu, PopupMenuItem},
        switch::Switch,
        *,
    },
    prelude::FluentBuilder as _,
};
use ui::{EventKind, Kind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Form {
    Toolbar,
    Icons,
    Menu,
    Project,
}

pub(super) fn icon(name: Option<&str>) -> Icon {
    if let Some(name) = name {
        if name.starts_with("reicon:") {
            return crate::assets::icons::icon(&sailry_protocol::plugin::desktop::Icon::Name(
                name.into(),
            ));
        }
        let path = format!("icons/{name}.svg");
        if crate::assets::Assets.load(&path).ok().flatten().is_some() {
            return Icon::default().path(path);
        }
    }
    IconName::Settings2.into()
}

pub(super) fn label(label: &sailry_protocol::plugin::desktop::Navigation) -> SharedString {
    label.label(&rust_i18n::locale()).to_owned().into()
}

impl Entry {
    pub(crate) fn icon(&self) -> Option<Icon> {
        self.state
            .icon
            .as_ref()
            .or(self.declaration.icon.as_ref())
            .map(|name| icon(Some(name)))
    }
    pub(crate) fn choices(&self) -> &[ui::Choice] {
        self.state
            .choices
            .as_deref()
            .unwrap_or(&self.declaration.choices)
    }

    pub(crate) fn text(&self) -> SharedString {
        self.choices()
            .iter()
            .find(|choice| self.state.value.as_str() == Some(&choice.id))
            .map(|choice| label(&choice.label))
            .unwrap_or_else(|| label(self.state.label.as_ref().unwrap_or(&self.declaration.label)))
    }

    pub(crate) fn value(&self) -> SharedString {
        match &self.state.value {
            serde_json::Value::Null => crate::tr("composer_metric_unknown"),
            serde_json::Value::String(value) => value.clone().into(),
            value => value.to_string().into(),
        }
    }
}

impl Registry {
    pub(crate) fn control(&self, entry: Entry, form: Form, cx: &mut Context<Self>) -> AnyElement {
        let key = entry.key.clone();
        let selector = format!("plugin-control-{}-{}", key.package.name, key.id);
        let enabled = entry.state.enabled;
        let selected = entry.state.value.as_bool().unwrap_or(false);
        if entry.declaration.kind == Kind::Indicator {
            let indicator = serde_json::from_value::<ui::Indicator>(entry.state.value.clone()).ok();
            let hint: SharedString = indicator
                .as_ref()
                .map(|value| value.hint.clone())
                .unwrap_or_else(|| label(&entry.declaration.label).to_string())
                .into();
            let color = match indicator.as_ref().map(|value| value.tone) {
                Some(ui::Tone::Warning) => cx.theme().warning,
                Some(ui::Tone::Danger) => cx.theme().danger,
                _ => cx.theme().muted_foreground,
            };
            return div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .flex_shrink_0()
                .px_1()
                .tooltip({
                    let hint = hint.clone();
                    move |window, cx| tooltip::Tooltip::new(hint.clone()).build(window, cx)
                })
                .child(
                    progress::ProgressCircle::new(SharedString::from(format!(
                        "indicator-{}",
                        entry.key.id
                    )))
                    .value(
                        indicator
                            .as_ref()
                            .and_then(|value| value.percent)
                            .unwrap_or(0.)
                            .min(100.) as f32,
                    )
                    .loading(indicator.as_ref().is_some_and(|value| value.loading))
                    .with_size(px(20.))
                    .color(color)
                    .accessibility_label(hint),
                )
                .into_any_element();
        }

        if entry.declaration.kind == Kind::Toggle && form == Form::Menu {
            let label_selector = format!("{selector}-label");
            return h_flex()
                .w_full()
                .h_8()
                .px_2p5()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .debug_selector(move || label_selector.clone())
                        .child(label(&entry.declaration.label)),
                )
                .child(
                    div().debug_selector(move || selector.clone()).child(
                        Switch::new(SharedString::from(format!(
                            "toggle-{}-{}",
                            key.package.name, key.id
                        )))
                        .small()
                        .checked(selected)
                        .disabled(!enabled)
                        .accessibility_label(label(&entry.declaration.label))
                        .on_click(cx.listener(
                            move |registry, value: &bool, window, cx| {
                                registry.invoke(
                                    &key,
                                    EventKind::Change,
                                    (*value).into(),
                                    window,
                                    cx,
                                )
                            },
                        )),
                    ),
                )
                .into_any_element();
        }
        let control_icon = icon(
            entry
                .state
                .icon
                .as_deref()
                .or(entry.declaration.icon.as_deref()),
        );
        let mut button = Button::new(SharedString::from(selector.clone()))
            .when(form != Form::Project, |button| button.ghost())
            .when(
                entry.declaration.slot == Slot::Composer && entry.declaration.kind == Kind::Toggle,
                |button| {
                    button.custom(crate::theme::subtle_button(cx).foreground(if selected {
                        cx.theme().foreground
                    } else {
                        cx.theme().muted_foreground
                    }))
                },
            )
            .disabled(!enabled)
            .when(entry.declaration.slot == Slot::Context, |button| {
                button.small().max_w_48().min_w_0().flex_shrink_1()
            })
            .debug_selector(move || selector.clone())
            .accessibility_label(entry.text());
        if form == Form::Menu {
            button = button.w_full().child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .child(div().flex_1().min_w_0().truncate().child(entry.text()))
                    .child(
                        if matches!(
                            entry.declaration.kind,
                            Kind::Select | Kind::Menu | Kind::Picker
                        ) {
                            IconName::ChevronRight.into()
                        } else {
                            control_icon
                        },
                    ),
            );
        } else if entry.declaration.slot == Slot::Status {
            button = button
                .when_some(
                    entry.state.value.as_str().filter(|text| !text.is_empty()),
                    |button, hint| button.tooltip(hint.to_owned()),
                )
                .outline()
                .small()
                .rounded_full()
                .px_3()
                .font_normal()
                .icon(control_icon)
                .child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .font_normal()
                        .children(entry.state.segments.iter().map(|segment| {
                            let color = match segment.tone {
                                ui::SegmentTone::Success => cx.theme().success,
                                ui::SegmentTone::Danger => cx.theme().danger,
                                ui::SegmentTone::Warning => cx.theme().warning,
                                ui::SegmentTone::Muted => cx.theme().muted_foreground,
                            };
                            div().text_color(color).child(label(&segment.label))
                        }))
                        .when(entry.state.segments.is_empty(), |line| {
                            line.child(entry.text())
                        }),
                );
        } else {
            button = button
                .when(form != Form::Project, |button| button.rounded_full())
                .icon(control_icon)
                .dropdown_caret(entry.state.dropdown && form != Form::Icons)
                .when(matches!(form, Form::Toolbar | Form::Project), |button| {
                    button.label(entry.text())
                })
                .when(form == Form::Icons, |button| button.tooltip(entry.text()));
            if entry.declaration.kind == Kind::Toggle {
                button = button.selected(selected).toggled(selected);
            }
        }
        match entry.declaration.kind {
            Kind::Popover => {
                let Some(mounted) = self
                    .mounted
                    .get(&key.package.name)
                    .filter(|mounted| mounted.package == key.package)
                else {
                    return div().into_any_element();
                };
                popover::Popover::new(SharedString::from(format!(
                    "popover-{}-{}",
                    key.package.name, key.id
                )))
                .open(selected)
                .anchor(Anchor::BottomLeft)
                .when(entry.declaration.slot == Slot::Status, |popover| {
                    popover.bottom_2()
                })
                .trigger(button)
                .child(mounted.panel.clone())
                .on_open_change(cx.listener(move |registry, open: &bool, window, cx| {
                    registry.invoke(&key, EventKind::Change, (*open).into(), window, cx)
                }))
                .into_any_element()
            }
            Kind::Select | Kind::Menu => {
                let choices = entry.choices().to_vec();
                let selected = entry.state.value.clone();
                let owner = cx.weak_entity();
                button
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                        choices
                            .iter()
                            .enumerate()
                            .fold(menu, |mut menu, (index, choice)| {
                                if index > 0 && choices[index - 1].group != choice.group {
                                    menu = menu.separator();
                                }
                                let owner = owner.clone();
                                let key = key.clone();
                                let value = choice.id.clone();
                                menu.item(
                                    PopupMenuItem::new(label(&choice.label))
                                        .checked(selected.as_str() == Some(&choice.id))
                                        .disabled(!enabled || !choice.enabled)
                                        .when_some(choice.icon.as_deref(), |item, name| {
                                            item.icon(icon(Some(name)))
                                        })
                                        .on_click(move |_, window, cx| {
                                            if form == Form::Project {
                                                if let Some(registry) = owner.upgrade() {
                                                    native::Dispatch::new(
                                                        registry,
                                                        key.clone(),
                                                        EventKind::Change,
                                                        value.clone().into(),
                                                    )
                                                    .emit(cx);
                                                }
                                                return;
                                            }
                                            let _ = owner.update(cx, |registry, cx| {
                                                registry.invoke(
                                                    &key,
                                                    EventKind::Change,
                                                    value.clone().into(),
                                                    window,
                                                    cx,
                                                )
                                            });
                                        }),
                                )
                            })
                    })
                    .into_any_element()
            }
            Kind::Picker => {
                let owner = cx.entity();
                button
                    .on_click(move |_, window, cx| {
                        picker::open(owner.clone(), key.clone(), window, cx);
                    })
                    .into_any_element()
            }
            Kind::Toggle => button
                .on_click(cx.listener(move |registry, _, window, cx| {
                    registry.invoke(&key, EventKind::Change, (!selected).into(), window, cx)
                }))
                .into_any_element(),
            Kind::Button if form == Form::Project => {
                let registry = cx.entity();
                button
                    .on_click(move |_, _, cx| {
                        native::Dispatch::new(
                            registry.clone(),
                            key.clone(),
                            EventKind::Invoke,
                            serde_json::Value::Null,
                        )
                        .emit(cx);
                    })
                    .into_any_element()
            }
            Kind::Button => button
                .on_click(cx.listener(move |registry, _, window, cx| {
                    registry.invoke(&key, EventKind::Invoke, serde_json::Value::Null, window, cx)
                }))
                .into_any_element(),
            Kind::Metric | Kind::Indicator => div().into_any_element(),
        }
    }
}
