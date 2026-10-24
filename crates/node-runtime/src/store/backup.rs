//! Backup configuration and snapshots share the authoritative Store owner.
use super::{
    Ingress, Job,
    database::{Database, Exclusive, Handle, Storage, encode, storage_error},
    unavailable,
};
use crate::backup::{self, Auth, Plan, archive::Snapshot};
use sailry_link::{Admission, CancellationToken};
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, Output, Request, RequestId, Secret,
    backup::{Kind, Settings, Update},
    plugin::settings::SecretUpdate,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::oneshot;
use turso::{Connection, params, transaction::TransactionBehavior};

type Capture = (Plan, Snapshot, Vec<String>);

pub(super) struct Source {
    plan: Plan,
    snapshot: Snapshot,
    roots: Vec<String>,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Stored {
    pub settings: Settings,
    pub auth: Auth,
}

pub(super) async fn read(db: &turso::Connection) -> Result<Stored, Fault> {
    let body = match db
        .prepare("SELECT body FROM backup_settings WHERE singleton=1")
        .await
        .map_err(storage_error)?
        .query_row(())
        .await
    {
        Ok(row) => Some(row.get::<Vec<u8>>(0).map_err(storage_error)?),
        Err(turso::Error::QueryReturnedNoRows) => None,
        Err(error) => return Err(storage_error(error)),
    };
    let stored: Stored = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?
        .unwrap_or_default();
    validate(&stored)?;
    Ok(stored)
}

fn validate(stored: &Stored) -> Result<(), Fault> {
    if stored.settings.revision > i64::MAX as u64 {
        return Err(Fault::invalid("invalid backup settings revision"));
    }
    if let Some(destination) = &stored.settings.destination {
        destination.validate()?;
        if destination.kind != Kind::S3 && stored.auth.session_token.is_some() {
            return Err(Fault::invalid("backup session token requires S3"));
        }
    }
    if let Some(local) = &stored.settings.local {
        let path = Path::new(&local.directory);
        if !path.is_absolute()
            || local.directory.len() > 4096
            || local.directory.chars().any(char::is_control)
            || path.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(Fault::invalid(
                "local backup directory must be an absolute path",
            ));
        }
    }
    if let Some(schedule) = &stored.settings.schedule {
        super::dispatch::native::validate_time(schedule)?;
        if schedule.enabled
            && !(stored
                .settings
                .local
                .as_ref()
                .is_some_and(|local| local.enabled)
                || stored.settings.online_enabled && stored.settings.destination.is_some())
        {
            return Err(Fault::invalid(
                "automatic backup requires an enabled destination",
            ));
        }
        if schedule.enabled {
            backup::encryption::passphrase(stored.auth.passphrase.as_ref())?;
        }
    }
    for value in [
        &stored.auth.identity,
        &stored.auth.secret,
        &stored.auth.session_token,
        &stored.auth.passphrase,
    ]
    .into_iter()
    .flatten()
    {
        if value.expose().is_empty()
            || value.expose().len() > 8192
            || value.expose().chars().any(char::is_control)
        {
            return Err(Fault::invalid("invalid backup credential"));
        }
    }
    if stored.settings.credentials_configured != configured(&stored.auth) {
        return Err(Fault::invalid(
            "backup credential state differs from stored values",
        ));
    }
    if stored.settings.passphrase_configured != stored.auth.passphrase.is_some() {
        return Err(Fault::invalid(
            "backup passphrase state differs from stored value",
        ));
    }
    Ok(())
}

fn configured(auth: &Auth) -> bool {
    auth.identity.is_some() || auth.secret.is_some() || auth.session_token.is_some()
}

pub(super) async fn execute(
    db: &turso::Connection,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    controller(request)?;
    let mut stored = read(db).await?;
    match &request.command {
        Command::ReadBackupSettings => Ok((Output::BackupSettings(stored.settings), None)),
        Command::SaveBackupSettings(Update {
            settings,
            identity,
            secret,
            session_token,
            passphrase,
        }) => {
            super::commands::check_revision(stored.settings.revision, settings.revision)?;
            if let SecretUpdate::Replace(value) = passphrase {
                sailry_protocol::backup::validate_passphrase(value.expose())?;
            }
            if stored
                .settings
                .destination
                .as_ref()
                .map(|destination| destination.kind)
                != settings
                    .destination
                    .as_ref()
                    .map(|destination| destination.kind)
            {
                stored.auth = Auth {
                    passphrase: stored.auth.passphrase.take(),
                    ..Auth::default()
                };
            }
            update(&mut stored.auth.identity, identity);
            update(&mut stored.auth.secret, secret);
            update(&mut stored.auth.session_token, session_token);
            update(&mut stored.auth.passphrase, passphrase);
            stored.settings = settings.clone();
            stored.settings.revision = settings
                .revision
                .checked_add(1)
                .filter(|revision| *revision <= i64::MAX as u64)
                .ok_or_else(|| Fault::invalid("backup settings revision is exhausted"))?;
            stored.settings.credentials_configured = configured(&stored.auth);
            stored.settings.passphrase_configured = stored.auth.passphrase.is_some();
            validate(&stored)?;
            super::dispatch::native::configure(db, &stored.settings).await?;
            db.execute("INSERT INTO backup_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&stored)?]).await.map_err(storage_error)?;
            Ok((
                Output::BackupSettings(stored.settings.clone()),
                Some(Event::BackupSettingsChanged {
                    revision: stored.settings.revision,
                }),
            ))
        }
        _ => Err(Fault::invalid("backup settings command expected")),
    }
}

fn update(current: &mut Option<Secret>, value: &SecretUpdate) {
    match value {
        SecretUpdate::Keep => {}
        SecretUpdate::Replace(value) => *current = Some(value.clone()),
        SecretUpdate::Clear => *current = None,
    }
}

pub(super) async fn plan(db: &turso::Connection, expected: Option<u64>) -> Result<Plan, Fault> {
    let stored = read(db).await?;
    if let Some(expected) = expected {
        super::commands::check_revision(stored.settings.revision, expected)?;
    }
    Ok(Plan {
        destination: stored.settings.destination,
        local: stored.settings.local,
        online_enabled: stored.settings.online_enabled,
        auth: stored.auth,
    })
}

pub(super) async fn source(
    storage: &Storage,
    knowledge: Option<&Arc<Handle>>,
    revision: u64,
    cancel: &CancellationToken,
) -> Result<Source, Fault> {
    backup::cancelled(cancel)?;
    let profile = storage
        .profile
        .as_deref()
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "backup requires a Node profile"))?;
    if storage.handle.path() != profile.join("storage/node.turso")
        || knowledge.is_some_and(|handle| handle.path() != profile.join("storage/knowledge.turso"))
    {
        return Err(Fault::invalid(
            "backup source path differs from its profile",
        ));
    }
    let main = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(cancelled()),
        main = storage.handle.exclusive() => main.map_err(storage_error)?,
    };
    let knowledge = if let Some(handle) = knowledge {
        Some(tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            knowledge = handle.exclusive() => knowledge.map_err(storage_error)?,
        })
    } else {
        None
    };
    let transaction = turso::transaction::Transaction::new_unchecked(
        main.connection(),
        TransactionBehavior::Deferred,
    )
    .await
    .map_err(storage_error)?;
    let prepared: Result<(Plan, Vec<String>), Fault> = async {
        let identity = transaction
            .prepare("SELECT identity FROM node WHERE singleton=1")
            .await
            .map_err(storage_error)?
            .query_row(())
            .await
            .map_err(storage_error)?
            .get::<Vec<u8>>(0)
            .map_err(storage_error)?;
        if identity.as_slice() != storage.node.0.as_slice() {
            return Err(Fault::invalid("backup source identity changed"));
        }
        let plan = plan(&transaction, Some(revision)).await?;
        backup::encryption::passphrase(plan.auth.passphrase.as_ref())?;
        let roots = roots(&transaction, profile, cancel).await?;
        Ok((plan, roots))
    }
    .await;
    let (plan, roots) = match prepared {
        Ok(prepared) => {
            transaction.commit().await.map_err(storage_error)?;
            prepared
        }
        Err(error) => {
            transaction.rollback().await.map_err(storage_error)?;
            return Err(error);
        }
    };
    backup::cancelled(cancel)?;
    let snapshot = Snapshot::new(profile, storage.node)?;
    freeze(main, knowledge, plan, roots, snapshot, cancel).await
}

async fn freeze(
    main: Exclusive,
    knowledge: Option<Exclusive>,
    plan: Plan,
    roots: Vec<String>,
    snapshot: Snapshot,
    cancel: &CancellationToken,
) -> Result<Source, Fault> {
    let main = main.close().await.map_err(storage_error)?;
    let mut closed_knowledge = None;
    // Once a handle is closed, cancellation is a result, not a dropped future:
    // every exit below must attempt to reopen both databases.
    let frozen: Result<Source, Fault> = async {
        backup::cancelled(cancel)?;
        if let Some(knowledge) = knowledge {
            closed_knowledge = Some(knowledge.close().await.map_err(storage_error)?);
        }
        backup::cancelled(cancel)?;
        let main_path = main.path().to_owned();
        let knowledge_path = closed_knowledge
            .as_ref()
            .map(|closed| closed.path().to_owned());
        let stop = cancel.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            let mut snapshot = snapshot;
            snapshot.freeze(&main_path, &stop)?;
            if let Some(path) = knowledge_path {
                snapshot.freeze(&path, &stop)?;
            }
            Ok::<_, Fault>(snapshot)
        })
        .await
        .map_err(io_fault)??;
        Ok(Source {
            plan,
            snapshot,
            roots,
        })
    }
    .await;
    let knowledge_reopened = if let Some(knowledge) = closed_knowledge {
        knowledge.reopen().await.map_err(storage_error)
    } else {
        Ok(())
    };
    let main_reopened = main.reopen().await.map_err(storage_error);
    knowledge_reopened?;
    main_reopened?;
    backup::cancelled(cancel)?;
    frozen
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "profile backup cancelled")
}

async fn roots(
    db: &Connection,
    profile: &Path,
    cancel: &CancellationToken,
) -> Result<Vec<String>, Fault> {
    let mut roots = vec!["link.key".into()];
    let mut artifacts = db
        .query("SELECT session,id FROM artifacts ORDER BY session,id", ())
        .await
        .map_err(storage_error)?;
    while let Some(row) = artifacts.next().await.map_err(storage_error)? {
        backup::cancelled(cancel)?;
        let session = row.get::<String>(0).map_err(storage_error)?;
        let id = row.get::<String>(1).map_err(storage_error)?;
        crate::files::path::components(&session, false)?;
        crate::files::path::components(&id, false)?;
        roots.push(format!("artifacts/{session}/{id}"));
    }
    drop(artifacts);
    for (query, prefix) in [
        (
            "SELECT digest FROM plugin_packages ORDER BY digest",
            "plugins/packages",
        ),
        ("SELECT id FROM attachments ORDER BY id", "attachments"),
    ] {
        let mut rows = db.query(query, ()).await.map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            backup::cancelled(cancel)?;
            let value = row.get::<String>(0).map_err(storage_error)?;
            crate::files::path::components(&value, false)?;
            roots.push(format!("{prefix}/{value}"));
        }
    }
    let mut rows = db
        .query("SELECT path FROM worktrees WHERE id NOT IN (SELECT worktree FROM removed_worktrees) ORDER BY path", ())
        .await
        .map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        backup::cancelled(cancel)?;
        let value = PathBuf::from(row.get::<String>(0).map_err(storage_error)?);
        if let Ok(relative) = value.strip_prefix(profile) {
            if !crate::workspaces::managed(&value, profile) {
                return Err(Fault::invalid(
                    "worktree is outside a managed profile workspace",
                ));
            }
            let relative = relative
                .iter()
                .map(|part| part.to_str())
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| Fault::invalid("managed worktree names must be UTF-8"))?
                .join("/");
            roots.push(relative);
        }
    }
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn capture(source: Source, cancel: &CancellationToken) -> Result<Capture, Fault> {
    backup::cancelled(cancel)?;
    Ok((source.plan, source.snapshot, source.roots))
}

pub(super) fn controller(request: &Request) -> Result<(), Fault> {
    if request.plugin.is_some() {
        Err(Fault::new(
            ErrorCode::PermissionDenied,
            "profile backups require a controller",
        ))
    } else {
        Ok(())
    }
}

pub(super) async fn prepare(database: &Database, request: &Request) -> Result<(), Fault> {
    controller(request)?;
    let revision = match &request.command {
        Command::ExportBackup { expected_revision }
        | Command::RestoreBackup {
            expected_revision, ..
        }
        | Command::RestoreBackupFile {
            expected_revision, ..
        } => *expected_revision,
        _ => return Err(Fault::invalid("backup operation expected")),
    };
    let plan = plan(&database.connection, Some(revision)).await?;
    match &request.command {
        Command::ExportBackup { .. } => {
            if !(plan.online_enabled && plan.destination.is_some()
                || plan.local.as_ref().is_some_and(|local| local.enabled))
            {
                return Err(Fault::new(
                    ErrorCode::NotConfigured,
                    "backup destination is not configured",
                ));
            }
            backup::encryption::passphrase(plan.auth.passphrase.as_ref())?;
        }
        Command::RestoreBackup { passphrase, .. }
        | Command::RestoreBackupFile { passphrase, .. } => {
            backup::encryption::passphrase(passphrase.as_ref().or(plan.auth.passphrase.as_ref()))?;
        }
        _ => unreachable!(),
    }
    if let Command::RestoreBackupFile { target, .. } = &request.command {
        target_path(target, database.profile.as_deref())?;
    }
    if let Command::RestoreBackup { id, target, .. } = &request.command {
        if !uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == *id) {
            return Err(Fault::invalid("invalid backup identifier"));
        }
        target_path(target, database.profile.as_deref())?;
    }
    Ok(())
}

impl Ingress {
    async fn backup_plan(&self, expected: Option<u64>) -> Result<Plan, Fault> {
        let (reply, result) = oneshot::channel();
        self.send_internal(Job::BackupPlan { expected, reply })
            .await?;
        result.await.map_err(|_| unavailable())?
    }

    pub(super) async fn read_backup(&self, request: Request) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        controller(&request)?;
        let plan = self.backup_plan(None).await?;
        let output = Output::Backups(backup::list(&plan, &self.closed.child_token()).await?);
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(output));
        Ok(Admission {
            receipt: sailry_protocol::Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    async fn capture_backup(
        &self,
        revision: u64,
        cancel: &CancellationToken,
    ) -> Result<(Plan, backup::archive::Packed), Fault> {
        let resources = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(Fault::new(ErrorCode::Cancelled,"profile backup cancelled")),
            resources = self.backup_resources.clone().read_owned() => resources,
        };
        let (reply, result) = oneshot::channel();
        self.send_internal(Job::BackupSnapshot {
            revision,
            cancel: cancel.clone(),
            reply,
        })
        .await?;
        let source = result.await.map_err(|_| unavailable())??;
        let stop = cancel.clone();
        tokio::task::spawn_blocking(move || {
            let (plan, mut snapshot, roots) = capture(source, &stop)?;
            snapshot.capture(&roots, &stop)?;
            drop(resources);
            Ok((plan, snapshot.pack(&stop)?))
        })
        .await
        .map_err(io_fault)?
    }

    pub(super) async fn download_backup(
        &self,
        caller: sailry_protocol::NodeId,
        revision: u64,
        id: RequestId,
    ) -> Result<Output, Fault> {
        let stop = self.closed.child_token();
        let preparation = stop.clone().drop_guard();
        let (plan, packed) = self.capture_backup(revision, &stop).await?;
        let passphrase = backup::encryption::passphrase(plan.auth.passphrase.as_ref())?.clone();
        let cancel = stop.clone();
        let (packed, record, digest) = tokio::task::spawn_blocking(move || {
            use std::io::Read;
            let packed = backup::encryption::seal(packed, &passphrase, &cancel)?;
            let record = backup::record(&packed, id.to_string())?;
            let mut input = packed.file.reopen().map_err(backup::io)?;
            let mut hash = blake3::Hasher::new();
            let mut bytes = [0; 64 * 1024];
            loop {
                backup::cancelled(&cancel)?;
                let count = input.read(&mut bytes).map_err(backup::io)?;
                if count == 0 {
                    break;
                }
                hash.update(&bytes[..count]);
            }
            Ok::<_, Fault>((packed, record, hash.finalize().to_hex().to_string()))
        })
        .await
        .map_err(io_fault)??;
        let output = Output::BackupDownload(self.transfers.download_backup(
            caller,
            packed.file,
            packed.snapshot.directory,
            record,
            digest,
            stop,
        )?);
        preparation.disarm();
        Ok(output)
    }

    pub(super) async fn change_backup(
        &self,
        caller: sailry_protocol::NodeId,
        request: &Request,
        cancel: &CancellationToken,
    ) -> Result<Output, Fault> {
        controller(request)?;
        match &request.command {
            Command::ExportBackup { expected_revision } => {
                let (plan, packed) = self.capture_backup(*expected_revision, cancel).await?;
                if let Some(local) = plan.local.as_ref().filter(|local| local.enabled) {
                    local_target(&local.directory, &packed.snapshot.manifest.original_profile)?;
                }
                Ok(Output::Backup(
                    backup::export(&plan, packed, request.id.to_string(), cancel).await?,
                ))
            }
            Command::RestoreBackup {
                id,
                expected_revision,
                target,
                passphrase,
                location,
            } => {
                let mut plan = self.backup_plan(Some(*expected_revision)).await?;
                if let Some(passphrase) = passphrase {
                    plan.auth.passphrase = Some(passphrase.clone());
                }
                let profile = self.profile.as_deref().ok_or_else(|| {
                    Fault::new(ErrorCode::NotConfigured, "backup requires a Node profile")
                })?;
                let target = target_path(target, Some(profile))?;
                let temporary = crate::profile::temporary(profile)?;
                let (record, archive) =
                    backup::restore(&plan, id, *location, &temporary, cancel).await?;
                let stop = cancel.clone();
                let restored = record.clone();
                let passphrase =
                    backup::encryption::passphrase(plan.auth.passphrase.as_ref())?.clone();
                let archive = tokio::task::spawn_blocking(move || {
                    backup::encryption::open(&archive, &passphrase, &stop)
                })
                .await
                .map_err(io_fault)??;
                let path = publish(archive, &target, RecordSource::Known(restored), cancel).await?;
                Ok(Output::BackupRestored {
                    record,
                    target: path.1.to_string_lossy().into_owned(),
                })
            }
            Command::RestoreBackupFile {
                stream,
                expected_revision,
                target,
                passphrase,
            } => {
                let plan = self.backup_plan(Some(*expected_revision)).await?;
                let passphrase = backup::encryption::passphrase(
                    passphrase.as_ref().or(plan.auth.passphrase.as_ref()),
                )?
                .clone();
                let profile = self.profile.as_deref().ok_or_else(|| {
                    Fault::new(ErrorCode::NotConfigured, "backup requires a Node profile")
                })?;
                let target = target_path(target, Some(profile))?;
                let temporary = crate::profile::temporary(profile)?;
                let uploaded = self.transfers.claim_backup(caller, *stream)?;
                let stop = cancel.clone();
                let id = request.id.to_string();
                let mut input = uploaded.file.try_clone().map_err(backup::io)?;
                let size = uploaded.size;
                let (archive, sha256) = tokio::task::spawn_blocking(move || {
                    use std::io::Seek;
                    let sha256 = backup::archive::hash(&mut input, size, &stop)?;
                    input.rewind().map_err(backup::io)?;
                    let archive =
                        backup::encryption::open_file(input, &temporary, &passphrase, &stop)?;
                    Ok::<_, Fault>((archive, sha256))
                })
                .await
                .map_err(io_fault)??;
                let restored = publish(
                    archive,
                    &target,
                    RecordSource::Imported { id, size, sha256 },
                    cancel,
                )
                .await;
                drop(uploaded);
                let (record, path) = restored?;
                Ok(Output::BackupRestored {
                    record,
                    target: path.to_string_lossy().into_owned(),
                })
            }
            _ => Err(Fault::invalid("backup operation expected")),
        }
    }
}

fn local_target(value: &str, profile: &Path) -> Result<(), Fault> {
    let path = Path::new(value);
    let mut existing = path;
    while !existing.try_exists().map_err(backup::io)? {
        existing = existing
            .parent()
            .ok_or_else(|| Fault::invalid("backup parent is missing"))?;
    }
    crate::files::path::root(existing)?;
    let parent = existing.canonicalize().map_err(backup::io)?;
    let resolved = parent.join(path.strip_prefix(existing).map_err(backup::io)?);
    if resolved.starts_with(profile) || profile.starts_with(&resolved) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "local backup destination overlaps the active profile",
        ));
    }
    Ok(())
}

fn target_path(value: &str, profile: Option<&Path>) -> Result<PathBuf, Fault> {
    let path = PathBuf::from(value);
    if !path.is_absolute()
        || path.file_name().is_none()
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(Fault::invalid(
            "restore requires a new absolute profile directory",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Fault::invalid("restore parent is missing"))?;
    let parent = crate::files::path::root(parent)?;
    let name = path.file_name().unwrap();
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {
            let dir = cap_fs_ext::DirExt::open_dir_nofollow(&parent, name).map_err(backup::io)?;
            if dir.entries().map_err(backup::io)?.next().is_some() {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "restore destination must be empty",
                ));
            }
        }
        Ok(_) => {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "restore destination must be an empty directory",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(backup::io(error)),
    }
    if profile.is_some_and(|profile| path.starts_with(profile) || profile.starts_with(&path)) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "restore destination overlaps the active profile",
        ));
    }
    Ok(path)
}

enum RecordSource {
    Known(sailry_protocol::backup::Record),
    Imported {
        id: String,
        size: u64,
        sha256: String,
    },
}

async fn publish(
    archive: tempfile::NamedTempFile,
    target: &Path,
    source: RecordSource,
    cancel: &CancellationToken,
) -> Result<(sailry_protocol::backup::Record, PathBuf), Fault> {
    target_path(
        target
            .to_str()
            .ok_or_else(|| Fault::invalid("restore path must be UTF-8"))?,
        None,
    )?;
    let target = target.to_owned();
    let stop = cancel.clone();
    let staged = tokio::task::spawn_blocking(move || stage(archive, source, &stop))
        .await
        .map_err(io_fault)??;
    backup::cancelled(cancel)?;
    validate_profile(staged.directory.path(), staged.record.node).await?;
    backup::cancelled(cancel)?;
    relocate(
        staged.directory.path(),
        &staged.manifest.original_profile,
        &target,
    )
    .await?;
    validate_profile(staged.directory.path(), staged.record.node).await?;
    let stop = cancel.clone();
    tokio::task::spawn_blocking(move || finish(staged, &target, &stop))
        .await
        .map_err(io_fault)?
}

struct Staged {
    directory: tempfile::TempDir,
    manifest: backup::archive::Manifest,
    record: sailry_protocol::backup::Record,
}

fn stage(
    archive: tempfile::NamedTempFile,
    source: RecordSource,
    cancel: &CancellationToken,
) -> Result<Staged, Fault> {
    // Native validation and relocation stay in the Node's private transfer directory.
    let directory = tempfile::Builder::new()
        .prefix("restore-")
        .tempdir_in(archive.path().parent().unwrap())
        .map_err(backup::io)?;
    let manifest = backup::archive::unpack(
        archive.reopen().map_err(backup::io)?,
        directory.path(),
        cancel,
    )?;
    let record = match source {
        RecordSource::Known(record) => record,
        RecordSource::Imported { id, size, sha256 } => sailry_protocol::backup::Record {
            id,
            node: manifest.node,
            created_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
            size_bytes: size,
            files: manifest.files.len() as u32,
            sha256,
        },
    };
    if manifest.node != record.node || manifest.files.len() != record.files as usize {
        return Err(Fault::invalid(
            "restored profile differs from remote metadata",
        ));
    }
    Ok(Staged {
        directory,
        manifest,
        record,
    })
}

fn finish(
    mut staged: Staged,
    target: &Path,
    cancel: &CancellationToken,
) -> Result<(sailry_protocol::backup::Record, PathBuf), Fault> {
    use cap_fs_ext::DirExt;
    target_path(
        target
            .to_str()
            .ok_or_else(|| Fault::invalid("restore path must be UTF-8"))?,
        None,
    )?;
    staged
        .manifest
        .refresh_databases(staged.directory.path(), cancel)?;
    let parent = target.parent().unwrap();
    let parent_cap = crate::files::path::root(parent)?;
    let identity = crate::files::path::identity::directory(&parent_cap)?;
    let name = format!(".sailry-restore-{}", uuid::Uuid::new_v4());
    let mut builder = cap_std::fs::DirBuilder::new();
    #[cfg(unix)]
    cap_std::fs::DirBuilderExt::mode(&mut builder, 0o700);
    parent_cap
        .create_dir_with(&name, &builder)
        .map_err(backup::io)?;
    let mut destination = Target {
        directory: Some(parent_cap.open_dir_nofollow(&name).map_err(backup::io)?),
    };
    backup::archive::transfer(
        staged.directory.path(),
        destination.directory.as_ref().unwrap(),
        &staged.manifest,
        cancel,
    )?;
    backup::cancelled(cancel)?;
    target_path(target.to_str().unwrap(), None)?;
    if identity != crate::files::path::identity::directory(&crate::files::path::root(parent)?)? {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "restore parent changed",
        ));
    }
    let target_name = target.file_name().unwrap();
    match parent_cap.remove_dir(target_name) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(backup::io(error)),
    }
    parent_cap
        .rename(&name, &parent_cap, target_name)
        .map_err(backup::io)?;
    drop(destination.directory.take());
    Ok((staged.record, target.to_owned()))
}

struct Target {
    directory: Option<cap_std::fs::Dir>,
}
impl Drop for Target {
    fn drop(&mut self) {
        if let Some(directory) = self.directory.take() {
            let _ = directory.remove_open_dir_all();
        }
    }
}

async fn validate_profile(path: &Path, node: sailry_protocol::NodeId) -> Result<(), Fault> {
    let identity = sailry_link::Identity::open(path).map_err(backup::io)?;
    if identity.id() != node {
        return Err(Fault::invalid(
            "restored identity differs from its database",
        ));
    }
    validate_database(&path.join("storage/node.turso"), Some(node)).await?;
    let knowledge = path.join("storage/knowledge.turso");
    match knowledge.symlink_metadata() {
        Ok(_) => validate_database(&knowledge, None).await,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(backup::io(error)),
    }
}

async fn validate_database(
    path: &Path,
    node: Option<sailry_protocol::NodeId>,
) -> Result<(), Fault> {
    let handle = Handle::new(
        path.to_owned(),
        super::database::engine(path).await.map_err(storage_error)?,
    );
    let validation: Result<(), Fault> = async {
        let connection = handle.lease(true).await.map_err(storage_error)?;
        if let Some(node) = node {
            super::database::validate_backup(&connection, node).await
        } else {
            let integrity = connection
                .prepare("PRAGMA integrity_check")
                .await
                .map_err(storage_error)?
                .query_row(())
                .await
                .map_err(storage_error)?
                .get::<String>(0)
                .map_err(storage_error)?;
            if integrity == "ok" {
                Ok(())
            } else {
                Err(Fault::invalid("restored knowledge database is corrupt"))
            }
        }
    }
    .await;
    let closed = handle.pause().await.map_err(storage_error)?;
    drop(closed);
    validation
}

async fn relocate(staging: &Path, original: &Path, target: &Path) -> Result<(), Fault> {
    let path = staging.join("storage/node.turso");
    let handle = Handle::new(
        path.clone(),
        super::database::engine(&path)
            .await
            .map_err(storage_error)?,
    );
    let relocated: Result<(), Fault> = async {
        let connection = handle.lease(false).await.map_err(storage_error)?;
        let transaction = turso::transaction::Transaction::new_unchecked(
            &connection,
            TransactionBehavior::Concurrent,
        )
        .await
        .map_err(storage_error)?;
        match relocate_rows(&transaction, original, target).await {
            Ok(()) => transaction.commit().await.map_err(storage_error),
            Err(error) => {
                transaction.rollback().await.map_err(storage_error)?;
                Err(error)
            }
        }
    }
    .await;
    let closed = handle.pause().await.map_err(storage_error)?;
    drop(closed);
    relocated
}

async fn relocate_rows(db: &Connection, original: &Path, target: &Path) -> Result<(), Fault> {
    for table in ["projects", "worktrees"] {
        let rows = {
            let mut rows = db
                .query(format!("SELECT id,path FROM {table}"), ())
                .await
                .map_err(storage_error)?;
            let mut values = Vec::new();
            while let Some(row) = rows.next().await.map_err(storage_error)? {
                values.push((
                    row.get::<String>(0).map_err(storage_error)?,
                    row.get::<String>(1).map_err(storage_error)?,
                ));
            }
            values
        };
        for (id, value) in rows {
            if let Some(value) = relocated(&value, original, target)? {
                db.execute(
                    format!("UPDATE {table} SET path=?1 WHERE id=?2"),
                    params![value, id],
                )
                .await
                .map_err(storage_error)?;
            }
        }
    }
    let rows = {
        let mut rows = db
            .query("SELECT id,body FROM knowledge_collections", ())
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            values.push((
                row.get::<String>(0).map_err(storage_error)?,
                row.get::<Vec<u8>>(1).map_err(storage_error)?,
            ));
        }
        values
    };
    for (id, body) in rows {
        let mut stored: super::knowledge::Stored =
            serde_json::from_slice(&body).map_err(storage_error)?;
        match &mut stored.collection.definition.source {
            sailry_protocol::knowledge::Source::File { path }
            | sailry_protocol::knowledge::Source::Directory { path } => {
                if let Some(value) = relocated(path, original, target)? {
                    *path = value;
                    db.execute(
                        "UPDATE knowledge_collections SET body=?1 WHERE id=?2",
                        params![encode(&stored)?, id],
                    )
                    .await
                    .map_err(storage_error)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn relocated(value: &str, original: &Path, target: &Path) -> Result<Option<String>, Fault> {
    let Ok(relative) = Path::new(value).strip_prefix(original) else {
        return Ok(None);
    };
    if !relative.as_os_str().is_empty() {
        let relative = relative
            .iter()
            .map(|part| part.to_str())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| Fault::invalid("restored source path must be UTF-8"))?
            .join("/");
        crate::files::path::components(&relative, false)?;
    }
    Ok(Some(
        target
            .join(relative)
            .to_str()
            .ok_or_else(|| Fault::invalid("restore path must be UTF-8"))?
            .into(),
    ))
}

fn io_fault(error: tokio::task::JoinError) -> Fault {
    backup::io(error)
}

#[cfg(test)]
mod tests;
