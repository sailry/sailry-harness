//! Delegation preference changes the same revision-checked session configuration.
use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::FluentBuilder as _;

impl View {
    fn delegation_available(&self, cx: &App) -> bool {
        if !self.connected() || !self.configured() {
            return false;
        }
        let Some(config) = &self.config else {
            return false;
        };
        let tools = self
            .providers()
            .find(|provider| provider.id == config.provider)
            .and_then(|provider| {
                provider
                    .models
                    .iter()
                    .find(|model| model.id == config.model)
            })
            .is_some_and(|model| model.tools);
        tools
            && self.contributions.read(cx).operation_available(
                sailry_protocol::tool::Operation::DelegateAgent,
                self.binding.project,
                self.resource,
                self.session
                    .as_ref()
                    .and_then(|session| session.config.assistant.as_ref())
                    .or(self.assistant.as_ref()),
                cx,
            )
    }

    pub(super) fn delegation_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self
            .config
            .as_ref()
            .is_some_and(|config| config.delegation_proactive);
        let available = self.delegation_available(cx);
        let disabled = !available
            || self.readonly()
            || self.configuration_blocked()
            || (self.session.is_some() && !self.connected());
        Button::new("composer-delegation")
            .ghost()
            .icon(IconName::Network)
            .rounded_full()
            .flex_shrink_0()
            .selected(selected)
            .toggled(selected)
            .accessibility_label(tr("composer_delegation_proactive"))
            .tooltip(tr(if !available {
                "composer_delegation_unavailable"
            } else if selected {
                "composer_delegation_active"
            } else {
                "composer_delegation_normal"
            }))
            .disabled(disabled)
            .debug_selector(|| "composer-delegation".into())
            .on_click(cx.listener(|view, _, window, cx| view.toggle_delegation(window, cx)))
            .into_any_element()
    }

    fn toggle_delegation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly()
            || self.configuration_blocked()
            || !self.delegation_available(cx)
            || (self.session.is_some() && !self.connected())
        {
            return;
        }
        if let Some(mut config) = self.config.clone() {
            config.delegation_proactive = !config.delegation_proactive;
            self.configure(config, window, cx);
        }
    }
}
