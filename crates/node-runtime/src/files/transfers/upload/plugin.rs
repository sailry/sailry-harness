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
            return Err(Fault::invalid("plugin ZIP exceeds the upload limit"));
        }
        spec.revision = revision(&spec.revision)?;
        let (mut lease, cancel) = self.reserve(caller, PathBuf::new(), closed)?;
        let file = self.staging_file()?;
        let prepared = Prepared {
            file,
            size: spec.size,
            revision: spec.revision.clone(),
            target: None,
            scope: Scope::Plugin,
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
        self.inspect_upload(caller, stream, None, closed, move |file, _, stop| {
            host.inspect_archive(file, stop)
        })
        .await
    }

    pub(crate) async fn discover_skill_upload(
        self: &Arc<Self>,
        caller: NodeId,
        stream: StreamId,
        format: sailry_protocol::plugin::skills::ArchiveFormat,
        kind: sailry_protocol::plugin::UploadSource,
        host: crate::plugins::Host,
        closed: CancellationToken,
    ) -> Result<sailry_protocol::plugin::skills::ImportDiscovery, Fault> {
        if kind == sailry_protocol::plugin::UploadSource::Directory
            && format != sailry_protocol::plugin::skills::ArchiveFormat::Zip
        {
            return Err(Fault::invalid("directory snapshots use ZIP"));
        }
        self.inspect_upload(
            caller,
            stream,
            Some((format, kind)),
            closed,
            move |file, digest, stop| {
                host.discover_skill_import(file, stream, digest, format, kind, &stop)
            },
        )
        .await
    }

    async fn inspect_upload<T: Send + 'static>(
        self: &Arc<Self>,
        caller: NodeId,
        stream: StreamId,
        skill: Option<(
            sailry_protocol::plugin::skills::ArchiveFormat,
            sailry_protocol::plugin::UploadSource,
        )>,
        closed: CancellationToken,
        inspect: impl FnOnce(std::fs::File, String, CancellationToken) -> Result<T, Fault>
        + Send
        + 'static,
    ) -> Result<T, Fault> {
        let (mut prepared, mut lease) =
            self.take(caller, Path::new(""), stream, |scope| {
                match (scope, skill) {
                    (Scope::Plugin, _) => true,
                    (Scope::Skill { format, kind }, Some((expected_format, expected_kind))) => {
                        *format == expected_format && *kind == expected_kind
                    }
                    _ => false,
                }
            })?;
        if let Some((format, kind)) = skill {
            prepared.scope = Scope::Skill { format, kind };
        }
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
        let digest = prepared.revision.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let result = inspect(file, digest, stop);
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

    pub(crate) async fn discover_local_skills(
        self: &Arc<Self>,
        caller: NodeId,
        root: PathBuf,
        path: String,
        host: crate::plugins::Host,
        closed: CancellationToken,
    ) -> Result<sailry_protocol::plugin::skills::ImportDiscovery, Fault> {
        let (mut lease, cancel) = self.reserve(caller, PathBuf::new(), closed.clone())?;
        let stopping = cancel.clone();
        let file = self.staging_file()?;
        let captured = tokio::task::spawn_blocking(move || {
            let (mut file, format, kind) =
                crate::plugins::Host::snapshot_skills(&root, &path, file, &stopping)?;
            let digest = crate::plugins::Host::skill_import_digest(&mut file, &stopping)?;
            let size = file.metadata().map_err(io_error)?.len();
            if size == 0 || size > MAX_PACKAGE_BYTES {
                return Err(Fault::invalid("skill import exceeds its size limit"));
            }
            Ok::<_, Fault>((
                Box::new(Prepared {
                    file,
                    size,
                    revision: digest,
                    target: None,
                    scope: Scope::Skill { format, kind },
                }),
                format,
                kind,
            ))
        });
        let (prepared, format, kind) = tokio::select! {
            _ = cancel.cancelled() => return Err(cancelled()),
            result = captured => result.map_err(|_| unavailable())??,
        };
        self.entries
            .lock()
            .unwrap()
            .entries
            .get_mut(&lease.stream)
            .ok_or_else(missing)?
            .state = State::Uploaded(prepared);
        self.schedule_expiry(lease.stream, cancel);
        let stream = lease.stream;
        lease.armed = false;
        self.discover_skill_upload(caller, stream, format, kind, host, closed)
            .await
    }

    pub(crate) fn install_skill(
        self: &Arc<Self>,
        caller: NodeId,
        stream: StreamId,
        source: &sailry_protocol::plugin::skills::Imported,
        name: &str,
        host: &crate::plugins::Host,
    ) -> Result<Info, Fault> {
        let (prepared, _lease) = self.take(
            caller,
            Path::new(""),
            stream,
            |scope| matches!(scope,Scope::Skill { kind,.. } if *kind == source.kind),
        )?;
        if prepared.revision != source.digest {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "skill import does not match its captured bytes",
            ));
        }
        let Scope::Skill { format, .. } = prepared.scope else {
            unreachable!("skill upload scope checked");
        };
        host.install_skill_import(
            prepared.file,
            format,
            source.clone(),
            name,
            &CancellationToken::new(),
        )
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
