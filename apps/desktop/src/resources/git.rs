//! Core review entry points resolve the enabled resource declaration.
use super::SideResource;
use crate::{plugins::Panel, shell::Shell};
use gpui_kit::*;
use sailry_protocol::{NodeId, WorktreeId, plugin::desktop::ResourceKind};

impl Shell {
    pub(crate) fn open_git_resource(
        &mut self,
        scope: (NodeId, WorktreeId),
        target: Option<Option<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.current_chat().cloned() else {
            return;
        };
        if source.read(cx).renderer_unavailable(ResourceKind::Git, cx) {
            return;
        }
        let mut binding = source.read(cx).binding();
        if binding.client.target() != scope.0 {
            return;
        }
        binding.worktree = Some(scope.1);
        let current = match &self.side_resource {
            Some(SideResource::Plugin(panel))
                if panel.read(cx).resource.as_ref().is_some_and(|resource| {
                    resource.read(cx).kind == ResourceKind::Git && resource.read(cx).scope == scope
                }) =>
            {
                Some(panel.clone())
            }
            _ => None,
        };
        let panel = if let Some(panel) = current {
            panel
        } else {
            let retry = target.clone();
            if self.guard_file_navigation(window, cx, move |shell, window, cx| {
                shell.open_git_resource(scope, retry.clone(), window, cx)
            }) {
                return;
            }
            let panel = cx.new(|cx| Panel::resource_page(source, binding, ResourceKind::Git, cx));
            Self::observe_plugin_conversations(&panel, window, cx);
            cx.observe_in(&panel,window,|shell,panel,window,cx|{
                if let Some((_,package))=panel.read(cx).renderer_entry(ResourceKind::Git,cx)
                    && panel.read(cx).ready_for(&package,cx) {panel.update(cx,|panel,cx|panel.open(package,window,cx));}
                if matches!(&shell.side_resource,Some(SideResource::Plugin(current)) if current==&panel){cx.notify();}
            }).detach();
            panel
        };
        if let Some(path) = target {
            panel
                .read(cx)
                .resource
                .clone()
                .unwrap()
                .update(cx, |state, cx| {
                    state.reveal(serde_json::json!({"path":path}), cx)
                });
        }
        self.side_resource = Some(SideResource::Plugin(panel.clone()));
        self.layout.panel_open[0] = true;
        panel.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
}
