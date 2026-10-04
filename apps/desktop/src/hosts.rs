//! SSH deployment presentation. Execution and credentials remain on the local Node.
use crate::{backend::Services, shell::Shell, theme::DialogStyle as _, tr};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        dialog::DialogFooter,
        notification::{Notification, NotificationType},
        progress::Progress,
        *,
    },
    *,
};
use sailry_client::Client;
use sailry_protocol::{
    ssh::{HostKey, InstallProgress, Outcome, Profile},
    *,
};
use std::{sync::Arc, time::Duration};

mod editor;
mod sharing;
#[cfg(test)]
mod tests;
mod view;
use editor::{Editor, Event};

struct Installer {
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    editor: Entity<Editor>,
    profile: Option<Profile>,
    key: Option<HostKey>,
    changed: bool,
    form: bool,
    progress: InstallProgress,
    request: Option<Request>,
    error: Option<Fault>,
    task: Option<Task<()>>,
    polling: Option<Task<()>>,
}
impl EventEmitter<DismissEvent> for Installer {}

impl Shell {
    pub(crate) fn add_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(services) = cx.try_global::<Services>().cloned() else {
            return;
        };
        let installer = cx.new(|cx| Installer::new(services, window, cx));
        let owner = installer.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            let closing = owner.clone();
            dialog
                .form_title(tr("host_add"))
                .w(px(560.).min(window.viewport_size().width - px(48.)))
                .max_h(window.viewport_size().height * 0.85)
                .overlay_closable(false)
                .on_close(move |_, _, cx| closing.update(cx, |view, _| view.cancel()))
                .child(owner.clone())
                .footer(owner.update(cx, |view, cx| view.footer(cx)))
        });
    }
}
impl Installer {
    fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::feedback::observe_with(
            window,
            cx,
            |view: &Self, _| {
                view.error
                    .as_ref()
                    .map(|error| {
                        if error.code == ErrorCode::OutcomeUnknown {
                            "host_install_unknown"
                        } else {
                            "host_install_failed"
                        }
                    })
                    .into_iter()
                    .collect()
            },
            |view, key, _| {
                crate::feedback::diagnostic(view.error.as_ref().unwrap().message.clone().into())
                    .title(tr(key))
            },
        );
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(None, window, cx);
            editor.action = "host_install";
            editor
        });
        cx.subscribe_in(
            &editor,
            window,
            |this, _, event: &Event, window, cx| match event {
                Event::Save(command) => this.execute(command.as_ref().clone(), window, cx),
                Event::Cancel => {
                    this.cancel();
                    cx.emit(DismissEvent);
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &cx.entity(),
            window,
            |_, _, _: &DismissEvent, window, cx| window.close_dialog(cx),
        )
        .detach();
        Self {
            client: Arc::new(Client::new(services.local)),
            runtime: services.runtime,
            editor,
            profile: None,
            key: None,
            changed: false,
            form: true,
            progress: InstallProgress::Connecting,
            request: None,
            error: None,
            task: None,
            polling: None,
        }
    }
    fn execute(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.request.is_some() {
            return;
        }
        self.error = None;
        self.editor.update(cx, |editor, cx| {
            editor.locked = true;
            cx.notify();
        });
        let request = self.client.prepare(command);
        let install = matches!(request.command, Command::InstallHost { .. });
        self.request = Some(request.clone());
        let client = self.client.clone();
        let job = self
            .runtime
            .spawn(async move { client.execute(request).await });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job.await.unwrap_or_else(|_| {
                Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "Host installation completion unavailable",
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| this.complete(result, window, cx));
        }));
        if install {
            self.poll(cx);
        }
        cx.notify();
    }
    fn complete(
        &mut self,
        result: Result<Output, Fault>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let trusted = self
            .request
            .as_ref()
            .is_some_and(|request| matches!(request.command, Command::TrustSsh { .. }));
        self.request = None;
        self.polling = None;
        match result {
            Ok(Output::SshProfile(profile)) => {
                let command = if trusted {
                    Command::InstallHost {
                        profile: profile.id,
                        expected_revision: profile.revision,
                    }
                } else {
                    Command::CheckSsh {
                        profile: profile.id,
                        expected_revision: profile.revision,
                    }
                };
                self.profile = Some(profile);
                self.form = false;
                self.execute(command, window, cx);
            }
            Ok(Output::SshOutcome(Outcome::HostKeyRequired { key, changed })) => {
                self.key = Some(key);
                self.changed = changed;
            }
            Ok(Output::SshOutcome(Outcome::Connected)) => {
                let profile = self.profile.as_ref().unwrap();
                self.execute(
                    Command::InstallHost {
                        profile: profile.id,
                        expected_revision: profile.revision,
                    },
                    window,
                    cx,
                );
            }
            Ok(Output::SshOutcome(Outcome::HostInstalled { .. })) => {
                crate::feedback::status(
                    window,
                    tr("host_added"),
                    NotificationType::Success,
                    Notification::success(tr("host_added")),
                    cx,
                );
                cx.emit(DismissEvent);
            }
            Err(error) => {
                self.error = Some(error);
                self.editor.update(cx, |editor, cx| {
                    editor.locked = false;
                    cx.notify();
                });
            }
            _ => {
                self.error = Some(Fault::new(
                    ErrorCode::Internal,
                    "Unexpected host installation result",
                ))
            }
        }
        cx.notify();
    }
    fn poll(&mut self, cx: &mut Context<Self>) {
        let id = self.request.as_ref().unwrap().id;
        let client = self.client.clone();
        let runtime = self.runtime.clone();
        self.polling = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(300))
                    .await;
                let client = client.clone();
                let job = runtime.spawn(async move {
                    client
                        .execute(client.prepare(Command::ReadHostInstall { request: id }))
                        .await
                });
                if let Ok(Ok(Output::HostInstallProgress(progress))) = job.await
                    && this
                        .update(cx, |this, cx| {
                            this.progress = progress;
                            cx.notify();
                        })
                        .is_err()
                {
                    break;
                }
            }
        }));
    }
    fn cancel(&mut self) {
        if let Some(request) = self.request.take()
            && matches!(
                request.command,
                Command::InstallHost { .. } | Command::CheckSsh { .. }
            )
        {
            let client = self.client.clone();
            self.runtime.spawn(async move {
                let _ = client
                    .execute(client.prepare(Command::CancelSsh {
                        request: request.id,
                    }))
                    .await;
            });
        }
        self.task = None;
        self.polling = None;
    }
}
impl Drop for Installer {
    fn drop(&mut self) {
        self.cancel();
    }
}
