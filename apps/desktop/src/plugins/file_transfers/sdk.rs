use super::*;
use crate::plugins::host::{
    Host,
    sdk::values::{decode, encode},
};
use gpui_shell::{HostError, HostModule};

pub(in crate::plugins) struct Scope {
    pub binding: Binding,
    pub documents: Option<Entity<super::super::documents::Controller>>,
    pub read: bool,
    pub write: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    kind: String,
    path: Option<String>,
}

enum Operation {
    Capture(Vec<String>, bool),
    Paste(RequestId, String, String),
    Move(String, String),
    Upload(String),
    Download(String),
    Open(String),
    Action(RequestId, Action),
}

pub(in crate::plugins) fn module(scope: Scope, host: Arc<Host>, cx: &mut App) -> HostModule {
    let controller = registry(cx);
    if let Some(document) = scope.documents {
        controller.update(cx, |controller, _| {
            if !controller
                .documents
                .iter()
                .any(|existing| existing.entity_id() == document.entity_id())
            {
                controller.documents.push(document.downgrade());
            }
        });
    }
    let access = scope
        .binding
        .worktree
        .filter(|worktree| {
            Some(*worktree) == host.context().worktree
                && scope.read
                && host.context().surface == plugin::desktop::Surface::Workspace
        })
        .map(|_| Access {
            client: scope.binding.client.clone(),
            runtime: scope.binding.runtime.clone(),
            context: host.context().clone(),
            label: format!("{} / {}", scope.binding.host, scope.binding.project_name),
        });
    let context = Arc::new((controller, access, host, scope.write));
    let capture = context.clone();
    let clipboard = context.clone();
    let paste = context.clone();
    let moving = context.clone();
    let upload = context.clone();
    let download = context.clone();
    let open = context.clone();
    let action = context.clone();
    let read = context.clone();
    let next = context;
    HostModule::new("sailry/file-transfers")
        .async_function("captureClipboard",move |args| {
            let paths=serde_json::from_value(decode(args.value(0)?)?).map_err(|error|HostError::new(error.to_string()))?;
            let cut=args.value(1)?.as_bool().ok_or_else(missing)?;
            send(&capture,Operation::Capture(paths,cut),cut).map(complete)
        })
        .function("readClipboard",move |_| {
            let access=checked(&clipboard,false)?;
            gpui_shell::with_current_app(|cx| encode(clipboard.0.read(cx).clipboard.as_ref().map_or(Value::Null,|value|value.snapshot(&access)))).ok_or_else(missing)?
        })
        .async_function("preparePaste",move |args| {let id=args.string(0)?.parse().map_err(HostError::new)?;send(&paste,Operation::Paste(id,args.string(1)?.into(),args.string(2)?.into()),true).map(complete)})
        .async_function("prepareMove",move |args|send(&moving,Operation::Move(args.string(0)?.into(),args.string(1)?.into()),true).map(complete))
        .async_function("selectUpload",move |args|send(&upload,Operation::Upload(args.string(0)?.into()),true).map(complete))
        .async_function("startDownload",move |args|send(&download,Operation::Download(args.string(0)?.into()),false).map(complete))
        .async_function("openSystem",move |args|send(&open,Operation::Open(args.string(0)?.into()),false).map(complete))
        .async_function("transferAction",move |args| {
            let id=args.string(0)?.parse().map_err(HostError::new)?;
            let value=serde_json::from_value(decode(args.value(1)?)?).map_err(|error|HostError::new(error.to_string()))?;
            send(&action,Operation::Action(id,value),false).map(complete)
        })
        .function("readTransfers",move |_| {let access=checked(&read,false)?;gpui_shell::with_current_app(|cx|encode(read.0.read(cx).snapshot(&access))).ok_or_else(missing)?})
        .async_function("nextTransferChange",move |args| {
            let access=checked(&next,false)?;
            let seen=args.string(0)?.to_owned();
            let controller=next.0.clone();
            let mut changes=gpui_shell::with_current_app(|cx|controller.read(cx).changes.subscribe()).ok_or_else(missing)?;
            let stop=next.2.stop_token();
            Ok(async move {
                loop {
                    let snapshot=changes.borrow_and_update().clone();
                    if snapshot.cursor.to_string()!=seen {
                        return encode(snapshot.for_access(&access));
                    }
                    tokio::select! {biased;_ = stop.cancelled()=>return Err(missing()),changed=changes.changed()=>changed.map_err(|_|missing())?}
                }
            })
        }).declarations(include_str!("api.d.ts"))
}

type Mount = (Entity<Controller>, Option<Access>, Arc<Host>, bool);
fn checked(context: &Mount, write: bool) -> Result<Access, HostError> {
    context.2.check()?;
    if write && !context.3 {
        return Err(HostError::new("file action is read-only"));
    }
    context.1.clone().ok_or_else(missing)
}
fn send(
    context: &Mount,
    operation: Operation,
    write: bool,
) -> Result<tokio::sync::oneshot::Receiver<Result<Value, Fault>>, HostError> {
    let access = checked(context, write)?;
    let controller = context.0.clone();
    let stop = context.2.stop_token();
    let write = context.3;
    let (reply, receive) = tokio::sync::oneshot::channel();
    gpui_shell::with_current_app(|cx| {
        cx.defer(move |cx| {
            controller.update(cx, |controller, cx| {
                controller.request(access, operation, stop, write, reply, cx)
            })
        })
    })
    .ok_or_else(missing)?;
    Ok(receive)
}
async fn complete(
    receive: tokio::sync::oneshot::Receiver<Result<Value, Fault>>,
) -> Result<gpui_shell::HostValue, HostError> {
    encode(
        receive.await.map_err(|_| missing())?.map_err(|fault| {
            HostError::new(serde_json::to_string(&fault).unwrap_or(fault.message))
        })?,
    )
}
fn missing() -> HostError {
    HostError::new("file transfer scope is unavailable")
}

impl Controller {
    fn request(
        &mut self,
        access: Access,
        operation: Operation,
        stop: CancellationToken,
        write: bool,
        reply: Reply,
        cx: &mut Context<Self>,
    ) {
        if stop.is_cancelled() {
            let _ = reply.send(Err(unavailable()));
            return;
        }
        match operation {
            Operation::Move(source, path) => {
                let result = (|| {
                    if !write {
                        return Err(denied());
                    }
                    valid_path(&source, false)?;
                    // The scoped move has no clipboard capability or cross-Node path.
                    // Entry type is resolved by the ordinary Node rename operation.
                    self.insert(
                        access.clone(),
                        worker::Source::Entry {
                            access,
                            path: source,
                            kind: sailry_protocol::EntryKind::Other,
                            cut: true,
                        },
                        path,
                        None,
                        cx,
                    )
                })();
                let _ = reply.send(result);
            }
            Operation::Capture(paths, cut) => {
                if cut && !write {
                    let _ = reply.send(Err(denied()));
                    return;
                }
                self.generation += 1;
                let generation = self.generation;
                let target = access.clone();
                let stopping = stop.clone();
                let job = access.runtime.clone().spawn(async move {
                    access.verify(cut).await?;
                    let mut selected = Vec::new();
                    for path in paths {
                        valid_path(&path, false)?;
                        if stopping.is_cancelled() {
                            return Err(cancelled());
                        }
                        let (parent, name) = path.rsplit_once('/').unwrap_or(("", &path));
                        let mut after = None;
                        let kind = loop {
                            let Output::Directory(page) = access
                                .execute(Command::ListDirectory {
                                    worktree: access.worktree(),
                                    path: parent.into(),
                                    after,
                                })
                                .await?
                            else {
                                return Err(internal());
                            };
                            if let Some(entry) =
                                page.entries.iter().find(|entry| entry.name == name)
                            {
                                break entry.kind;
                            }
                            after = page.next;
                            if after.is_none() {
                                return Err(Fault::new(
                                    ErrorCode::NotFound,
                                    "clipboard source is unavailable",
                                ));
                            }
                        };
                        if !selected.iter().any(|(entry, _)| entry == &path) {
                            selected.push((path, kind));
                        }
                    }
                    if selected.is_empty() {
                        return Err(Fault::new(
                            ErrorCode::InvalidRequest,
                            "clipboard selection is empty",
                        ));
                    }
                    Ok(Clipboard {
                        id: RequestId::new(),
                        access,
                        paths: selected,
                        cut,
                    })
                });
                cx.spawn(async move |owner, cx| {
                    let result = job.await.unwrap_or(Err(internal()));
                    let result = owner
                        .update(cx, |owner, cx| {
                            if stop.is_cancelled() || owner.generation != generation {
                                return Err(unavailable());
                            }
                            let clipboard = result?;
                            let value = clipboard.snapshot(&target);
                            owner.clipboard = Some(clipboard);
                            owner.changed(cx);
                            Ok(value)
                        })
                        .unwrap_or(Err(unavailable()));
                    let _ = reply.send(result);
                })
                .detach();
            }
            Operation::Paste(id, source, path) => {
                let result = (|| {
                    if !write {
                        return Err(denied());
                    }
                    let clipboard = self
                        .clipboard
                        .as_ref()
                        .filter(|clipboard| clipboard.id == id)
                        .ok_or_else(unavailable)?;
                    let kind = clipboard
                        .paths
                        .iter()
                        .find(|(path, _)| path == &source)
                        .map(|(_, kind)| *kind)
                        .ok_or_else(unavailable)?;
                    if clipboard.access.client.target() != access.client.target()
                        && kind != sailry_protocol::EntryKind::File
                    {
                        return Err(Fault::new(
                            ErrorCode::InvalidRequest,
                            "directories cannot be transferred between Nodes",
                        ));
                    }
                    self.insert(
                        access,
                        worker::Source::Entry {
                            access: clipboard.access.clone(),
                            path: source,
                            kind,
                            cut: clipboard.cut,
                        },
                        path,
                        Some(id),
                        cx,
                    )
                })();
                let _ = reply.send(result);
            }
            Operation::Action(id, action) => {
                let result = (|| {
                    let transfer = self
                        .transfers
                        .get(&id)
                        .filter(|transfer| {
                            transfer.access.location() == access.location()
                                && transfer.access.context.package.name
                                    == access.context.package.name
                        })
                        .ok_or_else(unavailable)?;
                    if transfer.kind() != "download" && !write {
                        return Err(denied());
                    }
                    match action.kind.as_str() {
                        "start" | "replace" | "check" => self.start(
                            id,
                            action.path,
                            action.kind == "replace",
                            action.kind == "check",
                            stop,
                            cx,
                        ),
                        "cancel"
                            if matches!(transfer.stage, "ready" | "preparing" | "transferring") =>
                        {
                            let transfer = self.transfers.get_mut(&id).unwrap();
                            transfer.cancel.cancel();
                            if transfer.stage == "ready" {
                                transfer.stage = "cancelled";
                            }
                            let value = transfer.snapshot(id);
                            self.changed(cx);
                            Ok(value)
                        }
                        "dismiss" if !transfer.active() && transfer.stage != "uncertain" => {
                            self.transfers.remove(&id);
                            self.changed(cx);
                            Ok(Value::Null)
                        }
                        _ => Err(unavailable()),
                    }
                })();
                let _ = reply.send(result);
            }
            Operation::Upload(directory) => {
                if !write || valid_path(&directory, true).is_err() {
                    let _ = reply.send(Err(denied()));
                    return;
                }
                let selected = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: None,
                });
                cx.spawn(async move |owner, cx| {
                    let selected = selected.await;
                    let result = owner
                        .update(cx, |owner, cx| {
                            if stop.is_cancelled() {
                                return Err(unavailable());
                            }
                            let path = match selected {
                                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                                Ok(Ok(None)) => None,
                                _ => return Err(unavailable()),
                            };
                            let Some(path) = path else {
                                return Ok(Value::Null);
                            };
                            let name = path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .ok_or_else(unavailable)?;
                            let destination = if directory.is_empty() {
                                name.into()
                            } else {
                                format!("{directory}/{name}")
                            };
                            owner.insert(
                                access,
                                worker::Source::Upload(path),
                                destination,
                                None,
                                cx,
                            )
                        })
                        .unwrap_or(Err(unavailable()));
                    let _ = reply.send(result);
                })
                .detach();
            }
            Operation::Download(path) => {
                if valid_path(&path, false).is_err() {
                    let _ = reply.send(Err(unavailable()));
                    return;
                }
                let selected = cx.prompt_for_new_path(
                    &std::env::home_dir().unwrap_or_default(),
                    path.rsplit('/').next(),
                );
                cx.spawn(async move |owner, cx| {
                    let selected = selected.await;
                    let result = owner
                        .update(cx, |owner, cx| {
                            if stop.is_cancelled() {
                                return Err(unavailable());
                            }
                            let destination = match selected {
                                Ok(Ok(Some(path))) => path,
                                Ok(Ok(None)) => return Ok(Value::Null),
                                _ => return Err(unavailable()),
                            };
                            let value = owner.insert(
                                access,
                                worker::Source::Download(destination),
                                path,
                                None,
                                cx,
                            )?;
                            let id = serde_json::from_value(value["id"].clone())
                                .map_err(|_| internal())?;
                            owner.start(id, None, false, false, stop, cx)
                        })
                        .unwrap_or(Err(unavailable()));
                    let _ = reply.send(result);
                })
                .detach();
            }
            Operation::Open(path) => {
                let local = cx.global::<crate::backend::Services>().local.target()
                    == access.client.target();
                let stopping = stop.clone();
                let job = access
                    .runtime
                    .clone()
                    .spawn(system::prepare(access, path, local, stopping));
                cx.spawn(async move |owner, cx| {
                    let result = job.await.unwrap_or(Err(internal()));
                    let result = owner
                        .update(cx, |owner, cx| {
                            if stop.is_cancelled() {
                                return Err(unavailable());
                            }
                            let opened = result?;
                            if let Some(copy) = opened.copy {
                                owner.copies.push(copy);
                            }
                            cx.open_with_system(&opened.path);
                            Ok(Value::Null)
                        })
                        .unwrap_or(Err(unavailable()));
                    let _ = reply.send(result);
                })
                .detach();
            }
        }
    }
}

pub(in crate::plugins) fn open_captured(
    binding: &Binding,
    host: &Arc<Host>,
    path: String,
    cx: &mut App,
) -> tokio::sync::oneshot::Receiver<Result<Value, Fault>> {
    let (reply, receive) = tokio::sync::oneshot::channel();
    if binding.worktree.is_none() || binding.worktree != host.context().worktree {
        let _ = reply.send(Err(unavailable()));
        return receive;
    }
    let access = Access {
        client: binding.client.clone(),
        runtime: binding.runtime.clone(),
        context: host.context().clone(),
        label: format!("{} / {}", binding.host, binding.project_name),
    };
    registry(cx).update(cx, |controller, cx| {
        controller.request(
            access,
            Operation::Open(path),
            host.stop_token(),
            false,
            reply,
            cx,
        )
    });
    receive
}
