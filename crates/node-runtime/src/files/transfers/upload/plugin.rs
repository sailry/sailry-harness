use super::*;
use sailry_protocol::plugin::{Info, MAX_PACKAGE_BYTES, Upload, UploadSpec};

impl Transfers {
    pub(crate) async fn upload_plugin(
        self: &Arc<Self>,
        caller: NodeId,
        mut spec: UploadSpec,
        closed: CancellationToken,
    ) -> Result<Upload, Fault> {
        if spec.size == 0 || spec.size > MAX_PACKAGE_BYTES {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "plugin ZIP exceeds the upload limit",
            ));
        }
        spec.revision = revision(&spec.revision)?;
        let (mut lease, permit, cancel) = self.reserve(caller, PathBuf::new(), closed)?;
        let staging = path::root(self.profile.as_deref().ok_or_else(unavailable)?)?;
        let file = cap_tempfile::TempFile::new_anonymous(&staging)
            .map(cap_std::fs::File::into_std)
            .map_err(io_error)?;
        let prepared = Prepared {
            file,
            size: spec.size,
            revision: spec.revision.clone(),
            target: None,
            scope: Scope::Plugin,
            _permit: permit,
        };
        let output = Upload {
            stream: lease.stream,
            spec,
        };
        self.ready(&mut lease, Resource::Upload(Box::new(prepared)), cancel)?;
        Ok(output)
    }

    pub(crate) async fn inspect_plugin(
        self: &Arc<Self>,
        caller: NodeId,
        stream: StreamId,
        host: crate::plugins::Host,
        closed: CancellationToken,
    ) -> Result<Info, Fault> {
        let (prepared, mut lease) = self.take(caller, Path::new(""), stream, |scope| {
            matches!(scope, Scope::Plugin)
        })?;
        self.entries
            .lock()
            .unwrap()
            .entries
            .get_mut(&stream)
            .ok_or_else(missing)?
            .state = State::Inspecting;
        let file = prepared.file.try_clone().map_err(io_error)?;
        let cancel = self
            .entries
            .lock()
            .unwrap()
            .entries
            .get(&stream)
            .ok_or_else(missing)?
            .cancel
            .clone();
        let stop = cancel.clone();
        let worker = tokio::task::spawn_blocking(move || {
            // Keep the upload capacity reserved until all blocking work stops.
            let result = host.inspect_archive(file, stop);
            (prepared, result)
        });
        let (prepared, result) = tokio::select! {
            _ = closed.cancelled() => { cancel.cancel(); return Err(cancelled()); },
            _ = cancel.cancelled() => return Err(cancelled()),
            result = worker => result.map_err(|_| unavailable())?,
        };
        let info = result?;
        self.uploaded_after_inspection(stream, prepared)?;
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(600)).await;
            if let Some(transfers) = weak.upgrade() {
                transfers.expire(stream);
            }
        });
        lease.armed = false;
        Ok(info)
    }

    fn uploaded_after_inspection(
        &self,
        stream: StreamId,
        prepared: Box<Prepared>,
    ) -> Result<(), Fault> {
        let mut registry = self.entries.lock().unwrap();
        let entry = registry.entries.get_mut(&stream).ok_or_else(missing)?;
        if entry.cancel.is_cancelled() || !matches!(entry.state, State::Inspecting) {
            return Err(cancelled());
        }
        entry.state = State::Uploaded(prepared);
        entry.expires = Instant::now() + Duration::from_secs(600);
        Ok(())
    }

    pub(crate) fn install_plugin(
        self: &Arc<Self>,
        caller: NodeId,
        stream: StreamId,
        name: &str,
        host: &crate::plugins::Host,
    ) -> Result<Info, Fault> {
        let (prepared, _lease) = self.take(caller, Path::new(""), stream, |scope| {
            matches!(scope, Scope::Plugin)
        })?;
        host.install_archive(prepared.file, name)
    }
}
