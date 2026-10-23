use super::*;

/// The original transfer lease remains held through restore validation.
pub(crate) struct BackupFile {
    pub file: std::fs::File,
    pub size: u64,
    _lease: Lease,
}

impl Transfers {
    pub(crate) fn upload_backup(
        self: &Arc<Self>,
        caller: NodeId,
        size: u64,
        digest: &str,
        closed: CancellationToken,
    ) -> Result<sailry_protocol::backup::Upload, Fault> {
        if size == 0 || size > crate::backup::MAX_BYTES {
            return Err(Fault::invalid("backup exceeds the upload limit"));
        }
        let revision = revision(digest)?;
        let (mut lease, cancel) = self.reserve(caller, PathBuf::new(), closed)?;
        let prepared = Prepared {
            file: self.staging_file()?,
            size,
            revision: revision.clone(),
            target: None,
            scope: Scope::Backup,
        };
        let output = sailry_protocol::backup::Upload {
            stream: lease.stream,
            size,
            revision,
        };
        self.ready(&mut lease, Resource::Upload(Box::new(prepared)), cancel)?;
        Ok(output)
    }

    pub(crate) fn claim_backup(
        self: &Arc<Self>,
        caller: NodeId,
        stream: StreamId,
    ) -> Result<BackupFile, Fault> {
        let (prepared, lease) = self.take(caller, Path::new(""), stream, |scope| {
            matches!(scope, Scope::Backup)
        })?;
        let Prepared { file, size, .. } = *prepared;
        Ok(BackupFile {
            file,
            size,
            _lease: lease,
        })
    }
}
