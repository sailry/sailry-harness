use super::*;
use crate::{downloads::destination::Destination, theme::DialogStyle as _};
use gpui_kit::component::{button::Button, dialog::DialogFooter, spinner::Spinner};
use sailry_client::conversation::export::{Format, attachment_name};
use sailry_link::CancellationToken;
use sailry_protocol::{Output, Session, SessionId};
use std::{path::PathBuf, sync::Arc};
use tokio::io::AsyncWriteExt as _;

#[cfg(test)]
mod tests;

pub(super) fn choose(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    session: Session,
    format: Format,
    window: &mut Window,
    cx: &mut App,
) {
    let name: String = crate::activity::title(&session)
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let selected = cx.prompt_for_new_path(
        &std::env::home_dir().unwrap_or_default(),
        Some(&format!(
            "{}.{}",
            if name.is_empty() {
                "conversation"
            } else {
                &name
            },
            format.extension()
        )),
    );
    window
        .spawn(cx, async move |cx| {
            let selected = selected.await;
            _ = cx.update(|window, cx| match selected {
                Ok(Ok(Some(path))) => start(client, runtime, session, format, path, window, cx),
                Ok(Ok(None)) => (),
                _ => crate::feedback::error("", &tr("files_download_picker"), window, cx),
            });
        })
        .detach();
}

struct Progress {
    cancel: CancellationToken,
}
impl Drop for Progress {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl Render for Progress {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().py_4().child(Spinner::new())
    }
}

fn start(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    session: Session,
    format: Format,
    path: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    let cancel = CancellationToken::new();
    let form = cx.new(|_| Progress {
        cancel: cancel.clone(),
    });
    let closing = cancel.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let cancel = closing.clone();
        dialog
            .form_title(tr("conversation_export"))
            .w(px(360.))
            .overlay_closable(false)
            .on_close(move |_, _, _| cancel.cancel())
            .child(form.clone())
            .footer(
                DialogFooter::new().child(
                    Button::new("conversation-export-cancel")
                        .label(tr("settings_cancel"))
                        .debug_selector(|| "conversation-export-cancel".to_owned())
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
    let closing = cancel.clone();
    let title = crate::activity::title(&session).to_string();
    let job = runtime.spawn(save(client, session.id, title, format, path, cancel));
    window
        .spawn(cx, async move |cx| {
            let result = job.await;
            _ = cx.update(|window, cx| {
                if !closing.is_cancelled() {
                    window.close_dialog(cx);
                }
                match result {
                    Ok(Ok(())) => {
                        crate::feedback::info("", &tr("conversation_exported"), window, cx)
                    }
                    Ok(Err("files_download_cancelled")) => (),
                    Ok(Err(key)) => crate::feedback::error("", &tr(key), window, cx),
                    Err(_) => {
                        crate::feedback::error("", &tr("conversation_export_failed"), window, cx)
                    }
                }
            });
        })
        .detach();
}

async fn save(
    client: Arc<Client>,
    session: SessionId,
    title: String,
    format: Format,
    path: PathBuf,
    cancel: CancellationToken,
) -> Result<(), &'static str> {
    let destination = Destination::prepare(path.clone())?;
    let parent = path.parent().ok_or("files_download_destination")?;
    let staging = tempfile::Builder::new()
        .prefix(".sailry-export-")
        .tempdir_in(parent)
        .map_err(|_| "files_download_destination")?;
    let assets = format!("sailry-assets-{}", sailry_protocol::RequestId::new());
    let document = client
        .export_conversation(session, &title, format, &assets, &cancel)
        .await
        .map_err(|error| {
            if error.code == sailry_protocol::ErrorCode::Cancelled {
                "files_download_cancelled"
            } else {
                "conversation_export_failed"
            }
        })?;
    for attachment in &document.attachments {
        let command = document
            .images
            .iter()
            .find(|image| image.attachment.id == attachment.id)
            .map_or_else(
                || sailry_protocol::Command::DownloadAttachment {
                    worktree: attachment.spec.worktree,
                    attachment: attachment.id,
                },
                |image| sailry_protocol::Command::DownloadImage {
                    session,
                    image: image.clone(),
                },
            );
        let output = tokio::select! { _ = cancel.cancelled() => return Err("files_download_cancelled"), result = client.execute(client.prepare(command)) => result.map_err(|_| "conversation_export_failed")? };
        let Output::AttachmentDownload(download) = output else {
            return Err("conversation_export_failed");
        };
        if download.attachment != *attachment {
            return Err("conversation_export_failed");
        }
        let mut file = tokio::fs::File::create(staging.path().join(attachment_name(attachment)))
            .await
            .map_err(|_| "files_download_destination")?;
        client
            .download_attachment(&download, &mut file, cancel.clone(), |_| {})
            .await
            .map_err(|error| {
                if error.code == sailry_protocol::ErrorCode::Cancelled {
                    "files_download_cancelled"
                } else {
                    "conversation_export_failed"
                }
            })?;
        file.sync_all()
            .await
            .map_err(|_| "files_download_destination")?;
    }
    let has_assets = !document.attachments.is_empty();
    let published = parent.join(&assets);
    let mut writer = tokio::fs::File::from_std(destination.writer()?);
    writer
        .write_all(document.content.as_bytes())
        .await
        .map_err(|_| "files_download_destination")?;
    drop(writer);
    if cancel.is_cancelled() {
        return Err("files_download_cancelled");
    }
    if has_assets {
        tokio::fs::rename(staging.path(), &published)
            .await
            .map_err(|_| "files_download_destination")?;
    }
    let stopping = cancel.clone();
    let result = tokio::task::spawn_blocking(move || destination.publish(&stopping))
        .await
        .map_err(|_| "files_download_destination")?;
    if result.is_err() && has_assets {
        let _ = tokio::fs::remove_dir_all(published).await;
    }
    result
}
