//! Transfer preparation retains the shared Client's verified streams and receipts.
//! Adapted from Files entry/upload transfer owners; no transfer ledger is added.
use super::*;
use sailry_client::FileMove;
use sailry_protocol::{FileUploadSpec, FileWritten, Output, Request};

#[derive(Clone)]
pub(super) enum Source {
    Entry {
        access: Access,
        path: String,
        kind: sailry_protocol::EntryKind,
        cut: bool,
    },
    Upload(std::path::PathBuf),
    Download(std::path::PathBuf),
}

#[derive(Clone)]
pub(super) enum Prepared {
    Request(Box<Publication>),
    Move(Box<Move>),
}

#[derive(Clone)]
pub(super) struct Publication {
    request: Request,
    expected: Output,
}

#[derive(Clone)]
pub(super) struct Move {
    pending: FileMove,
    source: Access,
}

pub(super) struct Recovery {
    pub prepared: Option<Prepared>,
    pub retained: Arc<std::sync::Mutex<Option<Prepared>>>,
}

pub(super) struct Completion {
    pub prepared: Option<Prepared>,
    pub result: Result<(), Fault>,
    pub published: bool,
    pub existing: Option<String>,
}

pub(super) async fn run(
    target: Access,
    source: Source,
    path: String,
    expected: Option<String>,
    recovery: Recovery,
    cancel: CancellationToken,
    updates: tokio::sync::watch::Sender<Progress>,
) -> Completion {
    let Recovery { prepared, retained } = recovery;
    if let Some(prepared) = prepared {
        return finish(&target, prepared, &updates, &retained).await;
    }
    let destination = path.clone();
    let origin = match &source {
        Source::Entry { access, cut, .. } => Some((access.clone(), *cut)),
        _ => None,
    };
    let staged = async {
        target
            .verify(!matches!(source, Source::Download(_)))
            .await?;
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        match source {
            Source::Download(destination) => {
                let (sender, mut changes) =
                    tokio::sync::watch::channel(crate::downloads::transfer::Status::Preparing);
                let client = target.client.clone();
                let runtime = target.runtime.clone();
                let transfer = runtime.spawn(crate::downloads::transfer::run(
                    client,
                    crate::downloads::transfer::Source::ScopedFile {
                        context: target.context.clone(),
                        path,
                    },
                    destination,
                    cancel.clone(),
                    sender,
                ));
                let mut result = Err(internal());
                while changes.changed().await.is_ok() {
                    match changes.borrow_and_update().clone() {
                        crate::downloads::transfer::Status::Preparing => {}
                        crate::downloads::transfer::Status::Receiving { copied, size } => {
                            updates.send_replace(Progress::Copying(copied, size));
                        }
                        crate::downloads::transfer::Status::Publishing => {
                            updates.send_replace(Progress::Publishing);
                        }
                        crate::downloads::transfer::Status::Finished(value) => {
                            result = value.map_err(|key| {
                                Fault::new(
                                    if key == "files_download_cancelled" {
                                        ErrorCode::Cancelled
                                    } else {
                                        ErrorCode::Unavailable
                                    },
                                    key,
                                )
                            });
                        }
                    }
                }
                transfer.await.map_err(|_| internal())?;
                result?;
                Ok(None)
            }
            Source::Upload(source) => {
                let stopping = cancel.clone();
                let source = tokio::task::spawn_blocking(move || {
                    crate::resources::file_source::Source::open(&source, &stopping)
                })
                .await
                .map_err(|_| internal())?
                .map_err(|message| {
                    Fault::new(
                        if cancel.is_cancelled() {
                            ErrorCode::Cancelled
                        } else {
                            ErrorCode::Unavailable
                        },
                        message,
                    )
                })?;
                let spec = FileUploadSpec {
                    worktree: target.worktree(),
                    path,
                    size: source.size,
                    revision: source.revision,
                    expected_revision: expected,
                };
                let Output::FileUpload(upload) =
                    target.execute(Command::UploadFile(spec.clone())).await?
                else {
                    return Err(internal());
                };
                if upload.spec != spec {
                    release(&target.client, upload.stream).await;
                    return Err(internal());
                }
                let mut reader = tokio::fs::File::from_std(source.file);
                target
                    .client
                    .upload(&upload, &mut reader, cancel.clone(), |copied| {
                        updates.send_replace(Progress::Copying(copied, spec.size));
                    })
                    .await?;
                let request = target.request(Command::FinishFileUpload {
                    worktree: spec.worktree,
                    path: spec.path.clone(),
                    stream: upload.stream,
                });
                let output = Output::FileWritten(FileWritten {
                    path: spec.path,
                    revision: spec.revision,
                    size: spec.size,
                });
                Ok(Some(Prepared::Request(Box::new(Publication {
                    request,
                    expected: output,
                }))))
            }
            Source::Entry {
                access,
                path: from,
                kind,
                cut,
            } => {
                access.verify(cut).await?;
                if access.client.target() == target.client.target() {
                    // Both locations were selected through captured UI capabilities.
                    // A plugin request itself cannot widen its single worktree scope.
                    let (command, output) = if cut {
                        (
                            Command::MoveEntryTo {
                                source: access.worktree(),
                                worktree: target.worktree(),
                                from: from.clone(),
                                to: path.clone(),
                            },
                            Output::EntryMoved { from, to: path },
                        )
                    } else {
                        (
                            Command::CopyEntryTo {
                                source: access.worktree(),
                                worktree: target.worktree(),
                                from: from.clone(),
                                to: path.clone(),
                            },
                            Output::EntryCopied { from, to: path },
                        )
                    };
                    Ok(Some(Prepared::Request(Box::new(Publication {
                        request: target.client.prepare(command),
                        expected: output,
                    }))))
                } else {
                    if kind != sailry_protocol::EntryKind::File {
                        return Err(Fault::new(
                            ErrorCode::InvalidRequest,
                            "directories cannot be transferred between Nodes",
                        ));
                    }
                    let Output::FileDownload(download) = access
                        .execute(Command::DownloadFile {
                            worktree: access.worktree(),
                            path: from.clone(),
                        })
                        .await?
                    else {
                        return Err(internal());
                    };
                    if download.worktree != access.worktree() || download.path != from {
                        release(&access.client, download.stream).await;
                        return Err(internal());
                    }
                    let spec = FileUploadSpec {
                        worktree: target.worktree(),
                        path,
                        size: download.size,
                        revision: download.revision.clone(),
                        expected_revision: None,
                    };
                    let size = spec.size;
                    let progress = |copied| {
                        updates.send_replace(Progress::Copying(copied, size));
                    };
                    if cut {
                        target
                            .client
                            .stage_move(&access.client, &download, spec, cancel.clone(), progress)
                            .await
                            .map(|pending| {
                                Some(Prepared::Move(Box::new(Move {
                                    pending,
                                    source: access,
                                })))
                            })
                    } else {
                        let request = target
                            .client
                            .stage_copy(
                                &access.client,
                                &download,
                                spec.clone(),
                                cancel.clone(),
                                progress,
                            )
                            .await?;
                        Ok(Some(Prepared::Request(Box::new(Publication {
                            request,
                            expected: Output::FileWritten(FileWritten {
                                path: spec.path,
                                revision: spec.revision,
                                size: spec.size,
                            }),
                        }))))
                    }
                }
            }
        }
    }
    .await;
    match staged {
        Ok(None) => Completion {
            prepared: None,
            result: Ok(()),
            published: true,
            existing: None,
        },
        Ok(Some(prepared)) => {
            if cancel.is_cancelled() {
                let request = match &prepared {
                    Prepared::Request(publication) => &publication.request,
                    Prepared::Move(movement) => movement.pending.publication(),
                };
                if let Command::FinishFileUpload { stream, .. } = request.command {
                    release(&target.client, stream).await;
                }
                return Completion {
                    prepared: None,
                    result: Err(cancelled()),
                    published: false,
                    existing: None,
                };
            }
            // A permission change during staging must not start publication.
            let verified = async {
                target.verify(true).await?;
                if let Some((source, write)) = origin {
                    source.verify(write).await?;
                }
                Ok::<_, Fault>(())
            }
            .await;
            if let Err(error) = verified {
                release_prepared(&target, &prepared).await;
                return Completion {
                    prepared: None,
                    result: Err(error),
                    published: false,
                    existing: None,
                };
            }
            if cancel.is_cancelled() {
                release_prepared(&target, &prepared).await;
                return Completion {
                    prepared: None,
                    result: Err(cancelled()),
                    published: false,
                    existing: None,
                };
            }
            finish(&target, prepared, &updates, &retained).await
        }
        Err(error) => {
            let existing = if error.code == ErrorCode::RevisionConflict {
                revision(&target, &destination).await.ok()
            } else {
                None
            };
            Completion {
                prepared: None,
                result: Err(error),
                published: false,
                existing,
            }
        }
    }
}

async fn finish(
    target: &Access,
    prepared: Prepared,
    updates: &tokio::sync::watch::Sender<Progress>,
    retained: &std::sync::Mutex<Option<Prepared>>,
) -> Completion {
    if let Ok(mut retained) = retained.lock() {
        *retained = Some(prepared.clone());
    }
    updates.send_replace(Progress::Publishing);
    let mut published = false;
    let result = match &prepared {
        Prepared::Request(publication) => target
            .client
            .execute(publication.request.clone())
            .await
            .and_then(|output| {
                if output == publication.expected {
                    Ok(())
                } else {
                    Err(internal())
                }
            }),
        Prepared::Move(movement) => target
            .client
            .finish_move(&movement.source.client, &movement.pending, |_| {
                published = true;
                updates.send_replace(Progress::Recycling);
            })
            .await
            .map(|_| ()),
    };
    Completion {
        prepared: Some(prepared),
        published: published || result.is_ok(),
        result,
        existing: None,
    }
}

async fn release_prepared(target: &Access, prepared: &Prepared) {
    let request = match prepared {
        Prepared::Request(publication) => &publication.request,
        Prepared::Move(movement) => movement.pending.publication(),
    };
    if let Command::FinishFileUpload { stream, .. } = request.command {
        release(&target.client, stream).await;
    }
}

async fn revision(target: &Access, path: &str) -> Result<String, Fault> {
    let Output::FileDownload(download) = target
        .execute(Command::DownloadFile {
            worktree: target.worktree(),
            path: path.into(),
        })
        .await?
    else {
        return Err(internal());
    };
    release(&target.client, download.stream).await;
    if download.worktree != target.worktree() || download.path != path {
        return Err(internal());
    }
    Ok(download.revision)
}

pub(super) async fn release(client: &sailry_client::Client, stream: sailry_protocol::StreamId) {
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.execute(client.prepare(Command::CancelFileTransfer { stream })),
    )
    .await;
}
