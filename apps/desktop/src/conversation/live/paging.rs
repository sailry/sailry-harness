use super::*;
use sailry_protocol::conversation::RunKind;
use std::collections::BTreeSet;

#[cfg(test)]
pub(super) mod tests;

impl View {
    pub(super) fn load_older(&mut self, cx: &mut Context<Self>) {
        if self.paging_requested
            || !self.history.connected
            || self.history.loading_older
            || self.history.snapshot.as_ref().is_none_or(|snapshot| {
                snapshot.page.next_before.is_none() && snapshot.missing.is_empty()
            })
        {
            return;
        }
        if self.reveal.is_some() {
            self.request_target(cx);
        } else if let Some(older) = &self.older {
            self.paging_requested = older
                .try_send(sailry_client::conversation::HistoryRequest::Older)
                .is_ok();
        }
    }

    pub(super) fn page_at_start(&mut self, cx: &mut Context<Self>) {
        if self.history.older_error.is_none()
            && self.reveal.is_none()
            && !self.scroller.read(cx).is_following_tail()
            && self
                .rows
                .first()
                .is_some_and(|turn| self.navigation.near_start(*turn))
        {
            self.load_older(cx);
        }
    }

    pub(super) fn update_history(&mut self, next: History, cx: &mut Context<Self>) {
        if !self.details_visible.get() {
            self.pending_history = Some(next);
            return;
        }
        if !next.connected
            || next.older_error.is_some()
            || (self.history.loading_older && !next.loading_older)
        {
            self.paging_requested = false;
        }
        if let Some(snapshot) = &next.snapshot
            && self.history.snapshot.as_ref().is_some_and(|previous| {
                previous.page.session != snapshot.page.session
                    || previous.page.revision != snapshot.page.revision
            })
        {
            self.reveal = None;
            self.publication_heights.borrow_mut().clear();
            self.detail_rows.borrow_mut().clear();
            self.expanded
                .retain(|(turn, _), _| snapshot.page.runs.iter().any(|run| run.turn == *turn));
        }
        let changed = next.snapshot.as_ref().is_some_and(|snapshot| {
            self.history.snapshot.as_ref().is_none_or(|previous| {
                !Arc::ptr_eq(&previous.page, &snapshot.page) || previous.drafts != snapshot.drafts
            })
        });
        let affected = self
            .history
            .snapshot
            .as_ref()
            .zip(next.snapshot.as_ref())
            .and_then(|(previous, next)| {
                sailry_client::conversation::rows::changed(previous, next)
            });
        let repository_changed = self
            .binding
            .worktree
            .is_some_and(|worktree| repository::changed(&self.history, &next, worktree));
        self.approvals.observe(&next);
        self.history = next;
        self.select_details(self.navigation.bounds.get());
        if changed || self.outgoing.is_some() {
            self.sync_rows_for(affected.as_ref(), cx);
        }
        if repository_changed {
            self.refresh_repository(repository::Refresh::History, cx);
        }
        self.refresh_clock(cx);
        self.reveal_loaded(cx);
        if !self.history_pending {
            self.history_pending = true;
            let owner = cx.weak_entity();
            if cx
                .update_window(self.history_window, move |_, window, _| {
                    window.on_next_frame(move |_, cx| {
                        let _ = owner.update(cx, |view, cx| {
                            view.history_pending = false;
                            cx.notify();
                        });
                    });
                })
                .is_err()
            {
                self.history_pending = false;
                cx.notify();
            }
        }
    }
    pub(super) fn sync_rows(&mut self, cx: &mut Context<Self>) {
        self.sync_rows_for(None, cx);
    }

    fn sync_rows_for(&mut self, affected: Option<&BTreeSet<TurnId>>, cx: &mut Context<Self>) {
        if !self.details_visible.get() {
            return;
        }
        let outgoing = self.outgoing.as_ref().map(|outgoing| outgoing.row);
        self.reconcile_outgoing();
        let previous: BTreeSet<_> = self.rows.iter().copied().collect();
        let child = self
            .session
            .as_ref()
            .is_some_and(|session| session.delegation.is_some());
        let mut rows = self
            .history
            .snapshot
            .as_ref()
            .map(|snapshot| {
                let entries: BTreeSet<_> = snapshot
                    .page
                    .entries
                    .iter()
                    .map(|entry| entry.turn)
                    .collect();
                let users: BTreeSet<_> = snapshot
                    .page
                    .entries
                    .iter()
                    .filter(|entry| entry.author == "user")
                    .map(|entry| entry.turn)
                    .collect();
                let guidance: BTreeSet<_> = snapshot
                    .page
                    .queue
                    .items
                    .iter()
                    .filter(|item| item.guidance.is_some())
                    .map(|item| item.turn)
                    .collect();
                let visible = sailry_client::conversation::turns::visible_turns(
                    &snapshot.page,
                    &snapshot.drafts,
                );
                snapshot
                    .page
                    .runs
                    .iter()
                    .filter(|run| visible.contains(&run.turn))
                    .filter(|run| {
                        (run.status != Status::Queued || child && !guidance.contains(&run.turn))
                            && (run.status != Status::Cancelled
                                || child
                                || run.kind == RunKind::Compaction
                                || run.input.is_some()
                                || entries.contains(&run.turn))
                    })
                    .filter(|run| {
                        // A run can arrive before both its user event and the Node
                        // admission snapshot. Do not put an unpaired response above
                        // the local message while its exact identity is still unknown.
                        self.outgoing.as_ref().is_none_or(|outgoing| {
                            outgoing.turn == Some(run.turn)
                                || previous.contains(&run.turn)
                                || self.sent.contains_key(&run.turn)
                                || run.kind != RunKind::Task
                                || !matches!(run.status, Status::Running | Status::Stopping)
                                || users.contains(&run.turn)
                        })
                    })
                    .map(|run| run.turn)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut selected: BTreeSet<_> = rows.iter().copied().collect();
        if let Some(snapshot) = &self.history.snapshot {
            for run in sailry_client::conversation::turns::guided(&snapshot.page) {
                if selected.insert(run.turn) {
                    rows.push(run.turn);
                }
            }
        }
        if let Some(outgoing) = &self.outgoing
            && !outgoing.turn.is_some_and(|turn| selected.contains(&turn))
        {
            rows.push(outgoing.row);
            selected.insert(outgoing.row);
        }
        self.scroller.update(cx, |scroller, cx| {
            match insertion(&self.rows, &rows) {
                Some((before, after)) => {
                    if before > 0 {
                        scroller.prepend(before, cx);
                    }
                    if after > 0 {
                        scroller.append(after, cx);
                    }
                }
                None => {
                    let prefix = self
                        .rows
                        .iter()
                        .zip(&rows)
                        .take_while(|(left, right)| left == right)
                        .count();
                    let suffix = self.rows[prefix..]
                        .iter()
                        .rev()
                        .zip(rows[prefix..].iter().rev())
                        .take_while(|(left, right)| left == right)
                        .count();
                    scroller.splice(
                        prefix..self.rows.len() - suffix,
                        rows.len() - prefix - suffix,
                        cx,
                    );
                }
            }
            // Keep the pixel offset within the surviving row, including growing markdown.
            if let Some(affected) = affected {
                for (index, turn) in rows.iter().enumerate() {
                    if previous.contains(turn)
                        && (affected.contains(turn) || outgoing == Some(*turn))
                    {
                        scroller.remeasure_items(index..index + 1, cx);
                    }
                }
            } else {
                scroller.remeasure_items(0..rows.len(), cx);
            }
        });
        let prefixes: Vec<_> = rows.iter().map(ToString::to_string).collect();
        self.texts
            .borrow_mut()
            .retain(|key, _| prefixes.iter().any(|prefix| key.starts_with(prefix)));
        self.rows = rows;
        self.prune_tool_texts();
        self.changes.retain(|turn, _| selected.contains(turn));
    }
}

fn insertion(previous: &[TurnId], current: &[TurnId]) -> Option<(usize, usize)> {
    if previous.is_empty() {
        return Some((0, current.len()));
    }
    let before = current.iter().position(|turn| turn == &previous[0])?;
    current[before..]
        .starts_with(previous)
        .then(|| (before, current.len() - before - previous.len()))
}
