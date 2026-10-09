//! The native editor observes core ownership and locks independently of script redraws.
use super::*;
use crate::plugins::host::Host;
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::Editor,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder;
use std::sync::Arc;

pub(super) struct Surface {
    controller: Entity<Controller>,
    document: RequestId,
    selector: String,
    host: Arc<Host>,
    write: bool,
    _subscriptions: Vec<Subscription>,
}
impl Surface {
    pub(super) fn new(
        controller: Entity<Controller>,
        document: RequestId,
        selector: String,
        host: Arc<Host>,
        write: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let transfers = crate::plugins::file_transfers::registry(cx);
        let document_updates = cx.observe(&controller, |_, _, cx| cx.notify());
        let transfer_updates = cx.observe(&transfers, |_, _, cx| cx.notify());
        Self {
            controller,
            document,
            selector,
            host,
            write,
            _subscriptions: vec![document_updates, transfer_updates],
        }
    }
}
impl Render for Surface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.host.check().is_err() {
            return div().into_any_element();
        }
        let controller = self.controller.read(cx);
        let Some(document) = controller.documents.get(&self.document) else {
            return div().into_any_element();
        };
        let readonly = !self.write
            || document.revision.is_none()
            || document.uncertain
            || controller.locked(&document.path, cx);
        let view = if let Some(notebook) = &document.notebook
            && notebook.preview
        {
            notebook.render(self.document, &self.controller, &self.host.context(), cx)
        } else if let Some(markdown) = &document.markdown {
            crate::content::editor::View::new(markdown)
                .readonly(readonly)
                .aria_label(document.path.clone())
                .into_any_element()
        } else {
            Editor::new(&document.input)
                .readonly(readonly)
                .appearance(false)
                .bordered(false)
                .text_sm()
                .px_0()
                .h_full()
                .aria_label(document.path.clone())
                .into_any_element()
        };
        let provider = document.code.clone();
        let startup = provider
            .as_ref()
            .and_then(|provider| provider.startup.borrow().clone());
        let starting = provider
            .as_ref()
            .is_some_and(|provider| *provider.starting.borrow());
        let retry = provider
            .as_ref()
            .is_some_and(|provider| provider.attempt.borrow().is_some());
        let controller = self.controller.clone();
        let id = self.document;
        v_flex()
            .key_context("FileEditor")
            .debug_selector({
                let selector = self.selector.clone();
                move || selector.clone()
            })
            .size_full()
            .min_w_0()
            .min_h_0()
            .on_action(
                cx.listener(|surface, _: &crate::resources::SaveFile, _, cx| {
                    if surface.host.check().is_err() {
                        return;
                    }
                    let (reply, _) = tokio::sync::oneshot::channel();
                    surface.controller.update(cx, |_, cx| {
                        cx.emit(Request {
                            context: surface.host.context().clone(),
                            write: surface.write,
                            stop: surface.host.stop_token(),
                            operation: Operation::Save {
                                id: surface.document,
                            },
                            reply: RefCell::new(Some(reply)),
                        })
                    });
                }),
            )
            .when(startup.is_some() || retry, |surface| {
                surface.child(
                    h_flex().py_1().gap_2().child(
                        Button::new("editor-code-start")
                            .ghost()
                            .small()
                            .label(if retry {
                                crate::tr("editor_code_retry")
                            } else {
                                crate::tr("editor_code_start")
                            })
                            .loading(starting)
                            .disabled(starting)
                            .debug_selector(|| "editor-code-start".into())
                            .on_click(move |_, window, cx| {
                                controller.update(cx, |controller, cx| {
                                    controller.confirm_code(id, window, cx)
                                })
                            }),
                    ),
                )
            })
            .child(div().flex_1().min_h_0().child(view))
            .into_any_element()
    }
}
