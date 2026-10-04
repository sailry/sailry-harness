use super::*;
use sailry_protocol::{ErrorCode, Fault, plugin};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
mod source;

pub(in crate::settings::plugins) struct Package {
    pub upload: plugin::Upload,
    pub info: plugin::Info,
    pub directory: Option<PathBuf>,
    _lease: Lease,
}

struct Lease {
    binding: Binding,
    stream: sailry_protocol::StreamId,
}

impl Drop for Lease {
    fn drop(&mut self) {
        let client = self.binding.client.clone();
        let request = client.prepare(Command::CancelFileTransfer {
            stream: self.stream,
        });
        self.binding.runtime.spawn(async move {
            let _ = client.execute(request).await;
        });
    }
}

impl Editor {
    pub(super) fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading || self.pending || self.request.is_some() || self.closed {
            return;
        }
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some(tr("plugins_choose_package")),
        });
        let keep_alive = cx.entity();
        cx.spawn_in(window, async move |editor, cx| {
            let _keep_alive = keep_alive;
            let selected = selected.await;
            let _ = editor.update_in(cx, |editor, window, cx| match selected {
                Ok(Ok(Some(mut paths))) if paths.len() == 1 => {
                    editor.load(paths.remove(0), window, cx)
                }
                Ok(Ok(None)) => {}
                _ => {
                    editor.error = Some("files_upload_picker");
                    editor.report(window, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn load(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_upload(path, false, window, cx);
    }

    pub(super) fn load_directory(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_upload(path, true, window, cx);
    }

    fn prepare_upload(
        &mut self,
        path: PathBuf,
        reuse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loading || self.pending || self.request.is_some() || self.closed {
            return;
        }
        self.package = None;
        self.error = None;
        self.loading = true;
        crate::feedback::status(
            window,
            tr("plugins_preparing_package"),
            gpui_kit::component::notification::NotificationType::Info,
            Notification::info(tr("plugins_preparing_package"))
                .id1::<Editor>(("package", cx.entity_id()))
                .autohide(false),
            cx,
        );
        let binding = self.binding.clone();
        let stop = self.stop.clone();
        let job = binding.runtime.clone().spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "plugin selection cancelled")),
                result = prepare(binding, path, reuse, stop.clone()) => result,
            }
        });
        let keep_alive = cx.entity();
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let _keep_alive = keep_alive;
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                editor.loading = false;
                if editor.closed {
                    return;
                }
                match result {
                    Ok(Ok(Prepared::Package(package))) => {
                        let package = *package;
                        if editor.updating
                            && editor
                                .original
                                .as_ref()
                                .is_some_and(|original| original.name != package.info.summary.name)
                        {
                            editor.error = Some("plugins_package_mismatch");
                        } else {
                            if !editor.updating {
                                editor.original = editor
                                    .known
                                    .iter()
                                    .find(|known| known.name == package.info.summary.name)
                                    .cloned();
                            }
                            editor.package = Some(package);
                        }
                    }
                    Ok(Ok(Prepared::MissingDirectory)) => {
                        window.remove_notification1::<Editor>(("package", cx.entity_id()), cx);
                        editor.choose(window, cx);
                        cx.notify();
                        return;
                    }
                    Ok(Err(error)) => editor.error = Some(live::error_key(&error)),
                    _ => editor.error = Some("plugins_failed"),
                }
                if editor.package.is_some() {
                    editor.submit(window, cx);
                } else {
                    editor.report(window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

pub(in crate::settings::plugins) enum Prepared {
    Package(Box<Package>),
    MissingDirectory,
}

pub(in crate::settings::plugins) async fn prepare(
    binding: Binding,
    path: PathBuf,
    reuse: bool,
    stop: sailry_link::CancellationToken,
) -> Result<Prepared, Fault> {
    let invalid = || {
        Fault::new(
            ErrorCode::InvalidRequest,
            "select a valid plugin ZIP or directory",
        )
    };
    let cancel = stop.clone();
    let source = tokio::task::spawn_blocking(move || source::open(path, cancel))
        .await
        .map_err(|_| invalid())?;
    let source = match source {
        Err(error) if reuse && error.code == ErrorCode::NotFound => {
            return Ok(Prepared::MissingDirectory);
        }
        source => source?,
    };
    if reuse && source.directory.is_none() {
        return Err(invalid());
    }
    let mut file = tokio::fs::File::from_std(source.file);
    let metadata = file.metadata().await.map_err(|_| invalid())?;
    let size = metadata.len();
    if !metadata.is_file() || size == 0 || size > plugin::MAX_PACKAGE_BYTES {
        return Err(invalid());
    }
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0; 64 * 1024];
    let mut read = 0;
    loop {
        let count = file.read(&mut buffer).await.map_err(|_| invalid())?;
        if count == 0 {
            break;
        }
        read += count as u64;
        if read > size {
            return Err(invalid());
        }
        hash.update(&buffer[..count]);
    }
    if read != size {
        return Err(invalid());
    }
    file.rewind().await.map_err(|_| invalid())?;
    let spec = plugin::UploadSpec {
        size,
        revision: hash.finalize().to_hex().to_string(),
    };
    let Output::PluginUpload(upload) = binding
        .client
        .execute(binding.client.prepare(Command::UploadPlugin(spec)))
        .await?
    else {
        return Err(invalid());
    };
    let lease = Lease {
        binding: binding.clone(),
        stream: upload.stream,
    };
    binding
        .client
        .upload_plugin(&upload, &mut file, stop, |_| {})
        .await?;
    let Output::Plugin(info) = binding
        .client
        .execute(binding.client.prepare(Command::InspectPluginUpload {
            stream: upload.stream,
        }))
        .await?
    else {
        return Err(invalid());
    };
    Ok(Prepared::Package(Box::new(Package {
        upload,
        info,
        directory: source.directory,
        _lease: lease,
    })))
}
