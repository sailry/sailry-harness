//! Native chosen file capabilities for the packaged SSH controller.
//! Core owns local paths, authenticated streams and original durable publication IDs.
mod handlers;
#[cfg(test)]
mod tests;
mod worker;
use super::{
    Panel,
    host::{
        Host,
        sdk::values::{decode, encode},
    },
};
use gpui_kit::*;
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, RequestId, ssh};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

#[derive(Clone)]
enum Status {
    Ready,
    Preparing,
    Sending(u64, u64),
    Publishing,
    Done,
    Exists,
    Failed(Fault),
}
enum Source {
    Upload(PathBuf),
    Download(PathBuf, bool),
}
struct Job {
    access: worker::Access,
    source: Source,
    path: String,
    status: Status,
    plan: worker::Plan,
    cancel: CancellationToken,
    busy: bool,
}
impl Job {
    fn snapshot(&self, id: RequestId) -> Value {
        let (stage, error, progress) = match &self.status {
            Status::Ready => ("ready", None, None),
            Status::Preparing => ("preparing", None, None),
            Status::Sending(copied, size) => (
                "transferring",
                None,
                Some(json!({"copied":copied,"size":size})),
            ),
            Status::Publishing => ("publishing", None, None),
            Status::Done => ("done", None, None),
            Status::Exists => ("exists", None, None),
            Status::Failed(error) => (
                if error.code == ErrorCode::OutcomeUnknown {
                    "uncertain"
                } else if error.code == ErrorCode::Cancelled {
                    "cancelled"
                } else {
                    "failed"
                },
                Some(error),
                None,
            ),
        };
        let (kind, source) = match &self.source {
            Source::Upload(path) => (
                "upload",
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            Source::Download(_, directory) => (
                if *directory {
                    "download_directory"
                } else {
                    "download"
                },
                self.path.clone(),
            ),
        };
        json!({"id":id,"profile":self.access.profile.id,"kind":kind,"source":source,"path":self.path,"stage":stage,"error":error,"progress":progress,
            "can_start":!self.busy&&(matches!(self.status,Status::Ready)||matches!(&self.status,Status::Failed(error) if error.code!=ErrorCode::OutcomeUnknown)),"can_replace":!self.busy&&matches!(self.status,Status::Exists),
            "can_check":!self.busy&&self.plan.recoverable()&&matches!(self.status,Status::Failed(Fault{code:ErrorCode::OutcomeUnknown,..})),"can_cancel":self.busy&&!matches!(self.status,Status::Publishing)})
    }
}
struct Store {
    jobs: BTreeMap<RequestId, Job>,
    changes: tokio::sync::watch::Sender<Value>,
    uploads: Arc<tokio::sync::Semaphore>,
}
impl Store {
    fn new() -> Self {
        Self {
            jobs: BTreeMap::new(),
            uploads: Arc::new(tokio::sync::Semaphore::new(2)),
            changes: tokio::sync::watch::channel(Value::Null).0,
        }
    }
    fn publish(&self) {
        self.changes.send_replace(json!(self.jobs.iter().map(|(id,job)|json!({"node":job.access.client.target(),"package":job.access.context.package,"value":job.snapshot(*id)})).collect::<Vec<_>>()));
    }
    fn insert(&mut self, access: worker::Access, source: Source, path: String) -> RequestId {
        let exists = matches!(&source,Source::Download(path,_) if path.exists());
        let id = RequestId::new();
        self.jobs.insert(
            id,
            Job {
                access,
                source,
                path,
                status: if exists {
                    Status::Exists
                } else {
                    Status::Ready
                },
                plan: Default::default(),
                cancel: CancellationToken::new(),
                busy: false,
            },
        );
        self.publish();
        id
    }
    fn action(
        &mut self,
        id: RequestId,
        kind: &str,
        runtime: tokio::runtime::Handle,
        cx: &mut Context<Self>,
    ) -> Result<(), HostError> {
        let job = self.jobs.get_mut(&id).ok_or_else(missing)?;
        if kind == "cancel" {
            job.cancel.cancel();
            return Ok(());
        }
        if kind == "dismiss" {
            if !job.busy {
                self.jobs.remove(&id);
                self.publish();
            }
            return Ok(());
        }
        let check = kind == "check";
        let replace = kind == "replace";
        let allowed = match &job.status {
            Status::Ready => kind == "start",
            Status::Exists => replace,
            Status::Failed(error) => {
                if error.code == ErrorCode::OutcomeUnknown {
                    check && job.plan.recoverable()
                } else {
                    kind == "start"
                }
            }
            _ => false,
        };
        if job.busy || !allowed {
            return Err(HostError::new("file transfer action is unavailable"));
        }
        job.cancel = CancellationToken::new();
        job.busy = true;
        job.status = Status::Preparing;
        let access = job.access.clone();
        let path = job.path.clone();
        let cancel = job.cancel.clone();
        let plan = std::mem::take(&mut job.plan);
        let (updates, mut changes) = tokio::sync::watch::channel(Status::Preparing);
        let run = match &job.source {
            Source::Upload(source) => {
                let source = source.clone();
                let uploads = self.uploads.clone();
                runtime.spawn(async move {
                    let _permit = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => return (plan, Status::Failed(worker::cancelled())),
                        permit = uploads.acquire_owned() => match permit {
                            Ok(permit) => permit,
                            Err(_) => return (plan, Status::Failed(worker::failed("file transfer pool is unavailable"))),
                        },
                    };
                    worker::upload(access, source, worker::Destination { path, overwrite: replace }, plan, check, cancel, updates).await
                })
            }
            Source::Download(destination, directory) => {
                let destination = destination.clone();
                let directory = *directory;
                runtime.spawn(async move {
                    (
                        plan,
                        worker::download(access, path, destination, directory, cancel, updates)
                            .await,
                    )
                })
            }
        };
        self.publish();
        cx.spawn(async move |this, cx| {
            while changes.changed().await.is_ok() {
                let status = changes.borrow_and_update().clone();
                let _ = this.update(cx, |this, _| {
                    if let Some(job) = this.jobs.get_mut(&id) {
                        job.status = status;
                    }
                    this.publish();
                });
            }
            let result = run.await.unwrap_or_else(|_| {
                (
                    Default::default(),
                    Status::Failed(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "file transfer worker stopped before confirming completion",
                    )),
                )
            });
            let _ = this.update(cx, |this, _| {
                if let Some(job) = this.jobs.get_mut(&id) {
                    job.plan = result.0;
                    job.status = result.1;
                    job.busy = false;
                }
                this.publish();
            });
        })
        .detach();
        Ok(())
    }
}
struct Shared(Entity<Store>);
impl Global for Shared {}
fn store(cx: &mut App) -> Entity<Store> {
    if let Some(shared) = cx.try_global::<Shared>() {
        shared.0.clone()
    } else {
        let store = cx.new(|_| Store::new());
        cx.set_global(Shared(store.clone()));
        store
    }
}
fn missing() -> HostError {
    HostError::new("SSH file transfer is unavailable")
}
fn access(
    owner: &WeakEntity<Panel>,
    host: &Host,
    profile: ssh::Profile,
    write: bool,
    current: bool,
) -> Result<(worker::Access, tokio::runtime::Handle), HostError> {
    host.check()?;
    gpui_shell::with_current_app(|cx| {
        owner
            .read_with(cx, |panel, cx| {
                let required = if write {
                    sailry_protocol::plugin::Action::ControlSsh
                } else {
                    sailry_protocol::plugin::Action::ReadSsh
                };
                if panel.selected.as_ref() != Some(&host.context().package)
                    || !panel.connected
                    || !panel
                        .metadata
                        .read(cx)
                        .entries
                        .get(&host.context().package.name)
                        .and_then(|info| info.extension.as_ref())
                        .is_some_and(|extension| extension.actions.contains(&required))
                    || (current
                        && !panel
                            .snapshot
                            .borrow()
                            .as_ref()
                            .is_some_and(|snapshot| snapshot.ssh.contains(&profile)))
                {
                    return Err(missing());
                }
                Ok((
                    worker::Access {
                        client: panel.binding.client.clone(),
                        context: host.context().clone(),
                        profile,
                    },
                    panel.binding.runtime.handle().clone(),
                ))
            })
            .map_err(|_| missing())?
    })
    .ok_or_else(missing)?
}

pub(super) fn module(
    owner: WeakEntity<Panel>,
    host: Arc<Host>,
    drops: super::tree::drops::Drops,
    cx: &mut App,
) -> HostModule {
    let store = store(cx);
    let drop_store = store.clone();
    let drop_owner = owner.clone();
    let drop_host = host.clone();
    let select_store = store.clone();
    let select_owner = owner.clone();
    let select_host = host.clone();
    let download_store = store.clone();
    let download_owner = owner.clone();
    let download_host = host.clone();
    let actions = store.clone();
    let action_host = host.clone();
    let action_owner = owner;
    let read_host = host;
    HostModule::new("sailry/ssh-transfers")
        .function("uploadDropped", move |args| {
            handlers::upload_dropped(args, &drop_owner, &drop_host, &drop_store, &drops)
        })
        .async_function("selectUpload", move |args| {
            handlers::select_upload(args, &select_owner, &select_host, &select_store)
        })
        .async_function("selectDownload", move |args| {
            handlers::select_download(args, &download_owner, &download_host, &download_store)
        })
        .function("transferAction", move |args| {
            handlers::transfer_action(args, &action_owner, &action_host, &actions)
        })
        .async_function("nextTransfers", move |args| {
            handlers::next_transfers(args, &read_host, &store)
        })
        .declarations(include_str!("api.d.ts"))
}
