//! Delegation preference changes the same revision-checked session configuration.
use super::*;

impl View {
    pub(super) fn delegation_available(&self, cx: &App) -> bool {
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

    pub(super) fn toggle_delegation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
