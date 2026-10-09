//! Native Kit language-service adapters use the execution Node's read-only requests.
use super::*;
use gpui_kit::component::input::EditorState;
use sailry_protocol::{Command, Output, Permission, code};
use std::{cell::RefCell, rc::Rc, time::Duration};

mod mapping;
mod providers;
mod query;
mod startup;
#[cfg(test)]
mod tests;
mod watch;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Hover,
    Definition,
    Completion,
    Diagnostics,
}

pub(super) struct Provider {
    controller: WeakEntity<Controller>,
    input: WeakEntity<EditorState>,
    document: RequestId,
    context: plugin::Context,
    binding: Binding,
    window: AnyWindowHandle,
    stop: CancellationToken,
    requests: RefCell<BTreeMap<Kind, CancellationToken>>,
    observed: RefCell<Option<(String, String)>>,
    pub startup: RefCell<Option<code::Startup>>,
    pub attempt: RefCell<Option<sailry_protocol::Request>>,
    pub starting: RefCell<bool>,
    confirming: RefCell<bool>,
    automatic: RefCell<Option<String>>,
    diagnostics: RefCell<Option<Task<()>>>,
}

impl Controller {
    pub(super) fn attach_code(
        &mut self,
        id: RequestId,
        context: plugin::Context,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.documents.get_mut(&id) else {
            return;
        };
        if self.binding.worktree.is_none()
            || document.truncated
            || document.revision.is_none()
            || document.code.is_some()
        {
            return;
        }
        let provider = Rc::new(Provider {
            controller: cx.entity().downgrade(),
            input: document.input.downgrade(),
            document: id,
            context,
            binding: self.binding.clone(),
            window: window.window_handle(),
            stop: document.stop.clone(),
            requests: Default::default(),
            observed: Default::default(),
            startup: Default::default(),
            attempt: Default::default(),
            starting: Default::default(),
            confirming: Default::default(),
            automatic: Default::default(),
            diagnostics: Default::default(),
        });
        let navigation = provider.clone();
        document.input.update(cx, |input, _| {
            let lsp = input.lsp_mut();
            lsp.hover_provider = Some(provider.clone());
            lsp.definition_provider = Some(provider.clone());
            lsp.completion_provider = Some(provider.clone());
            lsp.show_document = Some(Rc::new(move |params, window, cx| {
                navigation.show(params, window, cx)
            }));
        });
        document.code = Some(provider);
        self.watch_code(cx);
    }

    pub(super) fn refresh_code(&self, cx: &mut Context<Self>) {
        for document in self
            .documents
            .values()
            .filter(|document| !document.dismissed)
        {
            let Some(provider) = &document.code else {
                continue;
            };
            let Some(revision) = &document.revision else {
                continue;
            };
            let text = document.input.read(cx).value().to_string();
            let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
            if provider.observed.borrow().as_ref() == Some(&(revision.clone(), hash.clone())) {
                continue;
            }
            *provider.observed.borrow_mut() = Some((revision.clone(), hash));
            provider.diagnostics(text, cx);
        }
    }
}

impl Provider {
    pub(super) fn pending(&self) -> bool {
        self.attempt.borrow().is_some()
    }

    fn show(
        &self,
        params: &lsp_types::ShowDocumentParams,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        if params.external == Some(true) || self.stop.is_cancelled() {
            return false;
        }
        let Some(path) = mapping::relative_uri(params.uri.as_str()) else {
            return false;
        };
        let Some(controller) = self.controller.upgrade() else {
            return false;
        };
        let context = self.context.clone();
        let position = params.selection.as_ref().map(|selection| selection.start);
        let stop = self.stop.child_token();
        let owner = self.controller.clone();
        window
            .spawn(cx, async move |cx| {
                if stop.is_cancelled() {
                    return;
                }
                let (reply, receive) = tokio::sync::oneshot::channel();
                if cx
                    .update(|window, cx| {
                        controller.update(cx, |controller, cx| {
                            controller.open(
                                load::Target {
                                    path,
                                    line: position.map(|position| position.line as usize + 1),
                                },
                                context,
                                Completion { stop, reply },
                                window,
                                cx,
                            );
                        });
                    })
                    .is_err()
                {
                    return;
                }
                let Ok(Ok(value)) = receive.await else {
                    return;
                };
                let Some(id) = value
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|value| value.parse::<RequestId>().ok())
                else {
                    return;
                };
                if let Some(position) = position {
                    let _ = cx.update(|window, cx| {
                        let _ = owner.update(cx, |controller, cx| {
                            if let Some(document) = controller.documents.get_mut(&id) {
                                document.input.update(cx, |input, cx| {
                                    input.set_cursor_position(position, window, cx)
                                });
                            }
                        });
                    });
                }
            })
            .detach();
        true
    }
}
