//! Native destination selection and progress for shared Client downloads.
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    component::{button::Button, progress::Progress, *},
    *,
};
use sailry_client::Client;
use sailry_link::CancellationToken;
use std::{path::PathBuf, sync::Arc};

mod destination;
#[cfg(test)]
mod tests;
pub(crate) mod transfer;
use transfer::Status;

pub(crate) fn attachment(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    attachment: sailry_protocol::attachment::Attachment,
    window: &mut Window,
    cx: &mut App,
) {
    let name = attachment.spec.name.clone();
    select_attachment(
        client,
        runtime,
        transfer::Source::Attachment(attachment),
        name,
        window,
        cx,
    );
}

pub(crate) fn image(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    session: sailry_protocol::SessionId,
    image: sailry_protocol::conversation::Image,
    window: &mut Window,
    cx: &mut App,
) {
    let name = image.attachment.spec.name.clone();
    select_attachment(
        client,
        runtime,
        transfer::Source::Image { session, image },
        name,
        window,
        cx,
    );
}

pub(crate) fn local(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    source: transfer::Source,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    select_attachment(client, runtime, source, name, window, cx);
}

fn select_attachment(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    source: transfer::Source,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let selected = cx.prompt_for_new_path(&std::env::home_dir().unwrap_or_default(), Some(&name));
    window
        .spawn(cx, async move |cx| {
            let selected = selected.await;
            let _ = cx.update(|window, cx| match selected {
                Ok(Ok(Some(path))) => start(client, runtime, source, name, path, window, cx),
                Ok(Ok(None)) => {}
                _ => crate::feedback::error(
                    &tr("files_download"),
                    &tr("files_download_picker"),
                    window,
                    cx,
                ),
            });
        })
        .detach();
}

pub(crate) fn start(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    source: transfer::Source,
    name: String,
    path: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    let cancel = CancellationToken::new();
    let form = cx.new(|_| Form {
        path: name,
        destination: path.to_string_lossy().into_owned(),
        cancel: cancel.clone(),
        status: Status::Preparing,
    });
    let (updates, mut changes) = tokio::sync::watch::channel(Status::Preparing);
    runtime.spawn(transfer::run(client, source, path, cancel, updates));
    form.update(cx, |_, cx| {
        cx.spawn_in(window, async move |form, cx| {
            while changes.changed().await.is_ok() {
                let status = changes.borrow_and_update().clone();
                if form
                    .update_in(cx, |form, window, cx| form.accept(status, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    });
    open(form, window, cx);
}

fn open(form: Entity<Form>, window: &mut Window, cx: &mut App) {
    let close = form.clone();
    window.open_dialog(cx, move |dialog, window, _| {
        let close = close.clone();
        dialog
            .form_title(tr("files_download"))
            .w((window.viewport_size().width - px(48.)).min(px(460.)))
            .overlay_closable(false)
            .on_ok(|_, _, _| false)
            .on_close(move |_, _, cx| close.update(cx, |form, _| form.cancel.cancel()))
            .child(form.clone())
    });
}

struct Form {
    path: String,
    destination: String,
    cancel: CancellationToken,
    status: Status,
}

impl Form {
    fn accept(&mut self, status: Status, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.status, Status::Finished(_)) {
            match &status {
                Status::Finished(Ok(())) => crate::feedback::status(
                    window,
                    tr("files_download_done"),
                    notification::NotificationType::Success,
                    notification::Notification::success(tr("files_download_done")),
                    cx,
                ),
                Status::Finished(Err("files_download_cancelled")) => {
                    crate::feedback::info("", &tr("files_download_cancelled"), window, cx)
                }
                Status::Finished(Err(key)) => crate::feedback::error("", &tr(key), window, cx),
                _ => {}
            }
        }
        self.status = status;
        cx.notify();
    }
}

impl Drop for Form {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (key, progress) = match &self.status {
            Status::Preparing => ("files_download_preparing", None),
            Status::Receiving { copied, size } => (
                "files_download_receiving",
                Some(if *size == 0 {
                    100.
                } else {
                    *copied as f32 / *size as f32 * 100.
                }),
            ),
            Status::Publishing => ("files_download_publishing", None),
            Status::Finished(Ok(())) => ("files_download_done", Some(100.)),
            Status::Finished(Err(error)) => (*error, None),
        };
        let finished = matches!(self.status, Status::Finished(_));
        v_flex()
            .id("file-download")
            .when(finished, |view| {
                view.debug_selector(|| "file-download-finished".into())
            })
            .gap_3()
            .child(div().text_sm().child(self.path.clone()))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.destination.clone()),
            )
            .when(!matches!(self.status, Status::Finished(Err(_))), |view| {
                view.child(
                    Progress::new("file-download-progress")
                        .accessibility_label(tr("files_download"))
                        .loading(progress.is_none())
                        .value(progress.unwrap_or_default()),
                )
            })
            .when(!finished, |view| {
                view.child(
                    div()
                        .debug_selector(move || key.into())
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr(key)),
                )
            })
            .child(
                gpui_kit::component::dialog::DialogFooter::new()
                    .w_full()
                    .mt_3()
                    .child(
                        Button::new("file-download-close")
                            .debug_selector(|| "file-download-close".into())
                            .label(tr(if finished {
                                "files_download_close"
                            } else {
                                "settings_cancel"
                            }))
                            .disabled(
                                !finished
                                    && (self.cancel.is_cancelled()
                                        || matches!(self.status, Status::Publishing)),
                            )
                            .on_click(cx.listener(move |form, _, window, cx| {
                                if finished {
                                    window.close_dialog(cx);
                                } else {
                                    form.cancel.cancel();
                                    cx.notify();
                                }
                            })),
                    ),
            )
    }
}
