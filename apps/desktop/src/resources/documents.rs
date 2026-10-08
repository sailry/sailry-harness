//! File links and the launcher mount an ordinary declared document renderer.
use super::SideResource;
use crate::{
    plugins::{Panel, documents::routes::OpenTarget},
    shell::Shell,
};
use gpui_kit::*;
use sailry_protocol::{NodeId, SessionId, WorktreeId, plugin::desktop::ResourceKind};

impl Shell {
    pub(crate) fn open_documents(
        &mut self,
        scope: (NodeId, WorktreeId),
        target: Option<(String, Option<usize>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = self.current_chat().and_then(|source| {
            (source.read(cx).binding().worktree == Some(scope.1))
                .then(|| source.read(cx).session())
                .flatten()
        });
        self.show_documents(
            scope,
            target.map(|(path, line)| OpenTarget::new(path, line)),
            session,
            window,
            cx,
        );
    }

    pub(crate) fn open_historical_document(
        &mut self,
        scope: (NodeId, WorktreeId),
        path: String,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_documents(scope, Some(OpenTarget::new(path, line)), None, window, cx);
    }

    pub(crate) fn open_artifact_document(
        &mut self,
        scope: (NodeId, WorktreeId),
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_documents(
            scope,
            Some(OpenTarget {
                path,
                line: None,
                reveal_unsupported: true,
            }),
            None,
            window,
            cx,
        );
    }

    fn show_documents(
        &mut self,
        scope: (NodeId, WorktreeId),
        target: Option<OpenTarget>,
        session: Option<SessionId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.current_chat().cloned() else {
            return;
        };
        if source
            .read(cx)
            .renderer_unavailable(ResourceKind::Documents, cx)
        {
            return;
        }
        let mut binding = source.read(cx).binding();
        if binding.client.target() != scope.0 {
            return;
        }
        binding.worktree = Some(scope.1);
        let current = match &self.side_resource {
            Some(SideResource::Plugin(panel))
                if panel.read(cx).document_scope(cx) == Some(scope)
                    && panel
                        .read(cx)
                        .documents
                        .as_ref()
                        .is_some_and(|documents| documents.read(cx).session() == session) =>
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
                shell.show_documents(scope, retry.clone(), session, window, cx)
            }) {
                return;
            }
            let Some(documents) = self.documents.get(binding.clone(), session, window, cx) else {
                return;
            };
            let panel = cx.new(|cx| Panel::document_page(source, binding, session, documents, cx));
            Self::observe_plugin_conversations(&panel, window, cx);
            cx.observe_in(&panel, window, |shell, panel, window, cx| {
                if let Some((_, package)) = panel.read(cx).renderer_entry(ResourceKind::Documents, cx)
                    && panel.read(cx).ready_for(&package, cx)
                { panel.update(cx, |panel, cx| panel.open(package, window, cx)); }
                if panel.read(cx).resource_active() {
                    let target = panel.update(cx, |panel, _| panel.pending_document.take());
                    if let Some(target) = target { Self::reveal_document(&panel, target, window, cx); }
                }
                if matches!(&shell.side_resource, Some(SideResource::Plugin(current)) if current == &panel) { cx.notify(); }
            }).detach();
            panel
        };
        panel.update(cx, |panel, _| panel.pending_document = target);
        self.side_resource = Some(SideResource::Plugin(panel.clone()));
        self.layout.panel_open[0] = true;
        panel.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(crate) fn reveal_document(
        panel: &Entity<Panel>,
        target: OpenTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(receive) = panel.update(cx, |panel, cx| {
            if target.reveal_unsupported {
                panel.open_artifact_target(target.path.clone(), window, cx)
            } else {
                panel.open_document_target(target.path.clone(), target.line, window, cx)
            }
        }) else {
            return;
        };
        let panel = panel.downgrade();
        cx.spawn_in(window, async move |_, cx| {
            let mut result = receive.await;
            if matches!(&result, Ok(Err(fault)) if fault.code == sailry_protocol::ErrorCode::InvalidRequest)
                && let Ok(Some(receive)) = cx.update(|_, cx| {
                    panel.upgrade().and_then(|panel| panel.update(cx, |panel, cx| {
                        if target.reveal_unsupported { panel.reveal_system_file(target.path, cx) }
                        else { panel.open_system_file(target.path, cx) }
                    }))
                }) { result = receive.await; }
            if !matches!(result, Ok(Ok(_))) {
                let _ = cx.update(|window, cx| {
                    if panel
                        .upgrade()
                        .is_some_and(|panel| panel.read(cx).resource_active())
                    {
                        crate::resources::file_open::failure("files_open_failed", window, cx);
                    }
                });
            }
        })
        .detach();
    }
}

pub(crate) type Navigation = std::rc::Rc<dyn Fn(&mut Shell, &mut Window, &mut Context<Shell>)>;

impl Shell {
    pub(crate) fn guard_document_navigation(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        next: Navigation,
    ) -> bool {
        let Some(SideResource::Plugin(panel)) = &self.side_resource else {
            return false;
        };
        let Some(documents) = panel.read(cx).documents.clone() else {
            return false;
        };
        if !documents.read(cx).has_unsaved(cx) {
            return false;
        }
        let panel = panel.clone();
        let shell = cx.entity().downgrade();
        crate::prompts::confirm(
            &crate::tr("files_discard_title"),
            &crate::tr("files_discard_description"),
            crate::tr("files_discard"),
            window,
            cx,
            move |window, cx| {
                let _ = shell.update(cx, |shell, cx| {
                    if !matches!(&shell.side_resource, Some(SideResource::Plugin(current)) if current == &panel) { return; }
                    documents.update(cx, |documents, cx| documents.discard_all(cx));
                    next(shell, window, cx);
                    cx.notify();
                });
            },
        );
        true
    }
}
