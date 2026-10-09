//! Captured file previews share the document owner's retained Notebook cells.
use super::Host;
use crate::conversation::live::Binding;
use crate::plugins::documents::notebook::Notebook;
use gpui_kit::{
    component::{
        ActiveTheme, Disableable,
        button::{Button, ButtonVariants},
        v_flex,
    },
    *,
};
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, Output, RequestId, notebook, tool::File};
use std::sync::Arc;

pub(super) struct View {
    binding: Binding,
    file: File,
    host: Arc<Host>,
    notebook: Option<Notebook>,
    id: RequestId,
    started: bool,
    loading: bool,
    failed: bool,
    stop: CancellationToken,
}

impl Drop for View {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl View {
    pub(super) fn new(binding: Binding, file: File, host: Arc<Host>) -> Self {
        let stop = host.stop_token().child_token();
        Self {
            binding,
            file,
            host,
            notebook: None,
            id: RequestId::new(),
            started: false,
            loading: false,
            failed: false,
            stop,
        }
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading || self.host.check().is_err() {
            return;
        }
        let offset = self
            .notebook
            .as_ref()
            .and_then(|value| value.next)
            .unwrap_or(0);
        let revision = self
            .notebook
            .as_ref()
            .map(|value| value.revision.clone())
            .or_else(|| self.file.revision.clone());
        self.started = true;
        self.loading = true;
        self.failed = false;
        let client = self.binding.client.clone();
        let context = self.host.context();
        let worktree = self.binding.worktree;
        let path = self.file.path.clone();
        let cancel = self.stop.clone();
        let job = self.binding.runtime.spawn(async move {
            let worktree = worktree.ok_or_else(|| Fault::invalid("notebook requires a worktree"))?;
            let request = client.prepare(Command::ReadNotebook {
                worktree,
                options: notebook::Read { path: path.clone(), offset, limit: 20 },
            }).with_plugin(context);
            let output = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(Fault::new(ErrorCode::Unavailable, "notebook preview is closed")),
                output = client.execute(request) => output,
            }?;
            let Output::NotebookPage(page) = output else {
                return Err(Fault::invalid("invalid notebook page response"));
            };
            if page.path != path || revision.as_ref().is_some_and(|revision| revision != &page.revision) {
                return Err(Fault::new(ErrorCode::RevisionConflict, "notebook changed while reading cells"));
            }
            Ok(page)
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await.unwrap_or_else(|_| {
                Err(Fault::new(
                    ErrorCode::Unavailable,
                    "notebook preview worker failed",
                ))
            });
            let _ = owner.update_in(cx, |owner, window, cx| {
                owner.loading = false;
                if owner.stop.is_cancelled() || owner.host.check().is_err() {
                    return;
                }
                match result {
                    Ok(page) => {
                        if let Some(notebook) = &mut owner.notebook {
                            notebook.append(page, window, cx);
                        } else {
                            owner.notebook = Some(Notebook::new(page, window, cx));
                        }
                    }
                    Err(fault) => {
                        owner.failed = true;
                        crate::feedback::error("", &fault.message, window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Render for View {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.host.check().is_err() {
            return div().into_any_element();
        }
        if !self.started {
            self.load(window, cx);
        }
        let mut view = v_flex()
            .size_full()
            .min_h_0()
            .debug_selector(|| "notebook-resource".into());
        if let Some(notebook) = &self.notebook {
            view = view.child(notebook.view(
                SharedString::from(format!("notebook-resource-{}", self.id)),
                cx,
            ));
        } else if self.loading {
            view = view.child(
                div()
                    .p_4()
                    .text_color(cx.theme().muted_foreground)
                    .child(crate::tr("artifact_preview_loading")),
            );
        }
        if self.failed
            || self
                .notebook
                .as_ref()
                .is_some_and(|value| value.next.is_some())
        {
            view = view.child(
                Button::new("notebook-resource-more")
                    .label(crate::tr(if self.failed {
                        "settings_retry"
                    } else {
                        "notebook_more"
                    }))
                    .ghost()
                    .loading(self.loading)
                    .disabled(self.loading)
                    .debug_selector(|| "notebook-resource-more".into())
                    .on_click(cx.listener(|owner, _, window, cx| owner.load(window, cx))),
            );
        }
        view.into_any_element()
    }
}

#[cfg(test)]
mod tests;
