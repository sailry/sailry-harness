//! Native file capabilities retain selected owners, drafts and durable receipts.
//! Package code chooses actions and renders status; it never chooses transports.
pub(crate) mod recovery;
mod sdk;
pub(crate) mod system;
#[cfg(test)]
mod tests;
mod worker;

use crate::conversation::live::Binding;
use gpui_kit::*;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, RequestId, WorktreeId, plugin};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

pub(super) use sdk::{Scope, module, open_captured};
type Location = (NodeId, WorktreeId);
type Reply = tokio::sync::oneshot::Sender<Result<Value, Fault>>;

#[derive(Clone)]
struct Access {
    client: Arc<sailry_client::Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    context: plugin::Context,
    label: String,
}
impl Access {
    fn worktree(&self) -> WorktreeId {
        self.context.worktree.expect("captured file worktree")
    }
    fn location(&self) -> Location {
        (self.client.target(), self.worktree())
    }
    fn request(&self, command: Command) -> sailry_protocol::Request {
        self.client
            .prepare(command)
            .with_plugin(self.context.clone())
    }
    async fn execute(&self, command: Command) -> Result<Output, Fault> {
        self.client.execute(self.request(command)).await
    }
    async fn verify(&self, write: bool) -> Result<(), Fault> {
        let Output::Plugin(package) = self
            .client
            .execute(self.client.prepare(Command::ReadPlugin {
                name: self.context.package.name.clone(),
            }))
            .await?
        else {
            return Err(internal());
        };
        if !package.summary.enabled {
            return Err(Fault::new(ErrorCode::NotConfigured, "plugin is disabled"));
        }
        if package.summary.reference() != self.context.package {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "plugin package changed",
            ));
        }
        let action = if write {
            plugin::Action::WriteFiles
        } else {
            plugin::Action::ReadFiles
        };
        if !package
            .extension
            .as_ref()
            .is_some_and(|extension| extension.actions.contains(&action))
        {
            return Err(denied());
        }
        // This also validates the captured session and registered worktree.
        self.execute(Command::ListDirectory {
            worktree: self.worktree(),
            path: String::new(),
            after: None,
        })
        .await?;
        if write && let Some(session) = self.context.session {
            let Output::Session(session) = self
                .client
                .execute(self.client.prepare(Command::ReadSession { session }))
                .await?
            else {
                return Err(internal());
            };
            if session.worktree != self.worktree() || session.delegation.is_some() {
                return Err(denied());
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Clipboard {
    id: RequestId,
    access: Access,
    paths: Vec<(String, sailry_protocol::EntryKind)>,
    cut: bool,
}
impl Clipboard {
    fn snapshot(&self, target: &Access) -> Value {
        let cross = self.access.client.target() != target.client.target();
        json!({"id":self.id,"cut":self.cut,"source_label":self.access.label,
            "paths":self.paths.iter().map(|(path,kind)|json!({"path":path,"kind":kind})).collect::<Vec<_>>(),
            "cross_node":cross,"available":!cross || self.paths.iter().all(|(_,kind)| *kind==sailry_protocol::EntryKind::File)})
    }
}

#[derive(Clone)]
enum Progress {
    Preparing,
    Copying(u64, u64),
    Publishing,
    Recycling,
}

struct Transfer {
    access: Access,
    source: worker::Source,
    path: String,
    stage: &'static str,
    progress: Option<(u64, u64)>,
    error: Option<Fault>,
    prepared: Option<worker::Prepared>,
    existing: Option<String>,
    published: bool,
    cancel: CancellationToken,
    locked: bool,
    clipboard: Option<RequestId>,
}
impl Transfer {
    fn kind(&self) -> &'static str {
        match &self.source {
            worker::Source::Entry { cut: true, .. } => "move",
            worker::Source::Entry { .. } => "copy",
            worker::Source::Upload(_) => "upload",
            worker::Source::Download(_) => "download",
        }
    }
    fn active(&self) -> bool {
        matches!(
            self.stage,
            "preparing" | "transferring" | "publishing" | "recycling"
        )
    }
    fn snapshot(&self, id: RequestId) -> Value {
        let label = match &self.source {
            worker::Source::Entry { access, .. } => access.label.clone(),
            worker::Source::Upload(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            worker::Source::Download(_) => self.access.label.clone(),
        };
        let source_path = match &self.source {
            worker::Source::Entry { path, .. } => path.clone(),
            worker::Source::Upload(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            worker::Source::Download(_) => self.path.clone(),
        };
        json!({"id":id,"kind":self.kind(),"stage":self.stage,"path":self.path,"source_label":label,"source_path":source_path,"error":self.error,
            "progress":self.progress.map(|(copied,size)|json!({"copied":copied,"size":size})),"published":self.published,
            "can_cancel":matches!(self.stage,"ready"|"preparing"|"transferring"),
            "can_start":matches!(self.stage,"ready"|"failed"|"cancelled") && self.prepared.is_none(),
            "can_replace":self.stage=="exists" && matches!(self.source,worker::Source::Upload(_)),
            "can_check":self.stage=="uncertain" && self.prepared.is_some()})
    }
    fn paths(&self) -> Vec<(Location, String)> {
        let mut paths = vec![(self.access.location(), self.path.clone())];
        if let worker::Source::Entry {
            access,
            path,
            cut: true,
            ..
        } = &self.source
        {
            paths.push((access.location(), path.clone()));
        }
        paths
    }
}

struct Shared(Entity<Controller>);
impl Global for Shared {}

#[derive(Clone, Default)]
struct Snapshot {
    cursor: u64,
    transfers: Vec<(Location, String, Value)>,
}
impl Snapshot {
    fn for_access(&self, access: &Access) -> Value {
        json!({"cursor":self.cursor.to_string(),"transfers":self.transfers.iter().filter(|(location,package,_)|*location==access.location()&&package==&access.context.package.name).map(|(_,_,value)|value.clone()).collect::<Vec<_>>()})
    }
}

pub(super) struct Controller {
    clipboard: Option<Clipboard>,
    generation: u64,
    transfers: BTreeMap<RequestId, Transfer>,
    recovery: BTreeMap<RequestId, (Location, String)>,
    documents: Vec<WeakEntity<super::documents::Controller>>,
    changes: tokio::sync::watch::Sender<Snapshot>,
    cursor: u64,
    copies: Vec<tempfile::TempDir>,
}
impl Controller {
    fn new(_: &mut Context<Self>) -> Self {
        Self {
            clipboard: None,
            generation: 0,
            transfers: BTreeMap::new(),
            recovery: BTreeMap::new(),
            documents: vec![],
            changes: tokio::sync::watch::channel(Snapshot::default()).0,
            cursor: 0,
            copies: vec![],
        }
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.cursor += 1;
        self.changes.send_replace(Snapshot {
            cursor: self.cursor,
            transfers: self
                .transfers
                .iter()
                .map(|(id, transfer)| {
                    (
                        transfer.access.location(),
                        transfer.access.context.package.name.clone(),
                        transfer.snapshot(*id),
                    )
                })
                .collect(),
        });
        let documents = self.documents.clone();
        cx.defer(move |cx| {
            for document in documents
                .into_iter()
                .filter_map(|document| document.upgrade())
            {
                document.update(cx, |document, cx| document.changed(cx));
            }
        });
        cx.notify();
    }
    fn snapshot(&self, access: &Access) -> Value {
        json!({"cursor":self.cursor.to_string(),"transfers":self.transfers.iter().filter(|(_,transfer)|transfer.access.location()==access.location()&&transfer.access.context.package.name==access.context.package.name).map(|(id,transfer)|transfer.snapshot(*id)).collect::<Vec<_>>()})
    }
    fn affects(&self, location: Location, path: &str) -> bool {
        self.recovery
            .values()
            .any(|(owner, entry)| *owner == location && overlaps(path, entry))
            || self.transfers.values().any(|transfer| {
                transfer.locked
                    && transfer
                        .paths()
                        .iter()
                        .any(|(owner, entry)| *owner == location && overlaps(path, entry))
            })
    }
    fn dirty(&self, location: Location, path: &str, cx: &App) -> bool {
        self.documents
            .iter()
            .filter_map(WeakEntity::upgrade)
            .any(|controller| {
                let controller = controller.read(cx);
                controller.scope() == location && controller.affects(path, cx)
            })
    }
    fn insert(
        &mut self,
        access: Access,
        source: worker::Source,
        path: String,
        clipboard: Option<RequestId>,
        cx: &mut Context<Self>,
    ) -> Result<Value, Fault> {
        valid_path(&path, false)?;
        let id = RequestId::new();
        let transfer = Transfer {
            access,
            source,
            path,
            stage: "ready",
            progress: None,
            error: None,
            prepared: None,
            existing: None,
            published: false,
            cancel: CancellationToken::new(),
            locked: false,
            clipboard,
        };
        let value = transfer.snapshot(id);
        self.transfers.insert(id, transfer);
        self.changed(cx);
        Ok(value)
    }
    fn start(
        &mut self,
        id: RequestId,
        path: Option<String>,
        replace: bool,
        check: bool,
        stop: CancellationToken,
        cx: &mut Context<Self>,
    ) -> Result<Value, Fault> {
        let transfer = self.transfers.get(&id).ok_or_else(unavailable)?;
        if transfer.active() {
            return Err(busy());
        }
        if check {
            if transfer.stage != "uncertain" || transfer.prepared.is_none() {
                return Err(unavailable());
            }
        } else if transfer.prepared.is_some()
            || !(matches!(transfer.stage, "ready" | "failed" | "cancelled")
                || (replace
                    && transfer.stage == "exists"
                    && matches!(transfer.source, worker::Source::Upload(_))))
        {
            return Err(unavailable());
        }
        let path = path.unwrap_or_else(|| transfer.path.clone());
        valid_path(&path, false)?;
        if check && path != transfer.path {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "an admitted destination cannot change",
            ));
        }
        let mut paths = vec![(transfer.access.location(), path.clone())];
        if let worker::Source::Entry {
            access,
            path,
            cut: true,
            ..
        } = &transfer.source
        {
            paths.push((access.location(), path.clone()));
        }
        if !check && !matches!(transfer.source, worker::Source::Download(_)) {
            for (location, path) in &paths {
                if self.affects(*location, path) {
                    return Err(busy());
                }
                if self.dirty(*location, path, cx) {
                    return Err(Fault::new(ErrorCode::Conflict, "file has unsaved changes"));
                }
            }
        }
        let transfer = self.transfers.get_mut(&id).unwrap();
        transfer.path = path;
        transfer.stage = "preparing";
        transfer.error = None;
        transfer.progress = None;
        transfer.locked = !matches!(transfer.source, worker::Source::Download(_));
        transfer.cancel = stop.child_token();
        let cancel = transfer.cancel.clone();
        let access = transfer.access.clone();
        let source = transfer.source.clone();
        let path = transfer.path.clone();
        let expected = replace.then(|| transfer.existing.clone()).flatten();
        let prepared = transfer.prepared.clone();
        let retained = Arc::new(std::sync::Mutex::new(prepared.clone()));
        let recovery = retained.clone();
        let (updates, mut changes) = tokio::sync::watch::channel(Progress::Preparing);
        let job = access.runtime.clone().spawn(worker::run(
            access,
            source,
            path,
            expected,
            worker::Recovery { prepared, retained },
            cancel,
            updates,
        ));
        cx.spawn(async move |owner, cx| {
            while changes.changed().await.is_ok() {
                let progress = changes.borrow_and_update().clone();
                if owner
                    .update(cx, |owner, cx| {
                        if let Some(transfer) = owner
                            .transfers
                            .get_mut(&id)
                            .filter(|transfer| transfer.active())
                        {
                            match progress {
                                Progress::Preparing => {}
                                Progress::Copying(copied, size) => {
                                    transfer.stage = "transferring";
                                    transfer.progress = Some((copied, size));
                                }
                                Progress::Publishing => transfer.stage = "publishing",
                                Progress::Recycling => {
                                    transfer.stage = "recycling";
                                    transfer.published = true;
                                }
                            }
                            owner.changed(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |owner, cx| {
            let result = job.await.unwrap_or(worker::Completion {
                prepared: recovery.lock().ok().and_then(|prepared| prepared.clone()),
                result: Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "transfer worker stopped",
                )),
                published: false,
                existing: None,
            });
            let _ = owner.update(cx, |owner, cx| owner.finish(id, result, cx));
        })
        .detach();
        let value = self.transfers[&id].snapshot(id);
        self.changed(cx);
        Ok(value)
    }
    fn finish(&mut self, id: RequestId, result: worker::Completion, cx: &mut Context<Self>) {
        let Some(transfer) = self.transfers.get_mut(&id) else {
            return;
        };
        transfer.published |= result.published;
        let uncertain = result.prepared.is_some()
            && result.result.as_ref().is_err_and(|error| {
                matches!(
                    error.code,
                    ErrorCode::OutcomeUnknown
                        | ErrorCode::Unavailable
                        | ErrorCode::Cancelled
                        | ErrorCode::Internal
                )
            })
            || (transfer.published && result.result.is_err());
        transfer.prepared = if uncertain { result.prepared } else { None };
        transfer.existing = result.existing;
        transfer.stage = match &result.result {
            Ok(()) => "done",
            Err(_) if uncertain => "uncertain",
            Err(_)
                if transfer.existing.is_some()
                    && matches!(transfer.source, worker::Source::Upload(_)) =>
            {
                "exists"
            }
            Err(error) if error.code == ErrorCode::Cancelled => "cancelled",
            Err(_) => "failed",
        };
        transfer.error = result.result.err();
        transfer.locked = uncertain;
        if transfer.stage == "done"
            && let worker::Source::Entry {
                path, cut: true, ..
            } = &transfer.source
            && let Some(clipboard) = &mut self.clipboard
            && Some(clipboard.id) == transfer.clipboard
        {
            clipboard.paths.retain(|(entry, _)| entry != path);
            if clipboard.paths.is_empty() {
                self.clipboard = None;
            }
        }
        if transfer.stage == "done" {
            let target = transfer.access.location();
            let target_path = transfer.path.clone();
            let context = transfer.access.context.clone();
            let source = match &transfer.source {
                worker::Source::Entry {
                    access,
                    path,
                    cut: true,
                    ..
                } => Some((access.location(), path.clone())),
                _ => None,
            };
            let refresh = !matches!(transfer.source, worker::Source::Download(_));
            cx.defer(move |cx| {
                use super::documents::entries::EntryAction;
                if let Some((origin, path)) = source {
                    let action = if origin == target {
                        EntryAction::Rename {
                            from: path,
                            to: target_path.clone(),
                        }
                    } else {
                        EntryAction::Trash { path }
                    };
                    for document in documents(origin, cx) {
                        document.update(cx, |document, cx| document.apply_entry(&action, cx));
                    }
                }
                if refresh {
                    for document in documents(target, cx) {
                        document.update(cx, |document, cx| {
                            document.refresh(&target_path, context.clone(), cx)
                        });
                    }
                }
            });
        }
        self.changed(cx);
    }
}

pub(super) fn registry(cx: &mut App) -> Entity<Controller> {
    if !cx.has_global::<Shared>() {
        let controller = cx.new(Controller::new);
        cx.set_global(Shared(controller));
    }
    cx.global::<Shared>().0.clone()
}

pub(crate) fn affects(location: Location, path: &str, cx: &App) -> bool {
    cx.try_global::<Shared>()
        .is_some_and(|shared| shared.0.read(cx).affects(location, path))
}

pub(crate) fn documents(location: Location, cx: &App) -> Vec<Entity<super::documents::Controller>> {
    cx.try_global::<Shared>()
        .map(|shared| {
            shared
                .0
                .read(cx)
                .documents
                .iter()
                .filter_map(WeakEntity::upgrade)
                .filter(|controller| controller.read(cx).scope() == location)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn dirty(location: Location, path: &str, cx: &App) -> bool {
    documents(location, cx)
        .iter()
        .any(|controller| controller.read(cx).affects(path, cx))
}

fn overlaps(left: &str, right: &str) -> bool {
    left.is_empty()
        || right.is_empty()
        || left == right
        || left
            .strip_prefix(right)
            .is_some_and(|tail| tail.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|tail| tail.starts_with('/'))
}
fn valid_path(path: &str, root: bool) -> Result<(), Fault> {
    if (path.is_empty() && !root)
        || path.contains('\0')
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| matches!(part, "." | "..") || (!path.is_empty() && part.is_empty()))
    {
        Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid relative file path",
        ))
    } else {
        Ok(())
    }
}
fn internal() -> Fault {
    Fault::new(ErrorCode::Internal, "invalid file transfer response")
}
fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "file transfer is unavailable")
}
fn denied() -> Fault {
    Fault::new(ErrorCode::PermissionDenied, "file action is not declared")
}
fn busy() -> Fault {
    Fault::new(ErrorCode::Busy, "file operation is busy")
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "file transfer cancelled")
}

pub(super) fn notify_documents(cx: &mut App) {
    registry(cx).update(cx, |controller, cx| controller.changed(cx));
}

pub(super) fn other_entries(location: Location, path: &str, owner: EntityId, cx: &App) -> bool {
    cx.try_global::<Shared>().is_some_and(|shared| {
        shared
            .0
            .read(cx)
            .documents
            .iter()
            .filter_map(WeakEntity::upgrade)
            .filter(|document| document.entity_id() != owner)
            .any(|document| {
                let document = document.read(cx);
                document.scope() == location && document.entries_affect(path)
            })
    })
}

pub(super) fn other_dirty(location: Location, path: &str, owner: EntityId, cx: &App) -> bool {
    cx.try_global::<Shared>().is_some_and(|shared| {
        shared
            .0
            .read(cx)
            .documents
            .iter()
            .filter_map(WeakEntity::upgrade)
            .filter(|document| document.entity_id() != owner)
            .any(|document| {
                let document = document.read(cx);
                document.scope() == location && document.affects(path, cx)
            })
    })
}
