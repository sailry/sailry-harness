use super::*;
use sailry_protocol::conversation::RunKind;

#[cfg(test)]
pub(super) mod tests;

impl View {
    pub(super) fn load_older(&mut self, cx: &mut Context<Self>) {
        if self.paging_requested
            || !self.history.connected
            || self.history.loading_older
            || self
                .history
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| snapshot.page.next_before.is_none())
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
        if !next.connected
            || next.older_error.is_some()
            || (self.history.loading_older && !next.loading_older)
        {
            self.paging_requested = false;
        }
        if let Some(snapshot) = &next.snapshot
            && self
                .history
                .snapshot
                .as_ref()
                .is_some_and(|previous| previous.page.revision != snapshot.page.revision)
        {
            self.reveal = None;
            self.expanded
                .retain(|(turn, _), _| snapshot.page.runs.iter().any(|run| run.turn == *turn));
        }
        let changed = next.snapshot.as_ref().is_some_and(|snapshot| {
            self.history.snapshot.as_ref().is_none_or(|previous| {
                !Arc::ptr_eq(&previous.page, &snapshot.page) || previous.drafts != snapshot.drafts
            })
        });
        self.approvals.observe(&next);
        self.history = next;
        if changed || self.outgoing.is_some() {
            self.sync_rows(cx);
        }
        self.refresh_repository(repository::Refresh::History, cx);
        self.refresh_clock(cx);
        self.reveal_loaded(cx);
        cx.notify();
    }
    pub(super) fn sync_rows(&mut self, cx: &mut Context<Self>) {
        self.reconcile_outgoing();
        let mut rows =
            self.history
                .snapshot
                .as_ref()
                .map(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .filter(|run| {
                            run.status != Status::Queued
                                && (run.status != Status::Cancelled
                                    || run.kind == RunKind::Compaction
                                    || snapshot
                                        .page
                                        .entries
                                        .iter()
                                        .any(|entry| entry.turn == run.turn))
                        })
                        .filter(|run| {
                            // A run can arrive before both its user event and the Node
                            // admission snapshot. Do not put an unpaired response above
                            // the local message while its exact identity is still unknown.
                            self.outgoing.as_ref().is_none_or(|outgoing| {
                                outgoing.turn == Some(run.turn)
                                    || run.kind != RunKind::Task
                                    || !matches!(run.status, Status::Running | Status::Stopping)
                                    || snapshot.page.entries.iter().any(|entry| {
                                        entry.turn == run.turn && entry.author == "user"
                                    })
                            })
                        })
                        .map(|run| run.turn)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
        if let Some(outgoing) = &self.outgoing
            && !outgoing.turn.is_some_and(|turn| rows.contains(&turn))
        {
            rows.push(outgoing.row);
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
            scroller.remeasure_items(0..rows.len(), cx);
        });
        let prefixes: Vec<_> = rows.iter().map(ToString::to_string).collect();
        self.texts
            .borrow_mut()
            .retain(|key, _| prefixes.iter().any(|prefix| key.starts_with(prefix)));
        self.rows = rows;
        self.changes.retain(|turn, _| self.rows.contains(turn));
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
