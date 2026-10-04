//! Declared project actions use the same OS-native menu as built-in project actions.
use super::*;
use gpui_kit::component::native_menu::NativeMenu;
use ui::{EventKind, Kind};

pub(crate) fn init(cx: &mut App) {
    cx.on_action(|action: &Dispatch, cx| action.emit(cx));
}

#[derive(Clone, PartialEq, gpui_kit::Action)]
#[action(namespace = plugin_contribution, no_json)]
pub(crate) struct Dispatch {
    registry: Entity<Registry>,
    key: Key,
    kind: EventKind,
    value: serde_json::Value,
}

impl Dispatch {
    pub(super) fn new(
        registry: Entity<Registry>,
        key: Key,
        kind: EventKind,
        value: serde_json::Value,
    ) -> Self {
        Self {
            registry,
            key,
            kind,
            value,
        }
    }
    pub fn scope(&self) -> (NodeId, Option<WorktreeId>) {
        (self.key.node, self.key.worktree)
    }

    pub(super) fn emit(&self, cx: &mut App) {
        self.registry
            .update(cx, |_, cx| cx.emit(Event::Action(Box::new(self.clone()))));
    }

    pub fn dispatch(&self, window: &mut Window, cx: &mut App) {
        self.registry.update(cx, |registry, cx| {
            registry.invoke(&self.key, self.kind, self.value.clone(), window, cx)
        });
    }
}

pub(crate) fn append(
    mut menu: NativeMenu,
    registry: Entity<Registry>,
    slot: Slot,
    cx: &App,
) -> NativeMenu {
    for entry in registry.read(cx).entries(slot, cx) {
        match entry.declaration.kind {
            Kind::Button => {
                let action = Box::new(Dispatch {
                    registry: registry.clone(),
                    key: entry.key.clone(),
                    kind: EventKind::Invoke,
                    value: serde_json::Value::Null,
                });
                let label = controls::label(&entry.declaration.label);
                menu = if let Some(icon) = entry.icon() {
                    menu.menu_with_icon_disabled(label, icon, !entry.state.enabled, action)
                } else {
                    menu.menu_with_disabled(label, !entry.state.enabled, action)
                };
            }
            Kind::Menu => {
                let choices = entry.choices();
                let mut submenu = NativeMenu::new();
                for (index, choice) in choices.iter().enumerate() {
                    if index > 0 && choices[index - 1].group != choice.group {
                        submenu = submenu.separator();
                    }
                    let action = Box::new(Dispatch {
                        registry: registry.clone(),
                        key: entry.key.clone(),
                        kind: EventKind::Change,
                        value: choice.id.clone().into(),
                    });
                    let label = controls::label(&choice.label);
                    let disabled = !entry.state.enabled || !choice.enabled;
                    submenu = if let Some(name) = choice.icon.as_deref() {
                        submenu.menu_with_icon_disabled(
                            label,
                            controls::icon(Some(name)),
                            disabled,
                            action,
                        )
                    } else {
                        submenu.menu_with_disabled(label, disabled, action)
                    };
                }
                menu = menu.submenu(controls::label(&entry.declaration.label), submenu);
            }
            _ => {}
        }
    }
    menu
}
