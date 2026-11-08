use super::*;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        menu::{DropdownMenu, PopupMenu, PopupMenuItem},
        switch::Switch,
        *,
    },
    prelude::FluentBuilder as _,
};
use std::rc::Rc;
use ui::{EventKind, Kind};
mod indicator;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Form {
    Toolbar,
    Icons,
    Model,
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
        if self.declaration.kind == Kind::Menu
            && let Some(title) = &self.state.label
        {
            return label(title);
        }
        ui::Choice::selectable(self.choices())
            .find(|choice| self.state.value.as_str() == Some(&choice.id))
            .map(|choice| label(&choice.label))
            .unwrap_or_else(|| label(self.state.label.as_ref().unwrap_or(&self.declaration.label)))
    }

    pub(crate) fn segments(&self, cx: &App) -> Vec<AnyElement> {
        self.state
            .segments
            .iter()
            .map(|segment| {
                let color = match segment.tone {
                    ui::SegmentTone::Accent => cx.theme().primary,
                    ui::SegmentTone::Success => cx.theme().success,
                    ui::SegmentTone::Danger => cx.theme().danger,
                    ui::SegmentTone::Warning => cx.theme().warning,
                    ui::SegmentTone::Muted => cx.theme().muted_foreground,
                };
                div()
                    .flex_shrink_0()
                    .text_color(color)
                    .child(label(&segment.label))
                    .into_any_element()
            })
            .collect()
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
        if entry.retry {
            return h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new(SharedString::from(selector))
                        .small()
                        .disabled(true)
                        .label(entry.text())
                        .icon(icon(entry.declaration.icon.as_deref())),
                )
                .child(
                    crate::feedback::retry(SharedString::from(format!(
                        "plugin-retry-{}-{}",
                        key.package.name, key.id
                    )))
                    .small()
                    .disabled(if entry.declaration.slot == Slot::Status {
                        !entry.state.enabled
                    } else {
                        !self
                            .current(&key)
                            .is_some_and(|mounted| mounted.panel.read(cx).connected)
                    })
                    .on_click(
                        cx.listener(move |registry, _, window, cx| {
                            registry.retry(&key, window, cx)
                        }),
                    ),
                )
                .into_any_element();
        }
        if entry.declaration.kind == Kind::Indicator {
            return self.indicator(entry, cx);
        }
        let enabled = entry.state.enabled;
        let selected = entry.state.value.as_bool().unwrap_or(false);

        if entry.declaration.kind == Kind::Toggle && form == Form::Menu {
            let label_selector = format!("{selector}-label");
            return h_flex()
                .w_full()
                .min_w_0()
                .items_center()
                .h_8()
                .px_2p5()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .size_4()
                        .flex_shrink_0()
                        .when_some(entry.icon(), |slot, icon| slot.child(icon.size_4())),
                )
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
            .when(
                form != Form::Project && entry.declaration.slot != Slot::Status,
                |button| button.ghost(),
            )
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
            .loading(entry.state.loading)
            .when(
                form != Form::Menu && entry.declaration.slot == Slot::Context,
                |button| button.small().max_w_48().min_w_0().flex_shrink_1(),
            )
            .when(form == Form::Model, |button| button.small().h_6())
            .debug_selector(move || selector.clone())
            .accessibility_label(entry.text());
        if form == Form::Menu {
            button = button.small().h_8().w_full().min_w_0().child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(div().size_4().flex_shrink_0().child(control_icon.size_4()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_left()
                            .child(entry.text()),
                    )
                    .child(div().size_4().flex_shrink_0().when(
                        entry.state.dropdown
                            || matches!(
                                entry.declaration.kind,
                                Kind::Select | Kind::Menu | Kind::Picker | Kind::Popover
                            ),
                        |slot| slot.child(Icon::new(IconName::ChevronRight).size_4()),
                    )),
            );
        } else if entry.declaration.slot == Slot::Status {
            button = crate::theme::capsule(button, cx)
                .when_some(
                    status::tooltip(&entry.state.details, &rust_i18n::locale()),
                    |button, hint| button.tooltip(hint),
                )
                .icon(control_icon)
                .child(
                    h_flex()
                        .gap_2()
                        .children(entry.segments(cx))
                        .when(entry.state.segments.is_empty(), |line| {
                            line.child(entry.text())
                        }),
                );
        } else {
            // Header icons match the native square ghost header buttons beside them.
            let header = entry.declaration.slot == Slot::Header;
            button = button
                .when(form != Form::Project && !header, |button| {
                    button.rounded_full()
                })
                .when(header, |button| button.small())
                .icon(control_icon)
                .dropdown_caret(
                    (entry.state.dropdown || entry.declaration.kind == Kind::Menu)
                        && !matches!(form, Form::Icons | Form::Model),
                )
                .when(matches!(form, Form::Toolbar | Form::Project), |button| {
                    button.label(entry.text())
                })
                .when(matches!(form, Form::Icons | Form::Model), |button| {
                    button.tooltip(entry.text())
                });
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
                crate::theme::popover(
                    popover::Popover::new(SharedString::from(format!(
                        "popover-{}-{}",
                        key.package.name, key.id
                    ))),
                    cx,
                )
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
                let choices: Rc<[ui::Choice]> = entry.choices().into();
                let selected: Option<SharedString> = entry
                    .state
                    .value
                    .as_str()
                    .map(|value| value.to_owned().into());
                let owner = cx.weak_entity();
                let pick: Pick =
                    Rc::new(move |value: String, window: &mut Window, cx: &mut App| {
                        if form == Form::Project {
                            if let Some(registry) = owner.upgrade() {
                                native::Dispatch::new(
                                    registry,
                                    key.clone(),
                                    EventKind::Change,
                                    value.into(),
                                )
                                .emit(cx);
                            }
                            return;
                        }
                        let _ = owner.update(cx, |registry, cx| {
                            registry.invoke(&key, EventKind::Change, value.into(), window, cx)
                        });
                    });
                button
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, window, cx| {
                        rows(
                            bounded(menu),
                            &choices,
                            selected.clone(),
                            enabled,
                            pick.clone(),
                            window,
                            cx,
                        )
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

type Pick = Rc<dyn Fn(String, &mut Window, &mut App)>;

fn bounded(menu: PopupMenu) -> PopupMenu {
    menu.min_w(px(220.))
        .max_w(px(320.))
        .max_h(px(360.))
        .scrollable(true)
}

/// Contribution choices as Kit rows: groups are separated, descriptions sit
/// muted on the right and a choice with children opens one submenu level.
fn rows(
    mut menu: PopupMenu,
    choices: &Rc<[ui::Choice]>,
    selected: Option<SharedString>,
    enabled: bool,
    pick: Pick,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    for (index, choice) in choices.iter().enumerate() {
        if index > 0 && choices[index - 1].group != choice.group {
            menu = menu.separator();
        }
        if !choice.children.is_empty() {
            let children: Rc<[ui::Choice]> = choice.children.clone().into();
            let selected = selected.clone();
            let pick = pick.clone();
            menu = menu.submenu_with_icon(
                choice.icon.as_deref().map(|name| icon(Some(name))),
                label(&choice.label),
                window,
                cx,
                move |menu, window, cx| {
                    rows(
                        bounded(menu),
                        &children,
                        selected.clone(),
                        enabled,
                        pick.clone(),
                        window,
                        cx,
                    )
                },
            );
            continue;
        }
        let title = label(&choice.label);
        let item = match &choice.description {
            Some(description) => {
                let description = label(description);
                PopupMenuItem::element(move |_, cx| {
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_4()
                        .justify_between()
                        .child(div().min_w_0().truncate().child(title.clone()))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(description.clone()),
                        )
                })
            }
            None => PopupMenuItem::new(title),
        };
        let value = choice.id.clone();
        let pick = pick.clone();
        menu = menu.item(
            item.checked(selected.as_deref() == Some(choice.id.as_str()))
                .disabled(!enabled || !choice.enabled)
                .when_some(choice.icon.as_deref(), |item, name| {
                    item.icon(icon(Some(name)))
                })
                .on_click(move |_, window, cx| pick(value.clone(), window, cx)),
        );
    }
    menu
}
