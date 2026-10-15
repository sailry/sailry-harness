use super::*;
use crate::theme::DialogStyle as _;
use crate::{plugins::host::sdk::confirmed, tr};
use gpui_kit::component::{
    WindowExt,
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
};

impl Controller {
    #[cfg(test)]
    pub(in crate::plugins::documents) fn confirming_code(&self, id: RequestId) -> bool {
        self.documents
            .get(&id)
            .and_then(|document| document.code.as_ref())
            .is_some_and(|provider| *provider.confirming.borrow())
    }

    pub(in crate::plugins::documents) fn pending_code(&self) -> bool {
        self.documents
            .values()
            .any(|document| document.code_capture.attempt.borrow().is_some())
    }

    pub(in crate::plugins::documents) fn auto_code(
        &self,
        id: RequestId,
        startup: code::Startup,
        approved: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = self
            .documents
            .get(&id)
            .and_then(|document| document.code.clone())
        else {
            return;
        };
        if *provider.starting.borrow()
            || provider.pending()
            || provider.automatic.borrow().as_deref() == Some(&startup.revision)
        {
            return;
        }
        *provider.automatic.borrow_mut() = Some(startup.revision.clone());
        let client = self.binding.client.clone();
        let session = self.session;
        let job = self.binding.runtime.spawn(async move {
            approved || query::automatic(&client, session).await.unwrap_or(false)
        });
        cx.spawn(async move |controller, cx| {
            if !job.await.unwrap_or(false) {
                return;
            }
            let _ = controller.update(cx, |controller, cx| {
                if controller
                    .documents
                    .get(&id)
                    .and_then(|document| document.code.as_ref())
                    .is_some_and(|current| {
                        Rc::ptr_eq(current, &provider)
                            && current.startup.borrow().as_ref() == Some(&startup)
                    })
                {
                    controller.begin_code(id, cx);
                }
            });
        })
        .detach();
    }

    pub(in crate::plugins::documents) fn confirm_code(
        &self,
        id: RequestId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = self
            .documents
            .get(&id)
            .and_then(|document| document.code.as_ref())
        else {
            return;
        };
        if *provider.starting.borrow() || *provider.confirming.borrow() {
            return;
        }
        if provider.pending() {
            self.begin_code(id, cx);
            return;
        }
        let Some(startup) = provider.startup.borrow().clone() else {
            return;
        };
        *provider.confirming.borrow_mut() = true;
        let confirming = provider.clone();
        let client = self.binding.client.clone();
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(client.prepare(Command::Snapshot)).await });
        cx.spawn_in(window, async move |controller, cx| {
            let result = job.await;
            *confirming.confirming.borrow_mut() = false;
            let _ = controller.update_in(cx, |controller, window, cx| {
                let Some(provider) = controller
                    .documents
                    .get(&id)
                    .filter(|document| !document.dismissed)
                    .and_then(|document| document.code.as_ref())
                else {
                    return;
                };
                if !Rc::ptr_eq(provider, &confirming)
                    || provider.startup.borrow().as_ref() != Some(&startup)
                {
                    return;
                }
                let Ok(Ok(Output::Snapshot(snapshot))) = result else {
                    crate::feedback::error(
                        "editor_code_failed",
                        "Worktree metadata is unavailable",
                        window,
                        cx,
                    );
                    return;
                };
                let Some(worktree) = snapshot
                    .worktrees
                    .iter()
                    .find(|worktree| worktree.id == startup.worktree)
                else {
                    return;
                };
                controller.code_consent(id, startup, worktree.path.clone(), window, cx);
            });
        })
        .detach();
    }

    fn code_consent(
        &self,
        id: RequestId,
        startup: code::Startup,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let owner = owner.clone();
            let captured = startup.clone();
            dialog
                .floating_surface(cx)
                .title(tr("editor_code_start"))
                .child(
                    rust_i18n::t!(
                        "approval_code_start",
                        server = startup.server,
                        worktree = path
                    )
                    .to_string(),
                )
                .child(
                    div()
                        .text_sm()
                        .whitespace_normal()
                        .child(startup.executable.clone()),
                )
                .footer(
                    DialogFooter::new().child(
                        Button::new("editor-code-start-confirm")
                            .primary()
                            .label(tr("editor_code_start"))
                            .debug_selector(|| "editor-code-start-confirm".into())
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |controller, cx| {
                                    if controller
                                        .documents
                                        .get(&id)
                                        .and_then(|document| document.code.as_ref())
                                        .is_some_and(|provider| {
                                            provider.startup.borrow().as_ref() == Some(&captured)
                                        })
                                    {
                                        controller.begin_code(id, cx);
                                    }
                                });
                                window.close_dialog(cx);
                            }),
                    ),
                )
        });
    }

    fn begin_code(&self, id: RequestId, cx: &mut Context<Self>) {
        let Some(provider) = self
            .documents
            .get(&id)
            .filter(|document| !document.dismissed)
            .and_then(|document| document.code.clone())
        else {
            return;
        };
        if *provider.starting.borrow() {
            return;
        }
        let request = match provider.attempt.borrow().clone() {
            Some(request) => request,
            None => {
                let Some(startup) = provider.startup.borrow().clone() else {
                    return;
                };
                self.binding
                    .client
                    .prepare(Command::StartCodeServer { startup })
            }
        };
        *provider.attempt.borrow_mut() = Some(request.clone());
        *provider.starting.borrow_mut() = true;
        let client = self.binding.client.clone();
        let job = self
            .binding
            .runtime
            .spawn(async move { confirmed(&client, request).await });
        let window = provider.window;
        cx.spawn(async move |controller, cx| {
            let result = job.await;
            let feedback = controller
                .update(cx, |controller, cx| {
                    let document = controller.documents.get(&id)?;
                    let current = document.code.as_ref();
                    let attached = current.is_some_and(|current| Rc::ptr_eq(current, &provider));
                    *provider.starting.borrow_mut() = false;
                    let feedback = match result {
                        Ok(Ok(Ok(Output::CodeServer(_)))) => {
                            provider.attempt.borrow_mut().take();
                            if attached {
                                provider.startup.borrow_mut().take();
                                provider.observed.borrow_mut().take();
                                provider.automatic.borrow_mut().take();
                            }
                            Ok(())
                        }
                        Ok(Ok(Err(error))) => {
                            if error.code != ErrorCode::OutcomeUnknown {
                                provider.attempt.borrow_mut().take();
                            }
                            Err(error)
                        }
                        _ => Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "Language startup outcome is unknown",
                        )),
                    };
                    controller.refresh_code(cx);
                    cx.notify();
                    Some(feedback)
                })
                .ok()
                .flatten();
            if let Some(Err(error)) = feedback {
                let _ = cx.update_window(window, |_, window, cx| {
                    crate::feedback::error("editor_code_failed", &error.to_string(), window, cx);
                });
            }
        })
        .detach();
        cx.notify();
    }
}
