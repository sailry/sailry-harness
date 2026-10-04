//! Captured document buffers outlive plugin renderers; Kit owns editing and undo.
mod closing;
pub(in crate::plugins) mod entries;
mod load;
mod recovery;
pub(crate) mod routes;
mod save;
pub(in crate::plugins) mod sdk;
mod state;
mod surface;
#[cfg(test)]
mod tests;

use crate::conversation::live::Binding;
use gpui_kit::*;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, NodeId, RequestId, SessionId, WorktreeId, plugin};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap};

pub(super) use sdk::{Scope, module};
use state::{Action, Document};

type Key = (NodeId, WorktreeId, Option<SessionId>);
type Reply = tokio::sync::oneshot::Sender<Result<Value, Fault>>;

struct Completion {
    stop: CancellationToken,
    reply: Reply,
}

#[derive(Default)]
pub(crate) struct Store {
    controllers: BTreeMap<Key, Entity<Controller>>,
}

impl Store {
    pub(crate) fn any_unsaved(&self, cx: &App) -> bool {
        self.controllers
            .values()
            .any(|controller| controller.read(cx).has_unsaved(cx))
    }

    pub(crate) fn release(&mut self, scope: (NodeId, WorktreeId), cx: &App) {
        self.controllers.retain(|(node, tree, _), controller| {
            (*node, *tree) != scope || controller.read(cx).affects("", cx)
        });
    }
    pub(crate) fn get(
        &mut self,
        binding: Binding,
        session: Option<SessionId>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Entity<Controller>> {
        let key = (binding.client.target(), binding.worktree?, session);
        Some(
            self.controllers
                .entry(key)
                .or_insert_with(|| cx.new(|cx| Controller::new(binding, session, window, cx)))
                .clone(),
        )
    }

    pub(crate) fn has_unsaved(&self, scope: (NodeId, WorktreeId), cx: &App) -> bool {
        self.controllers
            .iter()
            .any(|((node, tree, _), controller)| {
                (*node, *tree) == scope && controller.read(cx).has_unsaved(cx)
            })
    }
}

pub(crate) struct Controller {
    owner: EntityId,
    binding: Binding,
    session: Option<SessionId>,
    documents: BTreeMap<RequestId, Document>,
    entries: BTreeMap<RequestId, entries::Entry>,
    changes: tokio::sync::watch::Sender<Value>,
    cursor: u64,
    intent: u64,
    read_sequence: u64,
    reads: BTreeMap<String, u64>,
    reveal: Option<(RequestId, u64)>,
    stop: CancellationToken,
}

enum Operation {
    Refresh {
        path: String,
    },
    Open {
        path: String,
        line: Option<usize>,
    },
    Action {
        id: RequestId,
        action: Action,
    },
    Save {
        id: RequestId,
    },
    Entry {
        id: RequestId,
    },
    Close {
        id: RequestId,
        discard: bool,
        confirm: bool,
    },
}

struct Request {
    context: plugin::Context,
    write: bool,
    stop: CancellationToken,
    operation: Operation,
    reply: RefCell<Option<Reply>>,
}
impl EventEmitter<Request> for Controller {}

impl Drop for Controller {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Controller {
    fn new(
        binding: Binding,
        session: Option<SessionId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe_in(
            &cx.entity(),
            window,
            |controller, _, request: &Request, window, cx| {
                let Some(reply) = request.reply.borrow_mut().take() else {
                    return;
                };
                if request.stop.is_cancelled()
                    || request.context.worktree != controller.binding.worktree
                    || (request.context.session != controller.session
                        && !matches!(request.operation, Operation::Refresh { .. }))
                    || request.context.surface != plugin::desktop::Surface::Workspace
                {
                    let _ = reply.send(Err(unavailable()));
                    return;
                }
                match &request.operation {
                    Operation::Refresh { path } => controller.load(
                        load::Target {
                            path: path.clone(),
                            line: None,
                        },
                        request.context.clone(),
                        Completion {
                            stop: request.stop.clone(),
                            reply,
                        },
                        false,
                        window,
                        cx,
                    ),
                    Operation::Open { path, line } => controller.open(
                        load::Target {
                            path: path.clone(),
                            line: *line,
                        },
                        request.context.clone(),
                        Completion {
                            stop: request.stop.clone(),
                            reply,
                        },
                        window,
                        cx,
                    ),
                    Operation::Entry { id }
                        if request.write
                            && controller.owns_entry(*id, &request.context.package.name) =>
                    {
                        controller.execute_entry(*id, reply, window, cx)
                    }
                    Operation::Save { id } if request.write => controller.save(
                        *id,
                        request.context.clone(),
                        request.stop.clone(),
                        reply,
                        window,
                        cx,
                    ),
                    Operation::Action { id, action } if request.write || !action.edits() => {
                        let result = controller.action(*id, action.clone(), window, cx);
                        let _ = reply.send(result);
                    }
                    Operation::Close {
                        id,
                        discard,
                        confirm,
                    } if request.write
                        || (!discard && !confirm)
                        || controller
                            .documents
                            .get(id)
                            .is_none_or(|document| !document.unsaved(cx)) =>
                    {
                        controller.request_close(
                            *id,
                            *discard,
                            *confirm,
                            Completion {
                                stop: request.stop.clone(),
                                reply,
                            },
                            window,
                            cx,
                        );
                    }
                    _ => {
                        let _ = reply.send(Err(Fault::new(
                            ErrorCode::PermissionDenied,
                            "document is read-only",
                        )));
                    }
                }
            },
        )
        .detach();
        Self {
            owner: cx.entity_id(),
            binding,
            session,
            documents: BTreeMap::new(),
            entries: BTreeMap::new(),
            changes: tokio::sync::watch::channel(Value::Null).0,
            cursor: 0,
            intent: 0,
            read_sequence: 0,
            reads: BTreeMap::new(),
            reveal: None,
            stop: CancellationToken::new(),
        }
    }

    pub(in crate::plugins) fn refresh(
        &mut self,
        path: &str,
        context: plugin::Context,
        cx: &mut Context<Self>,
    ) {
        let paths = self
            .documents
            .values()
            .filter(|document| {
                !document.dismissed
                    && !document.unsaved(cx)
                    && (path.is_empty()
                        || document.path == path
                        || document
                            .path
                            .strip_prefix(path)
                            .is_some_and(|tail| tail.starts_with('/')))
            })
            .map(|document| document.path.clone())
            .collect::<Vec<_>>();
        for path in paths {
            let (reply, _) = tokio::sync::oneshot::channel();
            cx.emit(Request {
                context: context.clone(),
                write: false,
                stop: self.stop.child_token(),
                operation: Operation::Refresh { path },
                reply: RefCell::new(Some(reply)),
            });
        }
    }

    pub(crate) fn request_open(
        &mut self,
        path: String,
        line: Option<usize>,
        context: plugin::Context,
        stop: CancellationToken,
        cx: &mut Context<Self>,
    ) -> tokio::sync::oneshot::Receiver<Result<Value, Fault>> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        cx.emit(Request {
            context,
            write: false,
            stop,
            operation: Operation::Open { path, line },
            reply: RefCell::new(Some(reply)),
        });
        receive
    }

    pub(crate) fn session(&self) -> Option<SessionId> {
        self.session
    }

    pub(crate) fn scope(&self) -> (NodeId, WorktreeId) {
        (
            self.binding.client.target(),
            self.binding.worktree.expect("document worktree"),
        )
    }

    pub(crate) fn affects(&self, path: &str, cx: &App) -> bool {
        self.entries_affect(path)
            || self.documents.values().any(|document| {
                document.unsaved(cx)
                    && (path.is_empty()
                        || document.path == path
                        || document
                            .path
                            .strip_prefix(path)
                            .is_some_and(|suffix| suffix.starts_with('/')))
            })
    }

    pub(crate) fn discard_all(&mut self, cx: &mut Context<Self>) {
        for id in self.documents.keys().copied().collect::<Vec<_>>() {
            let _ = self.close(id, true, cx);
        }
    }

    pub(crate) fn has_unsaved(&self, cx: &App) -> bool {
        self.documents
            .values()
            .any(|document| !document.dismissed && document.unsaved(cx))
    }

    pub(crate) fn snapshot(&self, cx: &App) -> Value {
        json!({"cursor":self.cursor.to_string(), "documents":self.documents.iter().filter(|(_, document)| !document.dismissed).map(|(id, _)| self.document_value(*id, cx)).collect::<Vec<_>>(),
            "operations":self.entries.values().filter(|entry| entry.visible()).map(entries::Entry::snapshot).collect::<Vec<_>>(),
            "reveal":self.reveal.map(|(id, sequence)| json!({"document":id,"sequence":sequence.to_string()}))})
    }

    fn locked(&self, path: &str, cx: &App) -> bool {
        self.entries_affect(path)
            || crate::plugins::file_transfers::affects(self.scope(), path, cx)
            || crate::plugins::file_transfers::other_entries(self.scope(), path, self.owner, cx)
    }
    fn document_value(&self, id: RequestId, cx: &App) -> Value {
        let document = &self.documents[&id];
        let mut value = document.snapshot(id, cx);
        if self.locked(&document.path, cx) {
            value["can_edit"] = false.into();
            value["can_cut_paste"] = false.into();
            value["can_save"] = false.into();
            value["readonly"] = true.into();
        }
        value
    }

    pub(in crate::plugins) fn changed(&mut self, cx: &mut Context<Self>) {
        self.cursor += 1;
        self.changes.send_replace(self.snapshot(cx));
        cx.notify();
    }

    fn reveal(
        &mut self,
        id: RequestId,
        intent: u64,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if intent != self.intent {
            self.changed(cx);
            return;
        }
        if let Some(document) = self.documents.get_mut(&id) {
            if let Some(line) = line {
                document.action(Action::Reveal { line }, window, cx);
            }
            self.reveal = Some((id, intent));
            self.changed(cx);
        }
    }

    fn action(
        &mut self,
        id: RequestId,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, Fault> {
        let locked = self
            .documents
            .get(&id)
            .is_some_and(|document| self.locked(&document.path, cx));
        let document = self.documents.get_mut(&id).ok_or_else(unavailable)?;
        if action.edits() && locked {
            return Err(Fault::new(ErrorCode::Busy, "file operation is in progress"));
        }
        if action.edits() && (document.revision.is_none() || document.uncertain) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "document is read-only",
            ));
        }
        document.action(action, window, cx);
        let value = document.snapshot(id, cx);
        self.changed(cx);
        Ok(value)
    }

    fn close(
        &mut self,
        id: RequestId,
        discard: bool,
        cx: &mut Context<Self>,
    ) -> Result<bool, Fault> {
        let document = self.documents.get(&id).ok_or_else(unavailable)?;
        self.reads.remove(&document.path);
        if document.unsaved(cx) && !discard {
            return Ok(false);
        }
        if document.uncertain || (document.saving && document.staging.is_none()) {
            self.documents.get_mut(&id).unwrap().dismissed = true;
        } else {
            self.documents.remove(&id);
        }
        if self.reveal.is_some_and(|(document, _)| document == id) {
            self.reveal = None;
        }
        self.changed(cx);
        Ok(true)
    }
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "document is unavailable")
}
