//! Provider rows share one declared contribution host per package on this page.
use super::*;
use crate::{
    conversation::live::Binding,
    plugins::contributions::{Registry, ViewState},
};

pub(in crate::settings) struct State {
    registry: Entity<Registry>,
    node: sailry_protocol::NodeId,
    _observer: Subscription,
}

impl State {
    pub(super) fn invalidate(&self, cx: &mut App) {
        self.registry
            .update(cx, |registry, cx| registry.invalidate_credentials(cx));
    }

    pub(in crate::settings) fn close(self, cx: &mut App) {
        self.registry.update(cx, |registry, cx| registry.close(cx));
    }
}

impl Workspace {
    pub(super) fn sync_provider_contributions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.provider_link else {
            return;
        };
        let binding = Binding {
            client: live.binding.client.clone(),
            defaults: live.binding.client.clone(),
            runtime: live.binding.runtime.clone(),
            project: None,
            worktree: None,
            host: live.binding.label.clone(),
            project_name: SharedString::default(),
            branch: SharedString::default(),
        };
        let node = binding.client.target();
        if self
            .provider_contributions
            .as_ref()
            .is_some_and(|state| state.node != node)
            && let Some(state) = self.provider_contributions.take()
        {
            state.close(cx);
        }
        if self.provider_contributions.is_none() {
            let registry = cx.new(|cx| Registry::providers(&binding, cx));
            let observer = cx.observe(&registry, |_, _, cx| cx.notify());
            self.provider_contributions = Some(State {
                registry,
                node,
                _observer: observer,
            });
        }
        let live = self.provider_link.as_ref().unwrap();
        self.provider_contributions
            .as_ref()
            .unwrap()
            .registry
            .update(cx, |registry, cx| {
                registry.sync(
                    &binding,
                    ViewState {
                        session: None,
                        assistant: None,
                        snapshot: live.snapshot.as_ref(),
                        connected: live.connected,
                        credentials: &live.credentials,
                    },
                    window,
                    cx,
                )
            });
    }

    pub(super) fn provider_metrics(
        &self,
        channel: &Channel,
        cx: &App,
    ) -> Vec<crate::plugins::contributions::Entry> {
        let Some(provider) = self
            .provider_link
            .as_ref()
            .and_then(|live| live.providers.get(&channel.id))
        else {
            return vec![];
        };
        self.provider_contributions
            .as_ref()
            .map(|state| state.registry.read(cx).provider_entries(provider, cx))
            .unwrap_or_default()
    }
}
