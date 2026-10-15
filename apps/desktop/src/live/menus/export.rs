use super::*;
use crate::downloads::destination::Destination;
use gpui_kit::component::{notification::Notification, spinner::Spinner};
use sailry_client::conversation::export::{Format as DocumentFormat, attachment_name};
use sailry_link::CancellationToken;
use sailry_protocol::{Output, Session, TurnId};
use std::{path::PathBuf, sync::Arc};
use tokio::io::AsyncWriteExt as _;

pub(crate) mod image;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Markdown,
    Html,
    Image,
}
impl Format {
    fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Image => "png",
        }
    }
    fn document(self) -> DocumentFormat {
        match self {
            Self::Markdown => DocumentFormat::Markdown,
            Self::Html | Self::Image => DocumentFormat::Html,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Selection {
    pub session: Session,
    pub turn: Option<TurnId>,
}
impl From<Session> for Selection {
    fn from(session: Session) -> Self {
        Self {
            session,
            turn: None,
        }
    }
}

#[cfg(test)]
mod tests;

pub(crate) fn choose(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    selection: impl Into<Selection>,
    format: Format,
    window: &mut Window,
    cx: &mut App,
) {
    let selection = selection.into();
    let name: String = crate::activity::title(&selection.session)
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
                Ok(Ok(Some(path))) => start(client, runtime, selection, format, path, window, cx),
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
        h_flex()
            .id("conversation-export-progress")
            .debug_selector(|| "conversation-export-progress".into())
            .gap_2()
            .text_sm()
            .child(Spinner::new().small())
            .child(tr("conversation_export"))
    }
}

fn start(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    selection: Selection,
    format: Format,
    path: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    let cancel = CancellationToken::new();
    let form = cx.new(|_| Progress {
        cancel: cancel.clone(),
    });
    let progress = ("conversation-export", form.entity_id());
    let closing = cancel.clone();
    crate::feedback::toast(
        window,
        tr("conversation_export"),
        Notification::new()
            .id1::<Progress>(progress)
            .autohide(false)
            .content(move |_, _, _| div().child(form.clone()).into_any_element())
            // Kit's close button cancels; a finished export is already settled
            // when it removes this toast, so the late cancellation is inert.
            .on_close(move |_, _| closing.cancel()),
        cx,
    );
    let title = crate::activity::title(&selection.session).to_string();
    let job = runtime.spawn(prepare(
        client,
        selection,
        title,
        format,
        path.clone(),
        cancel.clone(),
    ));
    window
        .spawn(cx, async move |cx| {
            let result = match job.await {
                Ok(Ok(prepared)) => {
                    let rendered = if format != Format::Markdown {
                        match cx.update(|window, cx| {
                            let render = if format == Format::Html { image::render_html } else { image::render };
                            render(prepared.html.clone(), prepared.assets.clone(), prepared.staging.path().to_owned(), window, cx)
                        }) {
                            Ok(task) => tokio::select! { result = task => result, _ = cancel.cancelled() => Err("files_download_cancelled") },
                            Err(_) => Err("conversation_export_failed"),
                        }
                    } else {
                        Ok(prepared.html.as_bytes().to_vec())
                    };
                    match rendered {
                        Ok(bytes) => runtime
                            .spawn(publish(prepared, bytes, cancel))
                            .await
                            .unwrap_or(Err("conversation_export_failed")),
                        Err(error) => Err(error),
                    }
                }
                Ok(Err(error)) => Err(error),
                Err(_) => Err("conversation_export_failed"),
            };
            _ = cx.update(|window, cx| {
                window.remove_notification1::<Progress>(progress, cx);
                match result {
                    Ok(()) => {
                        crate::feedback::info("", &tr("conversation_exported"), window, cx);
                        crate::plugins::file_transfers::system::show(
                            crate::plugins::file_transfers::system::Action::Reveal,
                            &path,
                            cx,
                        );
                    }
                    Err("files_download_cancelled") => (),
                    Err(key) => crate::feedback::error("", &tr(key), window, cx),
                }
            });
        })
        .detach();
}

async fn prepare(
    client: Arc<Client>,
    selection: Selection,
    title: String,
    format: Format,
    path: PathBuf,
    cancel: CancellationToken,
) -> Result<Prepared, &'static str> {
    let session = selection.session.id;
    let destination = Destination::prepare(path.clone())?;
    let parent = path.parent().ok_or("files_download_destination")?;
    let staging = tempfile::Builder::new()
        .prefix(".sailry-export-")
        .tempdir_in(parent)
        .map_err(|_| "files_download_destination")?;
    let assets = format!("sailry-assets-{}", sailry_protocol::RequestId::new());
    let document = match selection.turn {
        Some(turn) => {
            client
                .export_reply(session, turn, &title, format.document(), &assets, &cancel)
                .await
        }
        None => {
            client
                .export_conversation(session, &title, format.document(), &assets, &cancel)
                .await
        }
    }
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
    if format != Format::Markdown {
        image::assets(staging.path())?;
    }
    Ok(Prepared {
        destination,
        staging,
        assets: assets.clone(),
        html: if format == Format::Markdown {
            format!(
                "{}\n---\n\n[Sailry](https://sailry.dev)\n",
                document.content
            )
        } else {
            image::decorate(&document.content, &assets)
        },
        has_assets: format != Format::Markdown || !document.attachments.is_empty(),
        standalone: format != Format::Markdown,
        parent: parent.to_owned(),
    })
}

struct Prepared {
    destination: Destination,
    staging: tempfile::TempDir,
    assets: String,
    html: String,
    has_assets: bool,
    standalone: bool,
    parent: PathBuf,
}

async fn publish(
    prepared: Prepared,
    bytes: Vec<u8>,
    cancel: CancellationToken,
) -> Result<(), &'static str> {
    let Prepared {
        destination,
        staging,
        assets,
        parent,
        has_assets,
        standalone,
        ..
    } = prepared;
    let has_assets = has_assets && !standalone;
    let published = parent.join(&assets);
    let mut writer = tokio::fs::File::from_std(destination.writer()?);
    writer
        .write_all(&bytes)
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
