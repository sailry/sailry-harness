//! Entry mutations use the same durable request as the public SDK. Core buffers
//! follow confirmed renames/removals and retain locks after uncertain outcomes.
use super::*;
use crate::plugins::host::sdk::{confirmed, public_output};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::plugins) enum EntryAction {
    Write {
        path: String,
        text: String,
        revision: Option<String>,
    },
    CreateDirectory {
        path: String,
    },
    Rename {
        from: String,
        to: String,
    },
    Copy {
        from: String,
        to: String,
    },
    Trash {
        path: String,
    },
}
impl EntryAction {
    pub fn paths(&self) -> Vec<&str> {
        match self {
            Self::Write { path, .. } | Self::CreateDirectory { path } | Self::Trash { path } => {
                vec![path]
            }
            Self::Rename { from, to } | Self::Copy { from, to } => vec![from, to],
        }
    }
    pub fn command(&self, worktree: WorktreeId) -> sailry_protocol::Command {
        use sailry_protocol::Command;
        match self.clone() {
            Self::Write {
                path,
                text,
                revision,
            } => Command::WriteFile {
                worktree,
                path,
                text,
                expected_revision: revision,
            },
            Self::CreateDirectory { path } => Command::CreateDirectory { worktree, path },
            Self::Rename { from, to } => Command::RenameEntry { worktree, from, to },
            Self::Copy { from, to } => Command::CopyEntry { worktree, from, to },
            Self::Trash { path } => Command::TrashEntry { worktree, path },
        }
    }
    fn snapshot(&self) -> Value {
        match self {
            Self::Write { path, .. } => json!({"kind":"write","path":path}),
            _ => serde_json::to_value(self).expect("entry action"),
        }
    }
}
pub(super) struct Entry {
    action: EntryAction,
    request: sailry_protocol::Request,
    executing: bool,
    uncertain: bool,
}
impl Entry {
    pub fn snapshot(&self) -> Value {
        json!({"id":self.request.id,"action":self.action.snapshot(),"running":self.executing,"uncertain":self.uncertain})
    }
    pub fn visible(&self) -> bool {
        self.executing || self.uncertain
    }
}
impl Controller {
    pub(in crate::plugins) fn prepare_entry(
        &mut self,
        action: EntryAction,
        request: sailry_protocol::Request,
        cx: &mut Context<Self>,
    ) -> Result<(), Fault> {
        if self.entries.len() >= 16 {
            return Err(Fault::new(
                ErrorCode::Busy,
                "document operation capacity exhausted",
            ));
        }
        self.entries.insert(
            request.id,
            Entry {
                action,
                request,
                executing: false,
                uncertain: false,
            },
        );
        self.changed(cx);
        Ok(())
    }
    pub(in crate::plugins) fn owns_entry(&self, id: RequestId, package: &str) -> bool {
        self.entries.get(&id).is_some_and(|entry| {
            entry
                .request
                .plugin
                .as_ref()
                .is_some_and(|context| context.package.name == package)
        })
    }
    pub(in crate::plugins) fn forget_entry(&mut self, id: RequestId, cx: &mut Context<Self>) {
        if self.entries.get(&id).is_some_and(|entry| !entry.visible()) {
            self.entries.remove(&id);
            self.changed(cx);
        }
    }
    pub(in crate::plugins) fn entries_affect(&self, path: &str) -> bool {
        self.entries.values().any(|entry| {
            entry.visible()
                && entry
                    .action
                    .paths()
                    .iter()
                    .any(|candidate| overlap(candidate, path))
        })
    }
    pub(super) fn execute_entry(
        &mut self,
        id: RequestId,
        reply: Reply,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.entries.get(&id) else {
            let _ = reply.send(Err(unavailable()));
            return;
        };
        if entry.executing {
            let _ = reply.send(Err(Fault::new(
                ErrorCode::Busy,
                "file operation is already running",
            )));
            return;
        }
        if entry.action.paths().iter().any(|path| {
            self.documents
                .values()
                .any(|document| document.unsaved(cx) && overlap(path, &document.path))
                || crate::plugins::file_transfers::affects(self.scope(), path, cx)
                || crate::plugins::file_transfers::other_dirty(self.scope(), path, self.owner, cx)
                || self.entries.iter().any(|(other, entry)| {
                    *other != id
                        && entry.visible()
                        && entry
                            .action
                            .paths()
                            .iter()
                            .any(|candidate| overlap(path, candidate))
                })
        }) {
            let _ = reply.send(Err(Fault::new(
                ErrorCode::Conflict,
                "file operation overlaps an unsaved document",
            )));
            return;
        }
        let request = entry.request.clone();
        self.entries.get_mut(&id).unwrap().executing = true;
        cx.defer(crate::plugins::file_transfers::notify_documents);
        let client = self.binding.client.clone();
        let task = self
            .binding
            .runtime
            .spawn(async move { confirmed(&client, request).await });
        self.changed(cx);
        cx.spawn_in(window, async move |controller, cx| {
            let result = task.await.ok().and_then(Result::ok);
            let _ = controller.update_in(cx, |controller, _, cx| {
                let Some(entry) = controller.entries.get_mut(&id) else {
                    return;
                };
                entry.executing = false;
                cx.defer(crate::plugins::file_transfers::notify_documents);
                let response = match result {
                    Some(Ok(output)) => {
                        let action = entry.action.clone();
                        controller.entries.remove(&id);
                        controller.apply_entry(&action, cx);
                        let owner = cx.entity_id();
                        let scope = controller.scope();
                        cx.defer(move |cx| {
                            let documents = crate::plugins::file_transfers::documents(scope, cx);
                            for document in documents {
                                if document.entity_id() != owner {
                                    document.update(cx, |document, cx| {
                                        document.apply_entry(&action, cx)
                                    });
                                }
                            }
                        });
                        Ok(json!({"Ok":public_output(output).unwrap_or(Value::Null)}))
                    }
                    Some(Err(fault)) => {
                        controller.entries.remove(&id);
                        Ok(json!({"Err":fault}))
                    }
                    None => {
                        entry.uncertain = true;
                        Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "file operation outcome is unknown",
                        ))
                    }
                };
                controller.changed(cx);
                let _ = reply.send(response);
            });
        })
        .detach();
    }
    pub(in crate::plugins) fn apply_entry(&mut self, action: &EntryAction, cx: &mut Context<Self>) {
        match action {
            EntryAction::Rename { from, to } => {
                for document in self.documents.values_mut() {
                    if document.path == *from {
                        document.path = to.clone();
                    } else if let Some(suffix) = document
                        .path
                        .strip_prefix(from)
                        .filter(|suffix| suffix.starts_with('/'))
                    {
                        document.path = format!("{to}{suffix}");
                    }
                }
            }
            EntryAction::Trash { path } => {
                self.documents
                    .retain(|_, document| !overlap(path, &document.path));
            }
            _ => {}
        }
        self.changed(cx);
    }
}
fn overlap(left: &str, right: &str) -> bool {
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
