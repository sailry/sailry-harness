//! Core links select a declared editor and retain the originating resource scope.
use super::*;
use crate::{conversation::live::View as Chat, plugins::Panel};

impl Panel {
    pub(crate) fn document_page(
        source: Entity<Chat>,
        binding: Binding,
        session: Option<SessionId>,
        documents: Entity<Controller>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::standalone(binding, cx).with_documents(Some(documents));
        let workspace = cx.new(crate::plugins::workspace::State::embedded);
        panel.attach_workspace(workspace, cx);
        panel.source = Some(source);
        panel.session = session;
        panel
    }

    pub(crate) fn is_document_page(&self, cx: &App) -> bool {
        self.documents.is_some()
            && self
                .workspace
                .as_ref()
                .is_some_and(|workspace| workspace.read(cx).embedded)
    }

    pub(crate) fn document_scope(&self, cx: &App) -> Option<(NodeId, WorktreeId)> {
        self.documents
            .as_ref()
            .map(|documents| documents.read(cx).scope())
    }

    pub(crate) fn open_document_target(
        &self,
        path: String,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<Value, Fault>>> {
        if !self.resource_active() {
            return None;
        }
        if crate::content::images::supports(&path) {
            let host = self.mounted.as_ref()?.host();
            crate::content::images::open_scoped(
                self.binding.client.clone(),
                self.binding.runtime.clone(),
                host.context().clone(),
                path,
                window,
                cx,
            );
            None
        } else if crate::content::files::external(&path) {
            self.open_system_file(path, cx)
        } else {
            self.reveal_document(path, line, cx)
        }
    }

    pub(crate) fn open_system_file(
        &self,
        path: String,
        cx: &mut App,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<Value, Fault>>> {
        if !self.resource_active() {
            return None;
        }
        Some(crate::plugins::file_transfers::open_captured(
            &self.binding,
            &self.mounted.as_ref()?.host(),
            path,
            cx,
        ))
    }

    pub(crate) fn reveal_document(
        &self,
        path: String,
        line: Option<usize>,
        cx: &mut App,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<Value, Fault>>> {
        if !self.resource_active() {
            return None;
        }
        let documents = self.documents.as_ref()?;
        let host = self.mounted.as_ref()?.host();
        Some(documents.update(cx, |controller, cx| {
            controller.request_open(path, line, host.context().clone(), host.stop_token(), cx)
        }))
    }
}

pub(super) struct ClosePanel(sailry_protocol::plugin::Reference);
impl EventEmitter<ClosePanel> for Panel {}

pub(in crate::plugins) fn module(
    module: gpui_shell::HostModule,
    owner: WeakEntity<Panel>,
    host: std::sync::Arc<crate::plugins::host::Host>,
) -> gpui_shell::HostModule {
    let declarations = format!(
        "{}\nexport function closePanel(): void;",
        module.declared().unwrap_or_default()
    );
    module
        .function("closePanel", move |_| {
            host.check()?;
            let owner = owner.clone();
            let package = host.context().package.clone();
            gpui_shell::with_current_app(|cx| {
                cx.defer(move |cx| {
                    let _ = owner.update(cx, |panel, cx| {
                        if panel.resource_active() && panel.selected.as_ref() == Some(&package) {
                            cx.emit(ClosePanel(package));
                        }
                    });
                })
            })
            .ok_or_else(|| gpui_shell::HostError::new("panel closure requires an active view"))?;
            Ok(gpui_shell::HostValue::Null)
        })
        .declarations(declarations)
}

impl crate::shell::Shell {
    pub(in crate::plugins) fn observe_plugin_close(
        panel: &Entity<Panel>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(panel, window, |shell, panel, event: &ClosePanel, window, cx| {
            if panel.read(cx).resource_active()
                && panel.read(cx).selected.as_ref() == Some(&event.0)
                && matches!(&shell.side_resource, Some(crate::resources::SideResource::Plugin(current)) if current == panel)
            {
                shell.close_file_panel(window, cx);
            }
        }).detach();
    }
}
