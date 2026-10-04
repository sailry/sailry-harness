//! Reuses the existing Source, Client streams and atomic download destination.
//! Adapted from the SSH transfer helpers at ef5e82ea (Apache-2.0).
use super::*;
use crate::resources::file_source::Source as FileSource;
use sailry_protocol::{Command, EntryKind, Output, Request, RequestOutcome, ssh};
use std::path::PathBuf;

#[derive(Clone)]
pub(super) struct Access {
    pub client: Arc<sailry_client::Client>,
    pub context: sailry_protocol::plugin::Context,
    pub profile: ssh::Profile,
}
impl Access {
    fn request(&self, command: Command) -> Request {
        self.client
            .prepare(command)
            .with_plugin(self.context.clone())
    }
    async fn execute(&self, command: Command) -> Result<Output, Fault> {
        self.client.execute(self.request(command)).await
    }
}

#[derive(Clone)]
pub(super) struct Entry {
    local: PathBuf,
    remote: String,
    directory: bool,
    size: u64,
}
#[derive(Clone, Default)]
pub(super) struct Plan {
    entries: Vec<Entry>,
    index: usize,
    pending: Option<Request>,
    copied: u64,
    total: u64,
    overwrite: bool,
}

impl Plan {
    pub(super) fn recoverable(&self) -> bool {
        self.pending.is_some()
    }
}

pub(super) struct Destination {
    pub path: String,
    pub overwrite: bool,
}

pub(super) async fn upload(
    access: Access,
    source: PathBuf,
    destination: Destination,
    mut plan: Plan,
    check: bool,
    cancel: CancellationToken,
    updates: tokio::sync::watch::Sender<Status>,
) -> (Plan, Status) {
    let Destination {
        path: remote,
        overwrite,
    } = destination;
    plan.overwrite |= overwrite;
    let result = async {
        if check && plan.pending.is_none() {
            return Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "original upload receipt is unavailable",
            ));
        }
        if let Some(request) = &plan.pending {
            if !check {
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "upload publication is unconfirmed",
                ));
            }
            match access.client.outcome(request).await? {
                RequestOutcome::Completed(result) => {
                    let result = *result;
                    if !matches!(&result, Err(error) if error.code == ErrorCode::OutcomeUnknown) {
                        plan.pending = None;
                    }
                    match result? {
                        Output::SshOutcome(
                            ssh::Outcome::Transferred { .. } | ssh::Outcome::FilesChanged,
                        ) => {}
                        _ => return Err(failed("files_upload_failed")),
                    }
                    plan.pending = None;
                    plan.copied += plan.entries[plan.index].size;
                    plan.index += 1;
                }
                RequestOutcome::NotAdmitted => {
                    if let Command::FinishSshUpload { stream, .. } = &request.command {
                        release(&access, *stream).await;
                    }
                    plan.pending = None;
                    return Err(failed("upload publication was not admitted"));
                }
                _ => {
                    return Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "upload publication is unconfirmed",
                    ));
                }
            }
        }
        if plan.entries.is_empty() {
            let stop = cancel.clone();
            plan.entries = tokio::task::spawn_blocking(move || scan(source, remote, stop))
                .await
                .map_err(|_| failed("files_upload_source"))??;
            plan.total = plan.entries.iter().map(|entry| entry.size).sum();
        }
        while let Some(entry) = plan.entries.get(plan.index) {
            if cancel.is_cancelled() {
                return Err(cancelled());
            }
            let profile = &access.profile;
            let request = if entry.directory {
                access.request(Command::ModifySshFile {
                    profile: profile.id,
                    expected_revision: profile.revision,
                    path: entry.remote.clone(),
                    action: ssh::FileAction::Create { directory: true },
                })
            } else {
                let local = entry.local.clone();
                let stop = cancel.clone();
                let source = tokio::task::spawn_blocking(move || {
                    FileSource::open_limited(&local, &stop, 1024 * 1024 * 1024)
                })
                .await
                .map_err(|_| failed("files_upload_source"))?
                .map_err(failed)?;
                let spec = ssh::UploadSpec {
                    profile: profile.id,
                    expected_revision: profile.revision,
                    path: entry.remote.clone(),
                    size: source.size,
                    revision: source.revision,
                    overwrite: plan.overwrite,
                };
                let Output::SshUpload(upload) = access
                    .execute(Command::StageSshUpload(spec.clone()))
                    .await?
                else {
                    return Err(failed("files_upload_failed"));
                };
                if upload.spec != spec {
                    release(&access, upload.stream).await;
                    return Err(failed("files_upload_failed"));
                }
                let mut reader = tokio::fs::File::from_std(source.file);
                let sent = access
                    .client
                    .upload_ssh(&upload, &mut reader, cancel.clone(), |copied| {
                        updates.send_replace(Status::Sending(plan.copied + copied, plan.total));
                    })
                    .await;
                if let Err(error) = sent {
                    release(&access, upload.stream).await;
                    return Err(error);
                }
                if cancel.is_cancelled() {
                    release(&access, upload.stream).await;
                    return Err(cancelled());
                }
                access.request(Command::FinishSshUpload {
                    profile: profile.id,
                    expected_revision: profile.revision,
                    path: spec.path,
                    stream: upload.stream,
                })
            };
            updates.send_replace(Status::Publishing);
            plan.pending = Some(request.clone());
            let result = access.client.execute(request).await;
            match result {
                Ok(Output::SshOutcome(
                    ssh::Outcome::Transferred { .. } | ssh::Outcome::FilesChanged,
                )) => {}
                Ok(Output::SshOutcome(ssh::Outcome::HostKeyRequired { .. })) => {
                    plan.pending = None;
                    return Err(failed("ssh_key_changed"));
                }
                Err(error)
                    if error.code == ErrorCode::Conflict && entry.directory && plan.overwrite =>
                {
                    plan.pending = None;
                    if !matches!(
                        access
                            .execute(Command::BrowseSshDirectory {
                                profile: profile.id,
                                expected_revision: profile.revision,
                                path: entry.remote.clone(),
                                after: None
                            })
                            .await?,
                        Output::SshOutcome(ssh::Outcome::Directory(_))
                    ) {
                        return Err(failed("files_upload_failed"));
                    }
                }
                Err(error)
                    if matches!(
                        error.code,
                        ErrorCode::OutcomeUnknown | ErrorCode::Unavailable
                    ) =>
                {
                    return Err(Fault::new(ErrorCode::OutcomeUnknown, error.message));
                }
                Err(error) => {
                    plan.pending = None;
                    return Err(error);
                }
                _ => {
                    plan.pending = None;
                    return Err(failed("files_upload_failed"));
                }
            }
            plan.pending = None;
            plan.copied += entry.size;
            plan.index += 1;
        }
        Ok(())
    }
    .await;
    let status = match result {
        Ok(()) => Status::Done,
        Err(error) if plan.pending.is_some() => {
            Status::Failed(Fault::new(ErrorCode::OutcomeUnknown, error.message))
        }
        Err(error) if error.code == ErrorCode::Conflict => Status::Exists,
        Err(error) => Status::Failed(error),
    };
    (plan, status)
}

fn scan(source: PathBuf, remote: String, stop: CancellationToken) -> Result<Vec<Entry>, Fault> {
    let mut pending = vec![(source, remote)];
    let mut result = Vec::new();
    while let Some((local, remote)) = pending.pop() {
        if stop.is_cancelled() {
            return Err(cancelled());
        }
        let metadata =
            std::fs::symlink_metadata(&local).map_err(|_| failed("files_upload_source"))?;
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&local).map_err(|_| failed("files_upload_source"))? {
                let entry = entry.map_err(|_| failed("files_upload_source"))?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| failed("files_upload_name"))?;
                pending.push((entry.path(), join(&remote, &name)));
            }
        } else if !metadata.is_file() {
            return Err(failed("files_upload_source"));
        }
        result.push(Entry {
            local,
            remote,
            directory: metadata.is_dir(),
            size: if metadata.is_file() {
                metadata.len()
            } else {
                0
            },
        });
    }
    Ok(result)
}

pub(super) async fn download(
    access: Access,
    remote: String,
    local: PathBuf,
    directory: bool,
    cancel: CancellationToken,
    updates: tokio::sync::watch::Sender<Status>,
) -> Status {
    let result=async {
        let mut pending=vec![(remote,local,directory)];let mut files=Vec::new();
        while let Some((remote,local,directory))=pending.pop(){
            if cancel.is_cancelled(){return Err(cancelled());}
            if directory {
                match tokio::fs::symlink_metadata(&local).await {
                    Ok(metadata) if metadata.is_dir()=>{},
                    Err(error) if error.kind()==std::io::ErrorKind::NotFound=>tokio::fs::create_dir(&local).await.map_err(|_|failed("files_download_destination"))?,
                    _=>return Err(failed("files_download_destination")),
                }
                let mut after=None;
                loop {
                    let Output::SshOutcome(ssh::Outcome::Directory(list))=access.execute(Command::BrowseSshDirectory {profile:access.profile.id,expected_revision:access.profile.revision,path:remote.clone(),after}).await? else {return Err(failed("files_download_failed"));};
                    for entry in list.entries {
                        if entry.name.contains(['/', '\\','\0'])||matches!(entry.name.as_str(),"."|"..") {return Err(failed("files_download_failed"));}
                        pending.push((join(&remote,&entry.name),local.join(&entry.name),entry.kind==EntryKind::Directory));
                    }
                    after=list.next;if after.is_none(){break;}
                }
            } else {files.push((remote,local));}
        }
        let count=files.len().max(1) as u64;
        for (index,(remote,local)) in files.into_iter().enumerate(){
            use crate::downloads::transfer::{self,Status as Download};
            let (send,mut changes)=tokio::sync::watch::channel(Download::Preparing);
            let run=transfer::run(access.client.clone(),transfer::Source::ScopedSsh{context:access.context.clone(),profile:access.profile.clone(),path:remote},local,cancel.clone(),send);
            tokio::pin!(run);
            loop {
                tokio::select! {
                    _=&mut run=>{if let Download::Finished(result)=changes.borrow().clone(){result.map_err(failed)?;break;}else{return Err(failed("files_download_failed"));}},
                    changed=changes.changed()=>{changed.map_err(|_|failed("files_download_failed"))?;match changes.borrow_and_update().clone(){
                        Download::Receiving{copied,size}=>{updates.send_replace(Status::Sending(index as u64*1000+if size==0{1000}else{((copied as u128*1000)/(size as u128))as u64},count*1000));},
                        Download::Publishing=>{updates.send_replace(Status::Publishing);},
                        Download::Finished(result)=>{result.map_err(failed)?;break;},_=>{},
                    }},
                }
            }
        }Ok(())
    }.await;
    match result {
        Ok(()) => Status::Done,
        Err(error) => Status::Failed(error),
    }
}
async fn release(access: &Access, stream: sailry_protocol::StreamId) {
    let _ = access
        .client
        .execute(
            access
                .client
                .prepare(Command::CancelFileTransfer { stream }),
        )
        .await;
}
pub(super) fn failed(message: impl Into<String>) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}
pub(super) fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "file transfer was cancelled")
}
pub(super) fn join(directory: &str, name: &str) -> String {
    format!("{}/{}", directory.trim_end_matches('/'), name)
}
