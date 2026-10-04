//! Staging semantics adapted from sailry-platform 727ce0a:
//! harbor-file-service/src/worktree.rs write_transfer_chunk (Apache-2.0).
//! The authenticated stream stages bytes; a durable command publishes them.
use super::*;
use crate::files::{io_error, path, save};
use sailry_protocol::{FileUpload, FileUploadSpec, FileWritten, attachment};
use std::path::Path;

mod io;
mod plugin;
mod ssh;
pub(super) use io::{acknowledge, receive};

pub(super) struct Prepared {
    file: std::fs::File,
    size: u64,
    revision: String,
    target: Option<save::Target>,
    scope: Scope,
    _permit: OwnedSemaphorePermit,
}

enum Scope {
    Ssh(sailry_protocol::ssh::UploadSpec),
    Plugin,
    File { worktree: WorktreeId, path: String },
    Attachment(sailry_protocol::attachment::Attachment),
}

impl Transfers {
    pub(crate) async fn upload_attachment(
        self: &Arc<Self>,
        caller: NodeId,
        root: PathBuf,
        mut spec: attachment::Spec,
        closed: CancellationToken,
    ) -> Result<attachment::Upload, Fault> {
        crate::files::attachments::validate(&spec)?;
        spec.revision = revision(&spec.revision)?;
        let (mut lease, permit, cancel) = self.reserve(caller, root, closed)?;
        let profile = self.profile.clone();
        let upload = spec.clone();
        let stopping = cancel.clone();
        let worker = tokio::task::spawn_blocking(move || {
            if stopping.is_cancelled() {
                return Err(cancelled());
            }
            let staging = path::root(profile.as_deref().ok_or_else(unavailable)?)?;
            let file = cap_tempfile::TempFile::new_anonymous(&staging)
                .map(cap_std::fs::File::into_std)
                .map_err(io_error)?;
            let (attachment, target) =
                crate::files::attachments::prepare(profile.as_deref(), upload)?;
            if stopping.is_cancelled() {
                return Err(cancelled());
            }
            Ok(Prepared {
                file,
                size: attachment.spec.size,
                revision: attachment.spec.revision.clone(),
                target: Some(target),
                scope: Scope::Attachment(attachment),
                _permit: permit,
            })
        });
        let prepared = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = tokio::time::timeout(PREPARE_TIMEOUT, worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "attachment preparation timed out"))?
                .map_err(|_| unavailable())??,
        };
        let output = attachment::Upload {
            stream: lease.stream,
            spec,
        };
        self.ready(&mut lease, Resource::Upload(Box::new(prepared)), cancel)?;
        Ok(output)
    }

    pub(crate) async fn upload(
        self: &Arc<Self>,
        caller: NodeId,
        root: PathBuf,
        mut spec: FileUploadSpec,
        closed: CancellationToken,
    ) -> Result<FileUpload, Fault> {
        spec.revision = revision(&spec.revision)?;
        spec.expected_revision = spec
            .expected_revision
            .as_deref()
            .map(revision)
            .transpose()?;
        path::entry_components(&spec.path)?;
        let (mut lease, permit, cancel) = self.reserve(caller, root.clone(), closed)?;
        let profile = self.profile.clone();
        let upload = spec.clone();
        let stopping = cancel.clone();
        let worker = tokio::task::spawn_blocking(move || {
            prepare(&root, profile.as_deref(), upload, &stopping, permit)
        });
        let prepared = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = tokio::time::timeout(PREPARE_TIMEOUT, worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "upload preparation timed out"))?
                .map_err(|_| unavailable())??,
        };
        let output = FileUpload {
            stream: lease.stream,
            spec,
        };
        self.ready(&mut lease, Resource::Upload(Box::new(prepared)), cancel)?;
        Ok(output)
    }

    pub(super) fn uploaded(
        self: &Arc<Self>,
        stream: StreamId,
        prepared: Box<Prepared>,
        cancel: CancellationToken,
    ) -> Result<(), Fault> {
        {
            let mut registry = self.entries.lock().unwrap();
            let entry = registry.entries.get_mut(&stream).ok_or_else(cancelled)?;
            if !matches!(entry.state, State::Active) {
                return Err(missing());
            }
            entry.state = State::Uploaded(prepared);
            entry.expires = Instant::now() + OPEN_TIMEOUT;
        }
        self.schedule_expiry(stream, cancel);
        Ok(())
    }

    pub(crate) fn commit(
        self: &Arc<Self>,
        caller: NodeId,
        root: &Path,
        worktree: WorktreeId,
        path: &str,
        stream: StreamId,
    ) -> Result<FileWritten, Fault> {
        let (prepared, _lease) = self.take(caller, root, stream, |scope| {
            matches!(scope, Scope::File { worktree: expected, path: relative }
                if *expected == worktree && relative == path)
        })?;
        prepared.target.expect("file upload target").write(
            prepared.file,
            prepared.size,
            &prepared.revision,
            #[cfg(test)]
            &|_| {},
        )
    }

    pub(crate) fn commit_attachment(
        self: &Arc<Self>,
        caller: NodeId,
        root: &Path,
        worktree: WorktreeId,
        stream: StreamId,
    ) -> Result<sailry_protocol::attachment::Attachment, Fault> {
        let (prepared, _lease) = self.take(caller, root, stream, |scope| {
            matches!(scope, Scope::Attachment(attachment) if attachment.spec.worktree == worktree)
        })?;
        let Scope::Attachment(attachment) = prepared.scope else {
            unreachable!()
        };
        prepared.target.expect("attachment upload target").write(
            prepared.file,
            prepared.size,
            &prepared.revision,
            #[cfg(test)]
            &|_| {},
        )?;
        Ok(attachment)
    }

    fn take(
        self: &Arc<Self>,
        caller: NodeId,
        root: &Path,
        stream: StreamId,
        accepts: impl FnOnce(&Scope) -> bool,
    ) -> Result<(Box<Prepared>, Lease), Fault> {
        let prepared = {
            let mut registry = self.entries.lock().unwrap();
            let entry = registry
                .entries
                .get_mut(&stream)
                .filter(|entry| entry.caller == caller)
                .ok_or_else(missing)?;
            if entry.expires <= Instant::now() && matches!(entry.state, State::Uploaded(_)) {
                entry.cancel.cancel();
                registry.entries.remove(&stream);
                return Err(missing());
            }
            let State::Uploaded(prepared) = &entry.state else {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "upload is not ready for publication",
                ));
            };
            if entry.root != root || !accepts(&prepared.scope) {
                return Err(missing());
            }
            let State::Uploaded(prepared) = std::mem::replace(&mut entry.state, State::Finishing)
            else {
                unreachable!()
            };
            prepared
        };
        let lease = Lease {
            transfers: self.clone(),
            stream,
            armed: true,
        };
        // Once publication starts, its durable result is the authority. Cancellation
        // cannot undo a write; the ordinary mutation shutdown drain waits for it.
        Ok((prepared, lease))
    }
}

fn prepare(
    root: &Path,
    profile: Option<&Path>,
    spec: FileUploadSpec,
    cancel: &CancellationToken,
    permit: OwnedSemaphorePermit,
) -> Result<Prepared, Fault> {
    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    let absolute = root.join(&spec.path);
    let parent = absolute
        .parent()
        .unwrap()
        .canonicalize()
        .map_err(io_error)?;
    if profile.is_some_and(|profile| parent.starts_with(profile)) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "Node storage is not an upload destination",
        ));
    }
    let target = save::Target::prepare(
        root,
        &spec.path,
        spec.expected_revision.as_deref(),
        u64::MAX,
        Some(cancel),
    )?;
    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    // Windows anonymous files retain a delete-on-close name. Keep staging in
    // protected Node storage, outside the visible project tree on every platform.
    let staging = path::root(profile.unwrap_or(root))?;
    let file = cap_tempfile::TempFile::new_anonymous(&staging)
        .map(cap_std::fs::File::into_std)
        .map_err(io_error)?;
    Ok(Prepared {
        file,
        size: spec.size,
        revision: spec.revision,
        scope: Scope::File {
            worktree: spec.worktree,
            path: spec.path,
        },
        target: Some(target),
        _permit: permit,
    })
}

fn revision(value: &str) -> Result<String, Fault> {
    blake3::Hash::from_hex(value)
        .map(|hash| hash.to_hex().to_string())
        .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "invalid upload content revision"))
}
