use super::*;
use sailry_protocol::{FileUploadSpec, FileWritten, Output, Request, RequestOutcome};

/// Two stable Node-owned admissions, not a distributed transaction or a transfer ledger.
/// Retain this value before finishing. A partial failure never rolls back the destination.
#[derive(Clone, Debug)]
pub struct FileMove {
    publication: Request,
    recycle: Request,
    spec: FileUploadSpec,
}

impl FileMove {
    pub fn publication(&self) -> &Request {
        &self.publication
    }

    pub fn recycle(&self) -> &Request {
        &self.recycle
    }

    fn written(&self) -> FileWritten {
        FileWritten {
            path: self.spec.path.clone(),
            revision: self.spec.revision.clone(),
            size: self.spec.size,
        }
    }
}

impl Client {
    /// Prepares a cross-Node move without publishing or recycling anything.
    /// Ordinary same-Node moves should use MoveEntryTo and its single admission.
    pub async fn stage_move(
        &self,
        source: &Client,
        download: &FileDownload,
        spec: FileUploadSpec,
        cancel: CancellationToken,
        progress: impl FnMut(u64),
    ) -> Result<FileMove, Fault> {
        let error = if source.target() == self.target() {
            Some(Fault::new(
                ErrorCode::InvalidRequest,
                "cross-Node move requires distinct Nodes",
            ))
        } else if blake3::Hash::from_hex(&download.stamp).is_err() {
            Some(Fault::new(
                ErrorCode::InvalidRequest,
                "file identity revision is invalid",
            ))
        } else if spec.expected_revision.is_some() {
            Some(Fault::new(
                ErrorCode::InvalidRequest,
                "cross-Node move cannot replace an existing destination",
            ))
        } else {
            None
        };
        if let Some(error) = error {
            source.cancel_transfer(download.stream).await;
            return Err(error);
        }
        let recycle = source.prepare(Command::TrashFile {
            worktree: download.worktree,
            path: download.path.clone(),
            expected_revision: download.revision.clone(),
            expected_stamp: download.stamp.clone(),
        });
        let publication = self
            .stage_copy(source, download, spec.clone(), cancel, progress)
            .await?;
        Ok(FileMove {
            publication,
            recycle,
            spec,
        })
    }

    /// Finishes or explicitly resumes a prepared move using the original two request IDs.
    /// The callback reports confirmed publication; later failure can leave both copies.
    /// No cancellation or transport retry automatically replays an uncertain side effect.
    pub async fn finish_move(
        &self,
        source: &Client,
        pending: &FileMove,
        mut published: impl FnMut(&FileWritten),
    ) -> Result<FileWritten, Fault> {
        if pending.publication.target != self.target() || pending.recycle.target != source.target()
        {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "move belongs to different Nodes",
            ));
        }
        let expected = pending.written();
        // Query before touching the destination: a completed recycle can be recovered
        // even if that destination has since been edited, removed, or gone offline.
        match source.outcome(&pending.recycle).await? {
            RequestOutcome::Completed(result) => {
                recycled(&pending.recycle, *result)?;
                published(&expected);
                return Ok(expected);
            }
            RequestOutcome::Admitted | RequestOutcome::Unknown => {
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "source recycle outcome requires reconciliation; automatic replay is disabled",
                ));
            }
            RequestOutcome::NotAdmitted => {}
        }
        let output = self.execute(pending.publication.clone()).await?;
        if output != Output::FileWritten(expected.clone()) {
            return Err(invalid_response());
        }
        published(&expected);
        // A replayed publication result proves history, not current destination contents.
        // Re-read the full revision before the first admission of the source recycle.
        let output = self
            .execute(self.prepare(Command::DownloadFile {
                worktree: pending.spec.worktree,
                path: pending.spec.path.clone(),
            }))
            .await?;
        let Output::FileDownload(download) = output else {
            return Err(invalid_response());
        };
        self.cancel_transfer(download.stream).await;
        if download.worktree != pending.spec.worktree || download.path != expected.path {
            return Err(invalid_response());
        }
        if download.size != expected.size || download.revision != expected.revision {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "published destination changed; source was not recycled",
            ));
        }
        recycled(
            &pending.recycle,
            source.execute(pending.recycle.clone()).await,
        )?;
        Ok(expected)
    }
}

fn recycled(request: &Request, result: Result<Output, Fault>) -> Result<(), Fault> {
    let Command::TrashFile { path: expected, .. } = &request.command else {
        return Err(invalid_response());
    };
    match result? {
        Output::EntryTrashed { path } if path == *expected => Ok(()),
        _ => Err(invalid_response()),
    }
}

fn invalid_response() -> Fault {
    Fault::new(ErrorCode::Internal, "invalid file move response")
}
