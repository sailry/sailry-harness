use super::*;
use crate::conversation::mode;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use sailry_protocol::plugin;

use sailry_client::conversation::modes as selection;
pub(super) use selection::Mode as PluginMode;

pub(super) fn label(mode: &PluginMode) -> SharedString {
    mode.declaration
        .display
        .label(&rust_i18n::locale())
        .to_owned()
        .into()
}

pub(super) fn icon(entry: &PluginMode) -> gpui_kit::component::Icon {
    entry
        .declaration
        .display
        .icon
        .as_ref()
        .map(crate::assets::icons::icon)
        .unwrap_or_else(|| mode::icon(entry.declaration.base()).into())
}

impl View {
    pub(super) fn select_mode(
        &mut self,
        mode: sailry_protocol::WorkMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly()
            || self.configuration_blocked()
            || (self.session.is_some() && !self.connected())
        {
            return;
        }
        self.replace_mode_marker(mode, window, cx);
        if self.session.is_none() {
            self.draft_mode = Some(mode);
        }
        if let Some(config) = self.config.as_ref() {
            let config = selection::select(config, &self.declared_modes(cx), selection::key(mode))
                .expect("built-in modes are always available");
            self.configure(config, window, cx);
        } else {
            self.remember_options(cx);
        }
        cx.notify();
    }

    pub(super) fn plugin_modes(&self, cx: &App) -> Vec<PluginMode> {
        let modes = self.declared_modes(cx);
        selection::choices(None, &modes).cloned().collect()
    }

    fn declared_modes(&self, cx: &App) -> Vec<PluginMode> {
        self.contribution_metadata
            .read(cx)
            .entries
            .values()
            .flat_map(|info| {
                info.extension
                    .iter()
                    .filter(|extension| extension.scope == plugin::Scope::Host)
                    .flat_map(|extension| &extension.conversation_modes)
                    .map(move |declaration| PluginMode {
                        package: info.summary.reference(),
                        declaration: declaration.clone(),
                        enabled: info.summary.enabled,
                    })
            })
            .collect()
    }

    pub(super) fn selected_plugin_mode(&self, cx: &App) -> Option<PluginMode> {
        let config = self.config.as_ref()?;
        let modes = self.declared_modes(cx);
        let selected = selection::project(config, &modes).selected?;
        modes.into_iter().find(|mode| mode.key() == selected)
    }

    pub(super) fn select_plugin_mode(
        &mut self,
        mode: &PluginMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly()
            || self.configuration_blocked()
            || (self.session.is_some() && !self.connected())
            || !self.plugin_modes(cx).iter().any(|current| {
                current.package == mode.package && current.declaration == mode.declaration
            })
        {
            return;
        }
        let Some(config) = self.config.as_ref() else {
            return;
        };
        let Some(config) = selection::select(config, &self.declared_modes(cx), &mode.key()) else {
            return;
        };
        self.replace_mode_marker(config.mode, window, cx);
        if self.session.is_none() {
            self.draft_mode = Some(config.mode);
        }
        self.configure(config, window, cx);
        cx.notify();
    }

    pub(super) fn confirm_mode_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        let text = self.input.read(cx).value();
        let Some((mode, range)) = references::commands::leading_mode(&text) else {
            return;
        };
        if range.end == text.len() || self.current_mode() == mode {
            return;
        }
        self.select_mode(mode, window, cx);
    }

    pub(super) fn current_mode(&self) -> sailry_protocol::WorkMode {
        self.config
            .as_ref()
            .map(|config| config.mode)
            .or(self.draft_mode)
            .unwrap_or(sailry_protocol::WorkMode::Code)
    }

    pub(super) fn replace_mode_marker(
        &mut self,
        mode: sailry_protocol::WorkMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.input.read(cx).value();
        let Some((current, range)) = references::commands::leading_mode(&text) else {
            return;
        };
        if current == mode {
            return;
        }
        self.input.update(cx, |input, cx| {
            let selected = input.selected_range();
            let length = references::commands::mode_token(mode).len();
            let end = range.end;
            let start = range.start;
            let shift = |offset: usize| {
                if offset >= end {
                    offset - (end - start) + length
                } else if offset > start {
                    start + length
                } else {
                    offset
                }
            };
            input
                .replace_range_with_token(
                    range,
                    references::commands::token(references::commands::mode_token(mode)),
                    window,
                    cx,
                )
                .expect("leading mode is a valid input range");
            input.set_selected_range(shift(selected.start)..shift(selected.end), cx);
        });
    }

    pub(super) fn mode_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self
            .config
            .as_ref()
            .map(|config| config.mode)
            .or(self.draft_mode);
        let current = selected.unwrap_or(sailry_protocol::WorkMode::Code);
        let active = self.selected_plugin_mode(cx);
        let declared = self.declared_modes(cx);
        let plugins: Vec<_> = selection::choices(self.config.as_ref(), &declared)
            .cloned()
            .collect();
        let configured = self.config.is_some();
        let owner = cx.entity().downgrade();
        let button = Button::new("live-chat-mode")
            .custom(crate::theme::subtle_button(cx))
            .icon(
                active
                    .as_ref()
                    .map(icon)
                    .unwrap_or_else(|| mode::icon(current).into()),
            )
            .rounded_full()
            .label(
                active
                    .as_ref()
                    .map(label)
                    .unwrap_or_else(|| tr(mode::label(current))),
            )
            .dropdown_caret(false)
            .debug_selector(|| "live-chat-mode".into())
            .disabled(
                self.configuration_blocked() || (self.session.is_some() && !self.connected()),
            );
        button
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                let menu =
                    mode::MODES
                        .into_iter()
                        .fold(menu.check_side(Side::Right), |menu, mode| {
                            let owner = owner.clone();
                            menu.item(
                                mode::item(mode, active.is_none() && current == mode).on_click(
                                    move |_, window, cx| {
                                        let _ = owner.update(cx, |view, cx| {
                                            view.select_mode(mode, window, cx);
                                        });
                                    },
                                ),
                            )
                        });
                plugins.iter().fold(menu, |menu, entry| {
                    let owner = owner.clone();
                    let selected = active.as_ref().is_some_and(|active| {
                        active.package == entry.package
                            && active.declaration.id == entry.declaration.id
                    });
                    let entry = entry.clone();
                    menu.item(
                        PopupMenuItem::new(label(&entry))
                            .icon(icon(&entry))
                            .checked(selected)
                            .disabled(!configured || !entry.enabled)
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |view, cx| {
                                    view.select_plugin_mode(&entry, window, cx)
                                });
                            }),
                    )
                })
            })
            .into_any_element()
    }
}
