//! Turn-scoped recovery reuses Node checkpoints and the shared editor mutation guard.
type Scope = (sailry_protocol::NodeId, sailry_protocol::WorktreeId);
use crate::{
    conversation::live::{Binding, View as Chat},
    shell::Shell,
    tr,
};
use gpui_kit::component::{WindowExt, button::Button, notification::Notification};
use gpui_kit::*;
use sailry_protocol::{
    CheckpointId, Command, ErrorCode, Output, Request, SessionId, TurnId, conversation::checkpoint,
};
use std::collections::HashSet;

#[cfg(test)]
mod tests;

pub(crate) struct Restore {
    shell: WeakEntity<Shell>,
    source: WeakEntity<Chat>,
    binding: Binding,
    session: SessionId,
    turn: TurnId,
    files: Option<Vec<checkpoint::File>>,
    done: HashSet<CheckpointId>,
    target: Option<String>,
    request: Option<Request>,
    pub(crate) pending: bool,
    issue: Option<&'static str>,
}

impl Shell {
    pub(crate) fn undo_file_changes(
        &mut self,
        source: Entity<Chat>,
        turn: TurnId,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.current_chat() != Some(&source) || !source.read(cx).connected() {
            return;
        }
        let shell = cx.entity().downgrade();
        source.update(cx, |view, cx| {
            view.undo_changes(shell, turn, path, window, cx)
        });
    }
}

impl Restore {
    pub(crate) fn new(
        shell: WeakEntity<Shell>,
        source: WeakEntity<Chat>,
        binding: Binding,
        session: SessionId,
        turn: TurnId,
    ) -> Self {
        Self {
            shell,
            source,
            binding,
            session,
            turn,
            files: None,
            done: HashSet::new(),
            target: None,
            request: None,
            pending: false,
            issue: None,
        }
    }

    pub(crate) fn restored(&self, path: Option<&str>) -> bool {
        self.files.as_ref().is_some_and(|files| {
            let mut matching = files
                .iter()
                .filter(|file| path.is_none_or(|path| path == file.path))
                .peekable();
            matching.peek().is_some() && matching.all(|file| self.done.contains(&file.id))
        })
    }

    pub(crate) fn uncertain(&self) -> bool {
        self.request.is_some() && !self.pending
    }

    pub(crate) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start(self.target.clone(), window, cx);
    }

    fn report(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(issue) = self.issue else { return };
        let owner = cx.entity().downgrade();
        crate::feedback::toast(
            window,
            tr(issue),
            Notification::error(tr(issue))
                .id1::<Self>(("restore", cx.entity_id()))
                .action(move |_, _, cx| {
                    let owner = owner.clone();
                    Button::new("retry-undo")
                        .label(tr("turn_changes_retry"))
                        .debug_selector(|| "turn-changes-retry".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            let owner = owner.clone();
                            window.defer(cx, move |window, cx| {
                                _ = owner.update(cx, |state, cx| state.retry(window, cx));
                            });
                        }))
                }),
            cx,
        );
    }

    fn scope(&self) -> Scope {
        (
            self.binding.client.target(),
            self.binding.worktree.expect("checkpoint worktree"),
        )
    }

    fn current(&self, shell: &Shell, cx: &App) -> bool {
        self.source.upgrade().is_some_and(|source| {
            shell.current_chat() == Some(&source)
                && source.read(cx).connected()
                && source.read(cx).session() == Some(self.session)
        })
    }

    pub(crate) fn start(
        &mut self,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending {
            return;
        }
        // An uncertain command can only be retried with its original identity and target.
        if self.request.is_none() {
            self.target = path;
        }
        self.issue = None;
        window.remove_notification1::<Self>(("restore", cx.entity_id()), cx);
        if self.files.is_some() {
            self.next(window, cx);
            return;
        }
        self.pending = true;
        let client = self.binding.client.clone();
        let session = self.session;
        let turn = self.turn;
        let job = self.binding.runtime.spawn(async move {
            let mut files = Vec::new();
            let mut before = None;
            loop {
                let request = client.prepare(Command::ListFileCheckpoints {
                    session,
                    turn,
                    before,
                    limit: 100,
                });
                match client.execute(request).await {
                    Ok(Output::FileCheckpoints(page))
                        if page.session == session && page.turn == turn =>
                    {
                        files.extend(page.files.into_iter().filter(|file| {
                            matches!(
                                &file.outcome,
                                sailry_protocol::RequestOutcome::Completed(result)
                                    if matches!(result.as_ref(), Ok(Output::FileWritten(_)))
                            )
                        }));
                        before = page.next;
                        if before.is_none() {
                            return Some(files);
                        }
                    }
                    _ => return None,
                }
            }
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |state, window, cx| {
                state.pending = false;
                if let Ok(Some(files)) = result
                    && let Some(worktree) = files.first().map(|file| file.worktree)
                    && files.iter().all(|file| file.worktree == worktree)
                {
                    state.binding.worktree = Some(worktree);
                    state.files = Some(files);
                    state.next(window, cx);
                    return;
                }
                state.issue = Some("checkpoint_list_failed");
                state.report(window, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The Node lists checkpoints newest first, so repeated writes unwind in reverse order.
        let Some(file) = self
            .files
            .as_ref()
            .and_then(|files| {
                files.iter().find(|file| {
                    !self.done.contains(&file.id)
                        && self.target.as_ref().is_none_or(|path| path == &file.path)
                })
            })
            .cloned()
        else {
            self.pending = false;
            cx.notify();
            return;
        };
        let scope = self.scope();
        let request = self.request.clone().unwrap_or_else(|| {
            self.binding.client.prepare(Command::RestoreFileCheckpoint {
                session: self.session,
                checkpoint: file.id,
                worktree: scope.1,
            })
        });
        let guard = self
            .shell
            .update(cx, |shell, cx| {
                if !self.current(shell, cx) {
                    return Err("files_context_changed");
                }
                crate::plugins::file_transfers::recovery::reserve(
                    scope, request.id, &file.path, cx,
                )?;
                cx.notify();
                Ok(())
            })
            .unwrap_or(Err("files_context_changed"));
        if let Err(error) = guard {
            self.issue = Some(error);
            self.pending = false;
            self.report(window, cx);
            cx.notify();
            return;
        }
        self.request = Some(request.clone());
        self.pending = true;
        let client = self.binding.client.clone();
        let id = request.id;
        let session = self.session;
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        // Dropping the conversation must not drop cleanup for an admitted mutation.
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let success = matches!(
                &result,
                Ok(Ok(Output::FileRestored(restored)))
                    if restored.session == session
                        && restored.checkpoint == file.id
                        && restored.path == file.path
                        && restored.revision.as_ref()
                            == file.before.as_ref().map(|before| &before.revision)
            );
            let error = if success {
                None
            } else {
                Some(match &result {
                    Ok(Err(fault)) => match fault.code {
                        ErrorCode::RevisionConflict => "checkpoint_conflict",
                        ErrorCode::Busy => "checkpoint_busy",
                        ErrorCode::WrongTarget | ErrorCode::NotFound => "checkpoint_missing",
                        ErrorCode::OutcomeUnknown | ErrorCode::Unavailable => "checkpoint_unknown",
                        _ => "checkpoint_failed",
                    },
                    _ => "checkpoint_unknown",
                })
            };
            let _ = cx.update(|window, cx| {
                crate::plugins::file_transfers::recovery::release(
                    id,
                    error == Some("checkpoint_unknown"),
                    cx,
                );
                if success {
                    for documents in crate::plugins::file_transfers::documents(scope, cx) {
                        documents.update(cx, |documents, cx| {
                            documents.restored(&file.path, file.before.is_none(), window, cx)
                        });
                    }
                }
            });
            let _ = owner.update_in(cx, |state, window, cx| {
                state.pending = false;
                state.issue = error;
                state.report(window, cx);
                if error != Some("checkpoint_unknown") {
                    state.request = None;
                }
                if success {
                    state.done.insert(file.id);
                    state.next(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
