//! Controller-local exports reuse the ordinary save, cancellation and image paths.
use super::*;
use crate::{downloads::destination::Destination, live::menus::export::image};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    notification::Notification,
    spinner::Spinner,
};
use sailry_link::CancellationToken;
use std::{path::PathBuf, sync::Arc};
use tokio::{io::AsyncWriteExt as _, sync::oneshot};

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Format {
    Html,
    Png,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Png => "png",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Options {
    pub artifact: ArtifactId,
    pub revision: u32,
    pub format: Format,
}

struct Source {
    host: Arc<Host>,
    owner: sailry_protocol::SessionId,
    options: Options,
    live: Option<WeakEntity<surface::View>>,
}

pub(super) fn choose(
    resource: Resource,
    host: Arc<Host>,
    options: Options,
    live: Option<WeakEntity<surface::View>>,
    reply: oneshot::Sender<bool>,
    window: &mut Window,
    cx: &mut App,
) {
    if options.revision == 0 || options.artifact != resource.artifact.id {
        crate::feedback::error("", &crate::tr("artifact_export_failed"), window, cx);
        let _ = reply.send(false);
        return;
    }
    let name: String = resource
        .artifact
        .title
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
    let filename = format!(
        "{}-v{}.{}",
        if name.is_empty() { "document" } else { &name },
        options.revision,
        options.format.extension()
    );
    let selected =
        cx.prompt_for_new_path(&std::env::home_dir().unwrap_or_default(), Some(&filename));
    let source = Source {
        host,
        owner: resource.artifact.session,
        options,
        live,
    };
    window
        .spawn(cx, async move |cx| {
            let selected = selected.await;
            let _ = cx.update(|window, cx| match selected {
                Ok(Ok(Some(path))) => start(resource, source, path, reply, window, cx),
                Ok(Ok(None)) => {
                    let _ = reply.send(false);
                }
                _ => {
                    crate::feedback::error("", &crate::tr("files_download_picker"), window, cx);
                    let _ = reply.send(false);
                }
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
            .debug_selector(|| "artifact-export-progress".into())
            .gap_2()
            .child(Spinner::new().small())
            .child(crate::tr("conversation_export"))
    }
}

fn start(
    resource: Resource,
    source: Source,
    path: PathBuf,
    reply: oneshot::Sender<bool>,
    window: &mut Window,
    cx: &mut App,
) {
    let cancel = CancellationToken::new();
    let form = cx.new(|_| Progress {
        cancel: cancel.clone(),
    });
    let progress = ("artifact-export", form.entity_id());
    let closing = cancel.clone();
    let stopping = cancel.clone();
    crate::feedback::toast(
        window,
        crate::tr("conversation_export"),
        Notification::new()
            .id1::<Progress>(progress)
            .autohide(false)
            .content(move |_, _, _| div().child(form.clone()).into_any_element())
            .action(move |_, _, cx| {
                let cancel = stopping.clone();
                Button::new("artifact-export-cancel")
                    .debug_selector(|| "artifact-export-cancel".to_owned())
                    .label(crate::tr("settings_cancel"))
                    .ghost()
                    .on_click(cx.listener(move |toast, _, window, cx| {
                        cx.stop_propagation();
                        cancel.cancel();
                        toast.dismiss(window, cx);
                    }))
            })
            .on_close(move |_, _| closing.cancel()),
        cx,
    );
    let runtime = resource.binding.runtime;
    window
        .spawn(cx, async move |cx| {
            let result = run(source, path.clone(), runtime, cancel, cx).await;
            let successful = result.is_ok();
            let _ = cx.update(|window, cx| {
                window.remove_notification1::<Progress>(progress, cx);
                match result {
                    Ok(()) => {
                        crate::feedback::info("", &crate::tr("conversation_exported"), window, cx);
                        crate::plugins::file_transfers::system::show(
                            crate::plugins::file_transfers::system::Action::Reveal,
                            &path,
                            cx,
                        );
                    }
                    Err("files_download_cancelled") => (),
                    Err(key) => crate::feedback::error("", &crate::tr(key), window, cx),
                }
            });
            let _ = reply.send(successful);
        })
        .detach();
}

async fn run(
    source: Source,
    path: PathBuf,
    runtime: Arc<tokio::runtime::Runtime>,
    cancel: CancellationToken,
    cx: &mut AsyncWindowContext,
) -> Result<(), &'static str> {
    let Source {
        host,
        owner,
        options,
        live,
    } = source;
    let image = options.format == Format::Png;
    let staging = runtime.spawn_blocking(move || prepare(path, image));
    let prepared = tokio::select! {
        _ = cancel.cancelled() => return Err("files_download_cancelled"),
        result = staging => result.map_err(|_| "files_download_destination")??,
    };
    let read = host
        .read_artifact(options.artifact, Some(options.revision), owner)
        .map_err(|_| "artifact_export_failed")?;
    let content = tokio::select! {
        _ = cancel.cancelled() => return Err("files_download_cancelled"),
        result = read => result.map_err(|_| "artifact_export_failed")?,
    };
    let task = cx
        .update(|window, cx| match live.and_then(|view| view.upgrade()) {
            Some(view) => view.update(cx, |view, cx| view.capture(options.revision, window, cx)),
            None => page::capture(content, window, cx),
        })
        .map_err(|_| "artifact_export_failed")?;
    let html = tokio::select! {
        _ = cancel.cancelled() => return Err("files_download_cancelled"),
        result = task => result.map_err(|_| "artifact_export_failed")?,
    };
    let bytes = if let Some(directory) = &prepared.images {
        let task = cx
            .update(|window, cx| {
                image::render(
                    image_document(html),
                    "assets".into(),
                    directory.path().to_owned(),
                    window,
                    cx,
                )
            })
            .map_err(|_| "artifact_export_failed")?;
        tokio::select! {
            _ = cancel.cancelled() => return Err("files_download_cancelled"),
            result = task => result.map_err(|_| "artifact_export_failed")?,
        }
    } else {
        html.into_bytes()
    };
    runtime
        .spawn(publish(prepared, bytes, cancel))
        .await
        .map_err(|_| "files_download_destination")?
}

struct Prepared {
    destination: Destination,
    images: Option<tempfile::TempDir>,
}

fn prepare(path: PathBuf, image: bool) -> Result<Prepared, &'static str> {
    let destination = Destination::prepare(path.clone())?;
    let images = if image {
        let parent = path.parent().ok_or("files_download_destination")?;
        let directory = tempfile::Builder::new()
            .prefix(".sailry-export-")
            .tempdir_in(parent)
            .map_err(|_| "files_download_destination")?;
        image::assets(directory.path())?;
        Some(directory)
    } else {
        None
    };
    Ok(Prepared {
        destination,
        images,
    })
}

pub(super) fn image_document(html: String) -> String {
    html.replacen(
        "</head>",
        "<script>window.sailryExportReady=Promise.resolve();</script></head>",
        1,
    )
}

async fn publish(
    prepared: Prepared,
    bytes: Vec<u8>,
    cancel: CancellationToken,
) -> Result<(), &'static str> {
    let mut writer = tokio::fs::File::from_std(prepared.destination.writer()?);
    writer
        .write_all(&bytes)
        .await
        .map_err(|_| "files_download_destination")?;
    drop(writer);
    tokio::task::spawn_blocking(move || prepared.destination.publish(&cancel))
        .await
        .map_err(|_| "files_download_destination")?
}
