//! Native chosen file capabilities for the packaged SSH controller.
//! Core owns local paths, authenticated streams and original durable publication IDs.
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
        .function("uploadDropped",move|args|{
            let profile=serde_json::from_value(decode(args.value(0)?)?).map_err(|error|HostError::new(error.to_string()))?;
            let directory=args.string(1)?.to_owned();let handle=args.string(2)?;
            let (access,_)=access(&drop_owner,&drop_host,profile,true,true)?;
            let paths=drops.take(handle)?;
            let ids=gpui_shell::with_current_app(|cx|drop_store.update(cx,|store,_|{
                let mut ids=Vec::new();for source in paths {let name=source.file_name().and_then(|name|name.to_str()).ok_or_else(missing)?.to_owned();let path=worker::join(&directory,&name);ids.push(store.insert(access.clone(),Source::Upload(source),path));}Ok::<_,HostError>(ids)
            })).ok_or_else(missing)??;
            encode(json!(ids))
        })
        .async_function("selectUpload",move|args|{
            let profile=serde_json::from_value(decode(args.value(0)?)?).map_err(|error|HostError::new(error.to_string()))?;
            let directory=args.string(1)?.to_owned();let prompt=args.string(2)?.to_owned();let (access,_)=access(&select_owner,&select_host,profile,true,true)?;
            let selected=gpui_shell::with_current_app(|cx|cx.prompt_for_paths(PathPromptOptions{files:true,directories:true,multiple:true,prompt:Some(prompt.into())})).ok_or_else(missing)?;
            let stop=select_host.stop_token();let store=select_store.clone();let (reply,receive)=tokio::sync::oneshot::channel();
            gpui_shell::with_current_app(|cx|{cx.spawn(async move|cx|{
                let result=match selected.await{Ok(Ok(Some(paths))) if !stop.is_cancelled()=>store.update(cx,|store,_|{
                    let mut ids=Vec::new();for source in paths {let name=source.file_name().and_then(|name|name.to_str()).ok_or_else(missing)?.to_owned();let path=worker::join(&directory,&name);ids.push(store.insert(access.clone(),Source::Upload(source),path));}Ok::<_,HostError>(ids)
                }).map_err(|_|missing()),Ok(Ok(None))=>Ok(Vec::new()),_=>Err(missing())};let _=reply.send(result);
            }).detach();}).ok_or_else(missing)?;
            Ok(async move{encode(json!(receive.await.map_err(|_|missing())??))})
        })
        .async_function("selectDownload",move|args|{
            #[derive(serde::Deserialize)]struct Entry{path:String,directory:bool}
            let profile=serde_json::from_value(decode(args.value(0)?)?).map_err(|error|HostError::new(error.to_string()))?;let entries:Vec<Entry>=serde_json::from_value(decode(args.value(1)?)?).map_err(|error|HostError::new(error.to_string()))?;let prompt=args.string(2)?.to_owned();let(access,_)=access(&download_owner,&download_host,profile,false,true)?;if entries.is_empty(){return Err(missing());}
            let stop=download_host.stop_token();let store=download_store.clone();let(reply,receive)=tokio::sync::oneshot::channel();
            gpui_shell::with_current_app(|cx|{
                if entries.len()==1 {let entry=entries.into_iter().next().unwrap();let selected=cx.prompt_for_new_path(&std::env::home_dir().unwrap_or_default(),entry.path.rsplit('/').next());
                    cx.spawn(async move|cx|{let result=match selected.await{Ok(Ok(Some(path))) if !stop.is_cancelled()=>store.update(cx,|store,_|Ok::<_,HostError>(vec![store.insert(access,Source::Download(path,entry.directory),entry.path)])).map_err(|_|missing()),Ok(Ok(None))=>Ok(Vec::new()),_=>Err(missing())};let _=reply.send(result);}).detach();
                }else{let selected=cx.prompt_for_paths(PathPromptOptions{files:false,directories:true,multiple:false,prompt:Some(prompt.into())});cx.spawn(async move|cx|{let result=match selected.await{Ok(Ok(Some(paths))) if !stop.is_cancelled()=>store.update(cx,|store,_|{let root=paths.first().ok_or_else(missing)?;let mut ids=Vec::new();for entry in entries{let name=entry.path.rsplit('/').next().filter(|name|!name.is_empty()&&!matches!(*name,"."|"..")&&!name.contains('\\')).ok_or_else(missing)?;ids.push(store.insert(access.clone(),Source::Download(root.join(name),entry.directory),entry.path));}Ok::<_,HostError>(ids)}).map_err(|_|missing()),Ok(Ok(None))=>Ok(Vec::new()),_=>Err(missing())};let _=reply.send(result);}).detach();}
            }).ok_or_else(missing)?;Ok(async move{encode(json!(receive.await.map_err(|_|missing())??))})
        })
        .function("transferAction",move|args|{
            action_host.check()?;let id=args.string(0)?.parse().map_err(HostError::new)?;let kind=args.string(1)?.to_owned();
            // Kit scopes reject a nested mutable App borrow. Finish the job
            // lookup before validating access, then mutate in a fresh scope.
            let (profile,node,package,write)=gpui_shell::with_current_app(|cx|{
                let state=actions.read(cx);let job=state.jobs.get(&id).ok_or_else(missing)?;
                Ok::<_,HostError>((job.access.profile.clone(),job.access.client.target(),job.access.context.package.clone(),matches!(job.source,Source::Upload(_))))
            }).ok_or_else(missing)??;
            let(access,runtime)=access(&action_owner,&action_host,profile,write,matches!(kind.as_str(),"start"|"replace"))?;
            if node!=access.client.target()||package!=access.context.package{return Err(missing());}
            gpui_shell::with_current_app(|cx|actions.update(cx,|state,cx|state.action(id,&kind,runtime,cx))).ok_or_else(missing)??;
            Ok(HostValue::Null)
        })
        .async_function("nextTransfers",move|args|{
            read_host.check()?;let seen=args.string(0)?.to_owned();let mut receiver=gpui_shell::with_current_app(|cx|store.read(cx).changes.subscribe()).ok_or_else(missing)?;let package=read_host.context().package.clone();let node=read_host.target();let stop=read_host.stop_token();
            Ok(async move{loop{let values=receiver.borrow_and_update().clone();let values=values.as_array().map(|values|values.iter().filter(|value|value["package"]==serde_json::to_value(&package).unwrap()&&value["node"]==serde_json::to_value(node).unwrap()).map(|value|value["value"].clone()).collect::<Vec<_>>()).unwrap_or_default();let cursor=blake3::hash(&serde_json::to_vec(&values).map_err(|error|HostError::new(error.to_string()))?).to_hex().to_string();if cursor!=seen{return encode(json!({"cursor":cursor,"transfers":values}));}tokio::select!{biased;_=stop.cancelled()=>return Err(missing()),result=receiver.changed()=>result.map_err(|_|missing())?}}})
        }).declarations(include_str!("api.d.ts"))
}
