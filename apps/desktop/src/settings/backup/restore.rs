use super::*;
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    form::Field,
    input::{Input, InputState},
};
use sailry_protocol::{
    ErrorCode, Fault, Output, Secret,
    backup::{Location, Record},
};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};

#[derive(Clone)]
enum Source {
    Record { id: String, location: Location },
    File(PathBuf),
}

pub(super) struct Restore {
    owner: Entity<Panel>,
    source: Source,
    pub(super) target: Entity<InputState>,
    pub(super) passphrase: Entity<InputState>,
    error: Option<&'static str>,
    loading: bool,
    stop: CancellationToken,
    task: Option<Task<()>>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(super) fn open(
    owner: Entity<Panel>,
    record: Record,
    location: Location,
    window: &mut Window,
    cx: &mut App,
) {
    form(
        owner,
        Source::Record {
            id: record.id,
            location,
        },
        window,
        cx,
    );
}

pub(super) fn open_file(owner: Entity<Panel>, window: &mut Window, cx: &mut App) {
    if owner.read(cx).locked() {
        return;
    }
    let selected = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(tr("backup_choose_archive")),
    });
    window
        .spawn(cx, async move |cx| match selected.await {
            Ok(Ok(Some(mut paths))) if paths.len() == 1 => {
                _ = cx.update(|window, cx| {
                    if !owner.read(cx).locked() {
                        form(owner, Source::File(paths.remove(0)), window, cx);
                    }
                });
            }
            Ok(Ok(None)) => {}
            _ => {
                _ = cx.update(|window, cx| {
                    crate::feedback::error("", &tr("files_upload_picker"), window, cx)
                });
            }
        })
        .detach();
}

fn form(owner: Entity<Panel>, source: Source, window: &mut Window, cx: &mut App) {
    let configured = owner
        .read(cx)
        .settings
        .as_ref()
        .is_some_and(|settings| settings.passphrase_configured);
    let form = cx.new(|cx| {
        crate::feedback::observe(window, cx, |form: &Restore, _| {
            form.error.into_iter().collect()
        });
        Restore {
            owner,
            source,
            target: cx.new(|cx| InputState::new(window, cx).placeholder(tr("backup_target"))),
            passphrase: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(tr(if configured {
                        "backup_keep_secret"
                    } else {
                        "backup_passphrase"
                    }))
            }),
            error: None,
            loading: false,
            stop: CancellationToken::new(),
            task: None,
        }
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        let restore = form.clone();
        dialog
            .form_title(tr("backup_restore"), cx)
            .w((window.viewport_size().width - px(48.)).min(px(560.)))
            .child(form.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("backup-restore-close")
                            .label(tr("close"))
                            .disabled(form.read(cx).loading)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("backup-restore-confirm")
                            .danger()
                            .label(tr("backup_restore"))
                            .debug_selector(|| "backup-restore-confirm".into())
                            .loading(form.read(cx).loading)
                            .disabled(
                                form.read(cx).loading || form.read(cx).owner.read(cx).locked(),
                            )
                            .on_click(move |_, window, cx| {
                                if restore.update(cx, |form, cx| form.submit(window, cx)) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
    });
}

impl Restore {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.owner.read(cx).active || self.loading {
            self.error = Some("backup_target_changed");
            cx.notify();
            return false;
        }
        let target = self.target.read(cx).value().trim().to_owned();
        if target.is_empty() {
            self.error = Some("backup_invalid_target");
            cx.notify();
            return false;
        }
        let Some(settings) = self.owner.read(cx).settings.clone() else {
            return false;
        };
        let passphrase = self.passphrase.read(cx).value().to_string();
        if passphrase.is_empty() && !settings.passphrase_configured {
            self.error = Some("backup_passphrase_required");
            cx.notify();
            return false;
        }
        let passphrase = (!passphrase.is_empty()).then(|| Secret::new(passphrase));
        match self.source.clone() {
            Source::Record { id, location } => self.owner.update(cx, |panel, cx| {
                panel.submit(
                    Command::RestoreBackup {
                        id,
                        location,
                        expected_revision: settings.revision,
                        target,
                        passphrase,
                    },
                    window,
                    cx,
                )
            }),
            Source::File(path) => {
                self.loading = true;
                self.error = None;
                let binding = self.owner.read(cx).binding.clone();
                let generation = self.owner.read(cx).generation;
                let stop = self.stop.child_token();
                let job = binding.runtime.clone().spawn(stage(binding, path, stop));
                self.task = Some(cx.spawn_in(window, async move |owner, cx| {
                    let result = job.await;
                    _ = owner.update_in(cx, |form, window, cx| {
                        form.loading = false;
                        match result {
                            Ok(Ok(mut staged)) => {
                                let submitted = form.owner.update(cx, |panel, cx| {
                                    if !panel.active || panel.generation != generation {
                                        return false;
                                    }
                                    panel.submit(
                                        Command::RestoreBackupFile {
                                            stream: staged.upload.stream,
                                            expected_revision: settings.revision,
                                            target,
                                            passphrase,
                                        },
                                        window,
                                        cx,
                                    )
                                });
                                if submitted {
                                    staged.handed_off = true;
                                    window.close_dialog(cx);
                                } else {
                                    form.error = Some("backup_target_changed");
                                }
                            }
                            Ok(Err(error)) => crate::feedback::error(
                                &tr("backup_failed"),
                                &crate::feedback::fault_message(&error),
                                window,
                                cx,
                            ),
                            Err(_) => form.error = Some("backup_failed"),
                        }
                        cx.notify();
                    });
                }));
                cx.notify();
                false
            }
        }
    }
}
impl Render for Restore {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.owner.read(cx).binding.label.clone()),
            )
            .child(div().text_sm().child(tr("backup_restore_consequence")))
            .child(
                Field::new().label(tr("backup_passphrase")).child(
                    div()
                        .debug_selector(|| "backup-restore-passphrase".into())
                        .child(
                            Input::new(&self.passphrase)
                                .disabled(self.loading || self.owner.read(cx).locked()),
                        ),
                ),
            )
            .child(Field::new().label(tr("backup_target")).child(
                div().debug_selector(|| "backup-target".into()).child(
                    Input::new(&self.target).disabled(self.loading || self.owner.read(cx).locked()),
                ),
            ))
    }
}

struct Staged {
    binding: Binding,
    upload: sailry_protocol::backup::Upload,
    handed_off: bool,
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.handed_off {
            let client = self.binding.client.clone();
            let request = client.prepare(Command::CancelFileTransfer {
                stream: self.upload.stream,
            });
            self.binding.runtime.spawn(async move {
                let _ = client.execute(request).await;
            });
        }
    }
}

async fn stage(binding: Binding, path: PathBuf, stop: CancellationToken) -> Result<Staged, Fault> {
    let _cancel = stop.clone().drop_guard();
    let invalid = || Fault::invalid("select a nonempty backup archive file");
    let failed = |error: std::io::Error| {
        Fault::new(
            ErrorCode::Unavailable,
            format!("backup source read failed: {error}"),
        )
    };
    let mut file = tokio::fs::File::open(path).await.map_err(failed)?;
    let metadata = file.metadata().await.map_err(failed)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(invalid());
    }
    let size = metadata.len();
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0; 64 * 1024];
    let mut read = 0_u64;
    loop {
        let count = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "backup upload cancelled")),
            count = file.read(&mut buffer) => count.map_err(failed)?,
        };
        if count == 0 {
            break;
        }
        read = read
            .checked_add(count as u64)
            .filter(|read| *read <= size)
            .ok_or_else(invalid)?;
        hash.update(&buffer[..count]);
    }
    if read != size {
        return Err(invalid());
    }
    file.rewind().await.map_err(failed)?;
    let Output::BackupUpload(upload) = binding
        .client
        .execute(binding.client.prepare(Command::UploadBackup {
            size,
            revision: hash.finalize().to_hex().to_string(),
        }))
        .await?
    else {
        return Err(invalid());
    };
    let staged = Staged {
        binding,
        upload,
        handed_off: false,
    };
    staged
        .binding
        .client
        .upload(&staged.upload, &mut file, stop.clone(), |_| {})
        .await?;
    Ok(staged)
}
