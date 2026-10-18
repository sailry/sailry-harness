//! Controller files cross the existing caller-scoped upload boundary.
use super::*;
use sailry_protocol::{
    StreamId,
    plugin::{
        self, UploadSource,
        skills::{ArchiveFormat, Candidate, ImportDiscovery, Imported},
    },
};
use std::{path::PathBuf, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

pub(super) struct Snapshot {
    file: tokio::sync::Mutex<tokio::fs::File>,
    format: ArchiveFormat,
    kind: UploadSource,
    size: u64,
    digest: String,
    pub path: PathBuf,
}

/// Archive extensions accepted by `archive`, for the path picker filter.
pub(super) const ARCHIVES: &[&str] = &["zip", "skill", "tar", "tar.gz", "tgz"];

pub(super) struct Lease {
    binding: Binding,
    stream: StreamId,
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

fn invalid() -> Fault {
    Fault::invalid("invalid skill directory or archive")
}

fn archive(path: &std::path::Path) -> Option<ArchiveFormat> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some(ArchiveFormat::TarGz)
    } else if name.ends_with(".tar") {
        Some(ArchiveFormat::Tar)
    } else if name.ends_with(".zip") || name.ends_with(".skill") {
        Some(ArchiveFormat::Zip)
    } else {
        None
    }
}

pub(super) async fn open(path: PathBuf, directory: bool) -> Result<Arc<Snapshot>, Fault> {
    let format = if directory {
        ArchiveFormat::Zip
    } else {
        archive(&path).ok_or_else(invalid)?
    };
    let selected = path.clone();
    let opened = tokio::task::spawn_blocking(move || {
        crate::settings::upload::open_skill(path, directory, sailry_link::CancellationToken::new())
    })
    .await
    .map_err(|_| invalid())??;
    let mut file = tokio::fs::File::from_std(opened.file);
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
    Ok(Arc::new(Snapshot {
        file: tokio::sync::Mutex::new(file),
        format,
        kind: if directory {
            UploadSource::Directory
        } else {
            UploadSource::Archive
        },
        size,
        digest: hash.finalize().to_hex().to_string(),
        path: selected,
    }))
}

async fn upload(binding: &Binding, snapshot: &Snapshot) -> Result<(ImportDiscovery, Lease), Fault> {
    let Output::PluginUpload(upload) = binding
        .client
        .execute(
            binding
                .client
                .prepare(Command::UploadPlugin(plugin::UploadSpec {
                    size: snapshot.size,
                    revision: snapshot.digest.clone(),
                })),
        )
        .await?
    else {
        return Err(invalid());
    };
    let lease = Lease {
        binding: binding.clone(),
        stream: upload.stream,
    };
    let mut file = snapshot.file.lock().await;
    file.rewind().await.map_err(|_| invalid())?;
    binding
        .client
        .upload(
            &upload,
            &mut *file,
            sailry_link::CancellationToken::new(),
            |_| {},
        )
        .await?;
    let Output::SkillImportDiscovery(discovery) = binding
        .client
        .execute(binding.client.prepare(Command::DiscoverSkillUpload {
            stream: upload.stream,
            format: snapshot.format,
            source: snapshot.kind,
        }))
        .await?
    else {
        return Err(invalid());
    };
    if discovery.stream != upload.stream
        || discovery.digest != snapshot.digest
        || discovery.kind != snapshot.kind
    {
        return Err(invalid());
    }
    Ok((discovery, lease))
}

pub(super) async fn discover(
    binding: &Binding,
    snapshot: &Snapshot,
) -> Result<ImportDiscovery, Fault> {
    // Discovery does not consume the retained local snapshot. Installation gets
    // its own single-use stream, including when several skills are selected.
    let (discovery, _lease) = upload(binding, snapshot).await?;
    Ok(discovery)
}

pub(super) async fn request(
    binding: &Binding,
    snapshot: &Snapshot,
    candidate: &Candidate,
    expected_revision: u64,
) -> Result<(Request, Lease), Fault> {
    let (discovery, lease) = upload(binding, snapshot).await?;
    Ok((
        binding
            .client
            .prepare(command(discovery, candidate, expected_revision)?),
        lease,
    ))
}

fn command(
    discovery: ImportDiscovery,
    candidate: &Candidate,
    expected_revision: u64,
) -> Result<Command, Fault> {
    if discovery
        .skills
        .iter()
        .filter(|entry| *entry == candidate)
        .count()
        != 1
    {
        return Err(invalid());
    }
    Ok(Command::InstallSkillUpload {
        stream: discovery.stream,
        source: Imported {
            digest: discovery.digest,
            path: candidate.path.clone(),
            kind: discovery.kind,
        },
        name: candidate.name.clone(),
        expected_revision,
    })
}

impl Editor {
    pub(super) fn discover_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self
            .channel
            .local()
            .and_then(|index| self.imports[index].clone())
        else {
            self.choose_import(window, cx);
            return;
        };
        let generation = self.generation;
        self.pending = true;
        self.error = None;
        self.discovery = None;
        self.selected.clear();
        self.completed.clear();
        let binding = self.binding.clone();
        let job = binding.runtime.clone().spawn(async move {
            let discovery = discover(&binding, &snapshot).await?;
            let Output::Plugins(known) = binding
                .client
                .execute(binding.client.prepare(Command::ListPlugins))
                .await?
            else {
                return Err(invalid());
            };
            Ok::<_, Fault>((discovery, known))
        });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, _, cx| {
                if editor.closed || editor.generation != generation {
                    return;
                }
                editor.pending = false;
                match result {
                    Ok(Ok((discovery, known))) => {
                        editor.accept(Discovery::Imported(discovery), known)
                    }
                    Ok(Err(error)) => editor.error = Some(error_key(&error)),
                    _ => editor.error = Some("skills_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn prepare_install(
        &mut self,
        index: usize,
        candidate: Candidate,
        expected_revision: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(snapshot) = self
            .channel
            .local()
            .and_then(|index| self.imports[index].clone())
        else {
            return;
        };
        self.pending = true;
        self.error = None;
        let binding = self.binding.clone();
        let job = binding.runtime.clone().spawn(async move {
            request(&binding, &snapshot, &candidate, expected_revision).await
        });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                editor.pending = false;
                if editor.closed {
                    return;
                }
                match result {
                    Ok(Ok((request, lease))) => {
                        editor.lease = Some(lease);
                        editor.request = Some((index, request));
                        editor.install(window, cx);
                    }
                    Ok(Err(error)) => editor.error = Some(error_key(&error)),
                    _ => editor.error = Some("skills_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn choose_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.channel.local() else {
            return;
        };
        if self.closed || self.pending || self.request.is_some() {
            return;
        }
        let generation = self.generation;
        let directory = index == 0;
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: !directory,
            directories: directory,
            multiple: false,
            prompt: Some(tr(if directory {
                "skill_source_choose_directory"
            } else {
                "skill_source_choose_archive"
            })),
        });
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let selected = selected.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                if editor.generation != generation {
                    return;
                }
                match selected {
                    Ok(Ok(Some(mut paths))) if paths.len() == 1 => {
                        editor.pick_import(index, paths.remove(0), window, cx)
                    }
                    Ok(Ok(None)) => {}
                    _ => {
                        editor.error = Some("files_upload_picker");
                        cx.notify();
                    }
                }
            });
        }));
    }

    /// Accepts a chosen or dropped path for the local source at `index`.
    /// The picker has already matched the path to that source's kind.
    pub(super) fn pick_import(
        &mut self,
        index: usize,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closed
            || self.pending
            || self.request.is_some()
            || self.channel.local() != Some(index)
        {
            return;
        }
        self.prepare_import(path, index == 0, window, cx);
    }

    pub(super) fn clear_import(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.closed
            || self.pending
            || self.request.is_some()
            || self.channel.local() != Some(index)
        {
            return;
        }
        self.invalidate();
        self.imports[index] = None;
        cx.notify();
    }

    /// The selected host name when a local import uploads to another Node.
    pub(super) fn remote_host(&self, cx: &App) -> Option<SharedString> {
        let services = cx.try_global::<crate::backend::Services>()?;
        let local = sailry_link::Transport::target(services.local.as_ref());
        (self.binding.client.target() != local).then(|| self.binding.label.clone())
    }

    fn prepare_import(
        &mut self,
        path: PathBuf,
        directory: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.invalidate();
        let generation = self.generation;
        self.pending = true;
        let runtime = self.binding.runtime.clone();
        let job = runtime.spawn(open(path, directory));
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                if editor.closed || editor.generation != generation {
                    return;
                }
                editor.pending = false;
                match result {
                    Ok(Ok(snapshot)) => {
                        editor.imports[usize::from(!directory)] = Some(snapshot);
                        editor.discover(window, cx);
                    }
                    Ok(Err(error)) => editor.error = Some(error_key(&error)),
                    _ => editor.error = Some("skills_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn recognizes_supported_archives() {
        for (path, expected) in [
            ("one.zip", ArchiveFormat::Zip),
            ("one.skill", ArchiveFormat::Zip),
            ("one.tar", ArchiveFormat::Tar),
            ("one.tar.gz", ArchiveFormat::TarGz),
            ("ONE.TGZ", ArchiveFormat::TarGz),
        ] {
            assert_eq!(archive(std::path::Path::new(path)), Some(expected));
        }
        for path in ["one", "one.json", "one.zip.exe"] {
            assert_eq!(archive(std::path::Path::new(path)), None);
        }
    }

    #[test]
    fn binds_install_to_the_discovered_snapshot() {
        let candidate = Candidate {
            name: "skill-analysis".into(),
            path: "nested/analysis".into(),
            skill: plugin::Skill {
                name: "Analysis".into(),
                display_name: None,
                description: "Fixture skill".into(),
                path: "SKILL.md".into(),
                license: Some("MIT".into()),
                compatibility: None,
            },
        };
        let discovery = ImportDiscovery {
            stream: StreamId::new(),
            digest: "a".repeat(64),
            kind: UploadSource::Directory,
            skills: vec![candidate.clone()],
        };
        let Command::InstallSkillUpload {
            stream,
            source,
            name,
            expected_revision,
        } = command(discovery.clone(), &candidate, 7).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(stream, discovery.stream);
        assert_eq!(
            source,
            Imported {
                digest: discovery.digest.clone(),
                path: candidate.path.clone(),
                kind: discovery.kind
            }
        );
        assert_eq!(name, candidate.name);
        assert_eq!(expected_revision, 7);
        let mut changed = candidate.clone();
        changed.path = "other".into();
        assert!(command(discovery.clone(), &changed, 7).is_err());
        let mut changed = discovery;
        changed.skills.push(candidate.clone());
        assert!(command(changed, &candidate, 7).is_err());
    }
}
