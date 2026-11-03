use super::*;
use crate::Apply;
use sailry_link::Pending;
use sailry_protocol::conversation::{History, TurnHistory};
use std::time::Duration;
use tokio::{sync::mpsc, time::Instant};

#[cfg(test)]
mod tests;

/// Automatic resubscription keeps the current view and stays silent until it lasts this long.
const GRACE: Duration = Duration::from_secs(5);

enum Loaded {
    Page {
        before: TurnId,
        history: Box<History>,
    },
    Turn(Box<TurnHistory>),
}

impl Loaded {
    fn sequence(&self) -> u64 {
        match self {
            Self::Page { history, .. } => history.sequence,
            Self::Turn(chunk) => chunk.sequence,
        }
    }
}

enum Load<'a> {
    Reading(Pending<'a, Result<Loaded, Fault>>),
    Ready(Box<Loaded>),
}

impl<'a> Load<'a> {
    /// The oldest partial turn continues before older turn pages are read.
    fn next(client: &'a Client, snapshot: &Snapshot) -> Option<Self> {
        let page = &snapshot.page;
        let session = page.session;
        if let Some(turn) = partial(snapshot) {
            let revision = page.revision;
            let before = earliest(snapshot, turn);
            return Some(Self::Reading(Box::pin(async move {
                client
                    .read_turn(session, turn, revision, before, super::history::TURN_PAGE)
                    .await
                    .map(|chunk| Loaded::Turn(Box::new(chunk)))
            })));
        }
        let before = page.next_before?;
        Some(Self::Reading(Box::pin(async move {
            client
                .read_page(session, Some(before), 20)
                .await
                .map(|history| Loaded::Page {
                    before,
                    history: Box::new(history),
                })
        })))
    }
}

/// The oldest turn whose earlier entries are not loaded yet.
fn partial(snapshot: &Snapshot) -> Option<TurnId> {
    snapshot
        .page
        .runs
        .iter()
        .filter(|run| snapshot.missing.contains(&run.turn))
        .min_by_key(|run| run.sequence)
        .map(|run| run.turn)
}

fn earliest(snapshot: &Snapshot, turn: TurnId) -> Option<u64> {
    snapshot
        .page
        .entries
        .iter()
        .find(|entry| entry.turn == turn)
        .map(|entry| entry.sequence)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Extent {
    #[default]
    Recent,
    /// Complete turns from this admission sequence onward.
    Through(u64),
    /// Restore a partially loaded oldest turn down to this entry.
    Partial {
        turn: u64,
        entry: Option<u64>,
    },
    All,
}

impl Extent {
    fn from(snapshot: &Snapshot) -> Self {
        let page = &snapshot.page;
        let sequence = |turn: TurnId| {
            page.runs
                .iter()
                .find(|run| run.turn == turn)
                .expect("validated turn")
                .sequence
        };
        if let Some(turn) = partial(snapshot) {
            return Self::Partial {
                turn: sequence(turn),
                entry: earliest(snapshot, turn),
            };
        }
        match page.next_before {
            Some(turn) => Self::Through(sequence(turn)),
            None if page.runs.is_empty() => Self::Recent,
            None => Self::All,
        }
    }

    fn needs_more(self, snapshot: &Snapshot) -> bool {
        let page = &snapshot.page;
        let sequence = |turn: TurnId| {
            page.runs
                .iter()
                .find(|run| run.turn == turn)
                .map(|run| run.sequence)
        };
        let older = |bound: u64| {
            page.next_before
                .and_then(sequence)
                .is_some_and(|oldest| oldest > bound)
        };
        match self {
            Self::Recent => false,
            Self::All => page.next_before.is_some() || !snapshot.missing.is_empty(),
            Self::Through(bound) => {
                older(bound)
                    || snapshot
                        .missing
                        .iter()
                        .any(|turn| sequence(*turn).is_some_and(|current| current >= bound))
            }
            Self::Partial { turn, entry } => {
                older(turn)
                    || snapshot
                        .missing
                        .iter()
                        .any(|current| match sequence(*current) {
                            Some(value) if value > turn => true,
                            Some(value) if value == turn => {
                                match (earliest(snapshot, *current), entry) {
                                    (_, None) => false,
                                    (None, Some(_)) => true,
                                    (Some(first), Some(entry)) => first > entry,
                                }
                            }
                            _ => false,
                        })
            }
        }
    }
}

impl Client {
    /// Restore the loaded turn range on reconnect without resubmitting work.
    /// Repeated older-page requests are coalesced while a read is pending. The latest
    /// requested turn range is retained; progress and failures are published in `updates`.
    pub async fn watch_conversation(
        &self,
        session: SessionId,
        updates: watch::Sender<View>,
        stop: CancellationToken,
        requests: mpsc::Receiver<HistoryRequest>,
    ) -> Result<(), Fault> {
        let cancellation = stop.child_token();
        let history = async {
            let result = self
                .watch_history(session, updates.clone(), cancellation.clone(), requests)
                .await;
            if matches!(&result, Err(error) if error.code == ErrorCode::NotFound) {
                // A history miss alone is not deletion; wait for the Node confirmation.
                std::future::pending::<Result<(), Fault>>().await
            } else {
                result
            }
        };
        let result = tokio::select! {
            result = history => result,
            result = super::removal::watch(self, session, updates.clone(), cancellation.clone()) => result,
        };
        cancellation.cancel();
        result
    }

    async fn watch_history(
        &self,
        session: SessionId,
        updates: watch::Sender<View>,
        stop: CancellationToken,
        mut requests: mpsc::Receiver<HistoryRequest>,
    ) -> Result<(), Fault> {
        let mut log = crate::observation::Observation::new(self.target(), Some(session));
        let result = async {
            let _closing = Closing(updates.clone());
            let mut projection = Projection::new(self.target(), session, 0);
            let mut generation = 0;
            let mut delay = Duration::from_millis(250);
            let mut extent = Extent::Recent;
            let mut requested: Option<Extent> = None;
            let mut requests_open = true;
            // A retried interruption keeps the last view until it outlasts the grace period.
            let mut outage: Option<(Instant, Fault)> = None;
            let mut reported = false;
            loop {
                generation += 1;
                log.connecting(generation);
                projection.reconnect(generation)?;
                let subscribing = self.subscribe_conversation(session);
                tokio::pin!(subscribing);
                let stream = loop {
                    tokio::select! {
                        _ = stop.cancelled() => return Ok(()),
                        _ = updates.closed() => return Ok(()),
                        stream = &mut subscribing => break stream,
                        _ = overdue(&outage), if !reported => {
                            reported = report(&updates, &outage);
                        }
                    }
                };
                let mut load: Option<Load> = None;
                let mut restored = false;
                let mut ready = false;
                let mut changed = false;
                let failure = match stream {
                    Ok(mut stream) => {
                        log.connected();
                        'connected: loop {
                            // A transport decoder may hold a partial frame. Retain its future across
                            // paging notifications instead of cancelling and restarting the read.
                            let incoming = stream.next();
                            tokio::pin!(incoming);
                            let update = loop {
                                if let Some(Load::Ready(loaded)) = &load
                                    && projection
                                        .snapshot()
                                        .is_some_and(|snapshot| snapshot.sequence >= loaded.sequence())
                                {
                                    let Some(Load::Ready(loaded)) = load.take() else {
                                        unreachable!()
                                    };
                                    let applied = match *loaded {
                                        Loaded::Page { before, history } => {
                                            projection.merge_history(generation, Some(before), *history)
                                        }
                                        Loaded::Turn(chunk) => projection.merge_turn(generation, *chunk),
                                    };
                                    match applied {
                                        // A superseded read is re-planned from the current view.
                                        Ok(Apply::Applied | Apply::Ignored) => changed = true,
                                        Ok(Apply::Recover) => {
                                            log.recovering("history", "history_changed");
                                            break 'connected recovery();
                                        }
                                        Err(error) => break 'connected error,
                                    }
                                }
                                if changed {
                                    if let Some(snapshot) = projection.snapshot() {
                                        // Restore the previously loaded range; a recent view publishes
                                        // at once and reads partial turns only on request.
                                        if !restored {
                                            if !extent.needs_more(snapshot) {
                                                restored = true;
                                            } else if load.is_none() {
                                                load = Load::next(self, snapshot);
                                                restored = load.is_none();
                                            }
                                        }
                                        if restored {
                                            if !ready {
                                                log.ready(snapshot.sequence);
                                                ready = true;
                                                outage = None;
                                                reported = false;
                                            }
                                            extent = Extent::from(snapshot);
                                            if requested.is_some_and(|range| !range.needs_more(snapshot)) {
                                                requested = None;
                                            }
                                            if requested.is_some() && load.is_none() {
                                                load = Load::next(self, snapshot);
                                                if load.is_none() {
                                                    requested = None;
                                                }
                                            }
                                            self.details.reconcile(&snapshot.page);
                                            updates.send_modify(|view| {
                                                view.replace(
                                                    Arc::new(snapshot.clone()),
                                                    projection.artifact_cursor(),
                                                    generation,
                                                    projection.live_start(),
                                                );
                                                view.loading_older = load.is_some();
                                                view.statistics_error = projection.statistics_error().cloned();
                                            });
                                        }
                                    }
                                    changed = false;
                                }
                                let reading = matches!(load, Some(Load::Reading(_)));
                                tokio::select! {
                                    _ = stop.cancelled() => return Ok(()),
                                    _ = updates.closed() => return Ok(()),
                                    update = &mut incoming => break update,
                                    _ = overdue(&outage), if !reported => {
                                        reported = report(&updates, &outage);
                                    }
                                    result = async {
                                        match load.as_mut() {
                                            Some(Load::Reading(future)) => future.await,
                                            _ => std::future::pending().await,
                                        }
                                    }, if reading => {
                                        // Drop repeated page clicks, but never lose a newer search target.
                                        while let Ok(request) = requests.try_recv() {
                                            if let HistoryRequest::Through(sequence) = request {
                                                requested = Some(Extent::Through(sequence.get()));
                                            }
                                        }
                                        load = None;
                                        match result {
                                            Ok(loaded) => load = Some(Load::Ready(Box::new(loaded))),
                                            Err(error) if error.code == ErrorCode::RevisionConflict => {
                                                log.recovering("history", "revision_conflict");
                                                break 'connected recovery();
                                            }
                                            Err(error) if restored => {
                                                log.issue("history", &error);
                                                requested = None;
                                                updates.send_modify(|view| {
                                                    view.loading_older = false;
                                                    view.older_error = Some(error);
                                                });
                                            }
                                            Err(error) => break 'connected error,
                                        }
                                    }
                                    request = requests.recv(), if requests_open => {
                                        match request {
                                            None => requests_open = false,
                                            Some(HistoryRequest::Through(sequence)) => {
                                                requested = Some(Extent::Through(sequence.get()));
                                                changed = true;
                                                updates.send_modify(|view| view.older_error = None);
                                            }
                                            Some(HistoryRequest::Older) if restored && load.is_none() => {
                                                load = projection
                                                    .snapshot()
                                                    .and_then(|snapshot| Load::next(self, snapshot));
                                                if load.is_some() {
                                                    updates.send_modify(|view| {
                                                        view.loading_older = true;
                                                        view.older_error = None;
                                                    });
                                                }
                                            }
                                            Some(HistoryRequest::Older) => {}
                                        }
                                    }
                                }
                            };
                            match update.and_then(|update| projection.apply(generation, update)) {
                                Ok(Apply::Applied) => {
                                    changed = true;
                                    delay = Duration::from_millis(250);
                                }
                                Ok(Apply::Ignored) => {}
                                Ok(Apply::Recover) => {
                                    log.recovering("projection", "sequence_gap");
                                    break recovery();
                                }
                                Err(error) => break error,
                            }
                        }
                    }
                    Err(error) => error,
                };
                log.disconnected(&failure, delay);
                updates.send_modify(|view| view.loading_older = false);
                if !matches!(failure.code, ErrorCode::Unavailable | ErrorCode::Busy) {
                    report(&updates, &Some((Instant::now(), failure.clone())));
                    return Err(failure);
                }
                // The previous view stays current and silent while retries recover it.
                let since = outage.as_ref().map_or_else(Instant::now, |(since, _)| *since);
                outage = Some((since, failure));
                if reported || since.elapsed() >= GRACE {
                    reported = report(&updates, &outage);
                }
                tokio::select! {
                    _ = stop.cancelled() => return Ok(()),
                    _ = updates.closed() => return Ok(()),
                    _ = tokio::time::sleep(delay) => {},
                }
                delay = (delay * 2).min(Duration::from_secs(5));
            }
        }
        .await;
        log.finish(result.as_ref().err());
        result
    }
}

/// Resolves once a retried interruption outlasts the grace period.
async fn overdue(outage: &Option<(Instant, Fault)>) {
    match outage {
        Some((since, _)) => tokio::time::sleep_until(*since + GRACE).await,
        None => std::future::pending().await,
    }
}

/// Publish an interruption; returns whether one was published.
fn report(updates: &watch::Sender<View>, outage: &Option<(Instant, Fault)>) -> bool {
    let Some((_, failure)) = outage else {
        return false;
    };
    updates.send_modify(|view| {
        view.connected &= failure.code == ErrorCode::Busy;
        view.loading_older = false;
        view.error = Some(failure.clone());
    });
    true
}

fn recovery() -> Fault {
    Fault::new(ErrorCode::Unavailable, "conversation requires recovery")
}
