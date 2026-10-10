mod consent;
mod details;
mod editor;
mod requests;
mod rows;
#[cfg(test)]
mod tests;
mod view;
mod watch;

use super::{Workspace, providers::Binding};
use gpui_kit::{component::*, *};
use sailry_link::{CancellationToken, Transport};
use sailry_protocol::{NodeId, Request, code};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default)]
pub(super) struct State {
    panels: BTreeMap<NodeId, Entity<Panel>>,
    selected: Option<NodeId>,
    scope: Option<Scope>,
}

#[derive(Clone)]
struct Scope {
    transport: Arc<dyn Transport>,
    runtime: Arc<tokio::runtime::Runtime>,
    label: SharedString,
}

pub(super) struct Panel {
    binding: Binding,
    active: bool,
    saved: Option<code::Settings>,
    draft: Option<code::Settings>,
    catalog: Option<Vec<code::Language>>,
    installations: Vec<code::Installation>,
    rules: Option<code::Rules>,
    tab: usize,
    loading: bool,
    pending: bool,
    cancelling: Option<String>,
    request: Option<Request>,
    error: Option<&'static str>,
    stop: Option<CancellationToken>,
    generation: u64,
    plugin_revision: Option<u64>,
    #[cfg(test)]
    editor: Option<WeakEntity<editor::Editor>>,
}

impl Drop for Panel {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.cancel();
        }
    }
}

impl Panel {
    fn new(binding: Binding, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::feedback::observe(window, cx, |panel: &Self, _| {
            panel.error.into_iter().collect()
        });
        Self {
            binding,
            active: false,
            saved: None,
            draft: None,
            catalog: None,
            installations: Vec::new(),
            rules: None,
            tab: 1,
            loading: false,
            pending: false,
            cancelling: None,
            request: None,
            error: None,
            stop: None,
            generation: 0,
            plugin_revision: None,
            #[cfg(test)]
            editor: None,
        }
    }

    fn dirty(&self) -> bool {
        self.saved != self.draft
    }

    fn locked(&self) -> bool {
        self.pending || self.request.is_some() || self.loading || self.saved.is_none()
    }
}

impl Workspace {
    pub(crate) fn bind_code_scope(
        &mut self,
        transport: Arc<dyn Transport>,
        runtime: Arc<tokio::runtime::Runtime>,
        label: SharedString,
        cx: &mut Context<Self>,
    ) {
        let changed = self
            .code
            .scope
            .as_ref()
            .is_none_or(|scope| scope.transport.target() != transport.target());
        if changed {
            self.clear_code(cx);
        }
        self.code.scope = Some(Scope {
            transport,
            runtime,
            label,
        });
    }

    pub(in crate::settings) fn forget_code_scope(&mut self, cx: &mut Context<Self>) {
        self.clear_code(cx);
        self.code.scope = None;
    }

    pub(in crate::settings) fn code(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(scope) = self.code.scope.clone() {
            self.bind_code(scope.transport, scope.runtime, scope.label, window, cx);
        }
        if let Some(key) = self.code.selected
            && let Some(panel) = self.code.panels.get(&key)
            && let Some(plugin) = self
                .plugin_catalog
                .packages
                .iter()
                .find(|plugin| plugin.name == "code")
        {
            panel.update(cx, |panel, cx| {
                if panel.plugin_revision.is_none() {
                    panel.plugin_revision = Some(plugin.revision);
                } else if panel.plugin_revision != Some(plugin.revision) && !panel.locked() {
                    panel.plugin_revision = Some(plugin.revision);
                    panel.load(window, cx);
                }
            });
        }
        if let Some(panel) = self
            .code
            .selected
            .and_then(|key| self.code.panels.get(&key))
        {
            let controls = panel.update(cx, |panel, cx| panel.controls(cx));
            v_flex()
                .gap_6()
                .child(controls)
                .child(panel.clone())
                .into_any_element()
        } else {
            div().into_any_element()
        }
    }

    pub(crate) fn bind_code(
        &mut self,
        transport: Arc<dyn Transport>,
        runtime: Arc<tokio::runtime::Runtime>,
        label: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bind_code_client(
            Binding {
                client: Arc::new(sailry_client::Client::new(transport)),
                runtime,
                label,
            },
            window,
            cx,
        );
    }

    pub(in crate::settings) fn bind_code_client(
        &mut self,
        binding: Binding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = binding.client.target();
        if self.code.selected == Some(key) {
            return;
        }
        self.clear_code(cx);
        let panel = self
            .code
            .panels
            .entry(key)
            .or_insert_with(|| {
                let panel = cx.new(|cx| Panel::new(binding, window, cx));
                cx.observe(&panel, |_, _, cx| cx.notify()).detach();
                panel
            })
            .clone();
        self.code.selected = Some(key);
        panel.update(cx, |panel, cx| {
            panel.active = true;
            panel.load(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn clear_code(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.code.selected.take()
            && let Some(panel) = self.code.panels.get(&key)
        {
            panel.update(cx, |panel, cx| {
                panel.active = false;
                panel.generation += 1;
                if let Some(stop) = panel.stop.take() {
                    stop.cancel();
                }
                cx.notify();
            });
        }
    }
}
