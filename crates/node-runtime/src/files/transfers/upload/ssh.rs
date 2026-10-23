//! SSH staging shares the authenticated, single-use file stream registry.
use super::*;
use sailry_protocol::ssh::{Upload, UploadSpec};

pub(crate) struct Source {
    pub file: std::fs::File,
    pub spec: UploadSpec,
    _prepared: Box<Prepared>,
    _lease: Lease,
}

impl Transfers {
    pub(crate) fn staging_file(&self) -> Result<std::fs::File, Fault> {
        temporary_file(self.profile.as_deref())
    }

    pub(crate) fn upload_ssh(
        self: &Arc<Self>,
        caller: NodeId,
        mut spec: UploadSpec,
        closed: CancellationToken,
    ) -> Result<Upload, Fault> {
        crate::ssh::files::validate_path(&spec.path)?;
        if spec.size > crate::files::ssh::LIMIT {
            return Err(Fault::invalid("SSH file exceeds the transfer limit"));
        }
        spec.revision = revision(&spec.revision)?;
        let (mut lease, cancel) = self.reserve(caller, PathBuf::new(), closed)?;
        let prepared = Prepared {
            file: self.staging_file()?,
            size: spec.size,
            revision: spec.revision.clone(),
            target: None,
            scope: Scope::Ssh(spec.clone()),
        };
        let output = Upload {
            stream: lease.stream,
            spec,
        };
        self.ready(&mut lease, Resource::Upload(Box::new(prepared)), cancel)?;
        Ok(output)
    }

    pub(crate) fn take_ssh(
        self: &Arc<Self>,
        caller: NodeId,
        profile: &sailry_protocol::ssh::Profile,
        path: &str,
        stream: StreamId,
    ) -> Result<Source, Fault> {
        let (prepared, lease) = self.take(caller, Path::new(""), stream, |scope| {
            matches!(scope, Scope::Ssh(spec) if spec.profile == profile.id && spec.expected_revision == profile.revision && spec.path == path)
        })?;
        let Scope::Ssh(spec) = &prepared.scope else {
            unreachable!()
        };
        Ok(Source {
            file: prepared.file.try_clone().map_err(io_error)?,
            spec: spec.clone(),
            _prepared: prepared,
            _lease: lease,
        })
    }

    pub(crate) fn download_ssh(
        self: &Arc<Self>,
        caller: NodeId,
        path: String,
        file: std::fs::File,
        size: u64,
        revision: String,
        closed: CancellationToken,
    ) -> Result<sailry_protocol::ssh::Download, Fault> {
        let (mut lease, cancel) = self.reserve(caller, PathBuf::new(), closed)?;
        let output = sailry_protocol::ssh::Download {
            stream: lease.stream,
            path,
            size,
            revision: revision.clone(),
        };
        self.ready(
            &mut lease,
            Resource::Download(super::super::Prepared {
                _staging: None,
                source: super::super::Source::File(file),
                size,
                revision,
                stamp: String::new(),
            }),
            cancel,
        )?;
        Ok(output)
    }
}
