//! Backup configuration and snapshots share the authoritative Store owner.
use super::{
    Ingress, Job,
    database::{Database, encode, storage_error},
    unavailable,
};
use crate::backup::{self, Auth, Plan, archive::Snapshot};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use sailry_link::{Admission, CancellationToken};
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, Output, Request, RequestId, Secret,
    backup::{Kind, Settings, Update},
    plugin::settings::SecretUpdate,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::oneshot;

type Capture = (Plan, Snapshot, Vec<String>);

pub(super) struct Source {
    profile: PathBuf,
    node: sailry_protocol::NodeId,
    revision: u64,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Stored {
    pub settings: Settings,
    pub auth: Auth,
}

pub(super) fn read(db: &Connection) -> Result<Stored, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM backup_settings WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
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

pub(super) fn execute(
    db: &Connection,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    controller(request)?;
    let mut stored = read(db)?;
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
            super::dispatch::native::configure(db, &stored.settings)?;
            db.execute("INSERT INTO backup_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&stored)?]).map_err(storage_error)?;
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

pub(super) fn plan(db: &Connection, expected: Option<u64>) -> Result<Plan, Fault> {
    let stored = read(db)?;
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

pub(super) fn source(
    database: &Database,
    revision: u64,
    cancel: &CancellationToken,
) -> Result<Source, Fault> {
    backup::cancelled(cancel)?;
    let plan = plan(&database.connection, Some(revision))?;
    backup::encryption::passphrase(plan.auth.passphrase.as_ref())?;
    Ok(Source {
        profile: database.profile.clone().ok_or_else(|| {
            Fault::new(ErrorCode::NotConfigured, "backup requires a Node profile")
        })?,
        node: database.node,
        revision,
    })
}

fn capture(source: Source, cancel: &CancellationToken) -> Result<Capture, Fault> {
    capture_with(source, cancel, |_| Ok(()))
}

fn capture_with(
    source: Source,
    cancel: &CancellationToken,
    ready: impl FnOnce(&Connection) -> Result<(), Fault>,
) -> Result<Capture, Fault> {
    backup::cancelled(cancel)?;
    let db = Connection::open_with_flags(
        source.profile.join("storage/node.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(storage_error)?;
    db.busy_timeout(Duration::from_secs(1))
        .map_err(storage_error)?;
    db.execute_batch("BEGIN").map_err(storage_error)?;
    let identity: Vec<u8> = db
        .query_row("SELECT identity FROM node WHERE singleton=1", [], |row| {
            row.get(0)
        })
        .map_err(storage_error)?;
    if identity.as_slice() != source.node.0.as_slice() {
        return Err(Fault::invalid("backup source identity changed"));
    }
    let plan = plan(&db, Some(source.revision))?;
    backup::encryption::passphrase(plan.auth.passphrase.as_ref())?;
    ready(&db)?;
    let profile = &source.profile;
    let mut snapshot = Snapshot::new(profile, source.node)?;
    let path = snapshot.database_path()?;
    let mut destination = Connection::open(&path).map_err(storage_error)?;
    {
        let transfer =
            rusqlite::backup::Backup::new(&db, &mut destination).map_err(storage_error)?;
        loop {
            backup::cancelled(cancel)?;
            match transfer.step(256).map_err(storage_error)? {
                rusqlite::backup::StepResult::Done => break,
                rusqlite::backup::StepResult::More => {}
                rusqlite::backup::StepResult::Busy | rusqlite::backup::StepResult::Locked => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                _ => return Err(storage_error("unexpected SQLite backup state")),
            }
        }
    }
    drop(destination);
    let mut roots = vec!["link.key".into()];
    let mut artifacts = db
        .prepare("SELECT session,id FROM artifacts ORDER BY session,id")
        .map_err(storage_error)?;
    for row in artifacts
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?
    {
        let (session, id) = row.map_err(storage_error)?;
        crate::files::path::components(&session, false)?;
        crate::files::path::components(&id, false)?;
        roots.push(format!("artifacts/{session}/{id}"));
    }
    for (query, prefix) in [
        (
            "SELECT digest FROM plugin_packages ORDER BY digest",
            "plugins/packages",
        ),
        ("SELECT id FROM attachments ORDER BY id", "attachments"),
    ] {
        let mut statement = db.prepare(query).map_err(storage_error)?;
        for value in statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage_error)?
        {
            let value = value.map_err(storage_error)?;
            crate::files::path::components(&value, false)?;
            roots.push(format!("{prefix}/{value}"));
        }
    }
    let mut statement = db
        .prepare("SELECT path FROM worktrees WHERE id NOT IN (SELECT worktree FROM removed_worktrees) ORDER BY path")
        .map_err(storage_error)?;
    for value in statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(storage_error)?
    {
        let value = PathBuf::from(value.map_err(storage_error)?);
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
    db.execute_batch("COMMIT").map_err(storage_error)?;
    snapshot.database(cancel)?;
    Ok((plan, snapshot, roots))
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

pub(super) fn prepare(database: &Database, request: &Request) -> Result<(), Fault> {
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
    let plan = plan(&database.connection, Some(revision))?;
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
                let path = tokio::task::spawn_blocking(move || {
                    let archive = backup::encryption::open(&archive, &passphrase, &stop)?;
                    publish(archive, &target, RecordSource::Known(restored), &stop)
                })
                .await
                .map_err(io_fault)??;
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
                let (record, path) = tokio::task::spawn_blocking(move || {
                    use std::io::Seek;
                    let mut input = uploaded.file.try_clone().map_err(backup::io)?;
                    let sha256 = backup::archive::hash(&mut input, uploaded.size, &stop)?;
                    input.rewind().map_err(backup::io)?;
                    let archive =
                        backup::encryption::open_file(input, &temporary, &passphrase, &stop)?;
                    let result = publish(
                        archive,
                        &target,
                        RecordSource::Imported {
                            id,
                            size: uploaded.size,
                            sha256,
                        },
                        &stop,
                    );
                    drop(uploaded);
                    result
                })
                .await
                .map_err(io_fault)??;
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

fn publish(
    archive: tempfile::NamedTempFile,
    target: &Path,
    source: RecordSource,
    cancel: &CancellationToken,
) -> Result<(sailry_protocol::backup::Record, PathBuf), Fault> {
    use cap_fs_ext::DirExt;
    target_path(
        target
            .to_str()
            .ok_or_else(|| Fault::invalid("restore path must be UTF-8"))?,
        None,
    )?;
    // SQLite validation and relocation stay in the Node's private transfer directory.
    let staging = tempfile::Builder::new()
        .prefix("restore-")
        .tempdir_in(archive.path().parent().unwrap())
        .map_err(backup::io)?;
    let mut manifest = backup::archive::unpack(
        archive.reopen().map_err(backup::io)?,
        staging.path(),
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
    validate_profile(staging.path(), record.node)?;
    relocate(staging.path(), &manifest.original_profile, target)?;
    validate_profile(staging.path(), record.node)?;
    let database = staging.path().join("storage/node.sqlite3");
    let size = std::fs::metadata(&database).map_err(backup::io)?.len();
    let digest = backup::archive::hash(
        &mut std::fs::File::open(database).map_err(backup::io)?,
        size,
        cancel,
    )?;
    let file = manifest
        .files
        .iter_mut()
        .find(|file| file.path == "storage/node.sqlite3")
        .unwrap();
    file.size = size;
    file.sha256 = digest;
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
        staging.path(),
        destination.directory.as_ref().unwrap(),
        &manifest,
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
    Ok((record, target.to_owned()))
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

fn validate_profile(path: &Path, node: sailry_protocol::NodeId) -> Result<(), Fault> {
    let identity = sailry_link::Identity::open(path).map_err(backup::io)?;
    if identity.id() != node {
        return Err(Fault::invalid(
            "restored identity differs from its database",
        ));
    }
    let db = Connection::open_with_flags(
        path.join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(storage_error)?;
    super::database::validate_backup(&db, node)
}

fn relocate(staging: &Path, original: &Path, target: &Path) -> Result<(), Fault> {
    let mut db = Connection::open(staging.join("storage/node.sqlite3")).map_err(storage_error)?;
    let transaction = db.transaction().map_err(storage_error)?;
    for table in ["projects", "worktrees"] {
        let rows = {
            let mut query = transaction
                .prepare(&format!("SELECT id,path FROM {table}"))
                .map_err(storage_error)?;
            query
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(storage_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(storage_error)?
        };
        for (id, value) in rows {
            if let Some(value) = relocated(&value, original, target)? {
                transaction
                    .execute(
                        &format!("UPDATE {table} SET path=?1 WHERE id=?2"),
                        rusqlite::params![value, id],
                    )
                    .map_err(storage_error)?;
            }
        }
    }
    let rows = {
        let mut query = transaction
            .prepare("SELECT id,body FROM knowledge_collections")
            .map_err(storage_error)?;
        query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?
    };
    for (id, body) in rows {
        let mut stored: super::knowledge::Stored =
            serde_json::from_slice(&body).map_err(storage_error)?;
        match &mut stored.collection.definition.source {
            sailry_protocol::knowledge::Source::File { path }
            | sailry_protocol::knowledge::Source::Directory { path } => {
                if let Some(value) = relocated(path, original, target)? {
                    *path = value;
                    transaction
                        .execute(
                            "UPDATE knowledge_collections SET body=?1 WHERE id=?2",
                            rusqlite::params![encode(&stored)?, id],
                        )
                        .map_err(storage_error)?;
                }
            }
            _ => {}
        }
    }
    transaction.commit().map_err(storage_error)
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
