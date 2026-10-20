//! Only the controller-facing backup service can register private archive files.
use super::*;

impl Transfers {
    pub(crate) fn download_backup(
        self: &Arc<Self>,
        caller: NodeId,
        file: tempfile::NamedTempFile,
        directory: tempfile::TempDir,
        record: sailry_protocol::backup::Record,
        revision: String,
        closed: CancellationToken,
    ) -> Result<sailry_protocol::backup::Download, Fault> {
        let (mut lease, permit, cancel) = self.reserve(caller, PathBuf::new(), closed)?;
        let prepared = Prepared {
            source: Source::File(file.reopen().map_err(crate::files::io_error)?),
            size: record.size_bytes,
            revision: revision.clone(),
            stamp: String::new(),
            _permit: permit,
            _staging: Some((file, directory)),
        };
        let output = sailry_protocol::backup::Download {
            stream: lease.stream,
            size: record.size_bytes,
            revision,
            record,
        };
        self.ready(&mut lease, Resource::Download(prepared), cancel)?;
        Ok(output)
    }
}
