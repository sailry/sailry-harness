use super::*;
use crate::Apply;
use sailry_link::Pending;
use sailry_protocol::conversation::History;
use std::time::Duration;
use tokio::sync::mpsc;

enum Load<'a> {
    Reading {
        before: Option<TurnId>,
        future: Pending<'a, Result<History, Fault>>,
    },
    Ready {
        before: Option<TurnId>,
        history: Box<History>,
    },
}

impl<'a> Load<'a> {
    fn older(client: &'a Client, session: SessionId, before: TurnId) -> Self {
        Self::Reading {
            before: Some(before),
            future: Box::pin(client.read_conversation(session, Some(before), 20)),
        }
    }

    fn hydrate(client: &'a Client, snapshot: &Snapshot) -> Self {
        let history = History {
            sequence: snapshot.sequence,
            page: (*snapshot.page).clone(),
            missing: snapshot.missing.clone(),
        };
        Self::Reading {
            before: None,
            future: Box::pin(client.complete_history(history)),
        }
    }
}

#[derive(Clone, Copy, Default)]
enum Extent {
    #[default]
    Recent,
    Through(u64),
    All,
}

impl Extent {
    fn from(snapshot: &Snapshot) -> Self {
        match snapshot.page.next_before {
            Some(turn) => Self::Through(
                snapshot
                    .page
                    .runs
                    .iter()
                    .find(|run| run.turn == turn)
                    .expect("validated turn cursor")
                    .sequence,
            ),
            None if snapshot.page.runs.is_empty() => Self::Recent,
            None => Self::All,
        }
    }

    fn needs_more(self, snapshot: &Snapshot) -> bool {
        let Some(turn) = snapshot.page.next_before else {
            return false;
        };
        match self {
            Self::Recent => false,
            Self::All => true,
            Self::Through(sequence) => snapshot
                .page
                .runs
                .iter()
                .any(|run| run.turn == turn && run.sequence > sequence),
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
        mut requests: mpsc::Receiver<HistoryRequest>,
    ) -> Result<(), Fault> {
        let _closing = Closing(updates.clone());
        let mut projection = Projection::new(self.target(), session, 0);
        let mut generation = 0;
        let mut delay = Duration::from_millis(250);
        let mut extent = Extent::Recent;
        let mut requested: Option<Extent> = None;
        let mut requests_open = true;
        loop {
            generation += 1;
            projection.reconnect(generation)?;
            let stream = tokio::select! {
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                stream = self.subscribe_conversation(session) => stream,
            };
            let mut load = None;
            let mut restored = false;
            let mut changed = false;
            let failure = match stream {
                Ok(mut stream) => 'connected: loop {
                    // A transport decoder may hold a partial frame. Retain its future across
                    // paging notifications instead of cancelling and restarting the read.
                    let incoming = stream.next();
                    tokio::pin!(incoming);
                    let update = loop {
                        if let Some(Load::Ready { history, .. }) = &load
                            && projection
                                .snapshot()
                                .is_some_and(|snapshot| snapshot.sequence >= history.sequence)
                        {
                            let Some(Load::Ready { before, history }) = load.take() else {
                                unreachable!()
                            };
                            match projection.merge_history(generation, before, *history) {
                                Ok(Apply::Applied) => {
                                    changed = true;
                                }
                                Ok(_) => break 'connected recovery(),
                                Err(error) => break 'connected error,
                            }
                        }
                        if changed {
                            if let Some(snapshot) = projection.snapshot() {
                                if load.is_none() && !snapshot.missing.is_empty() {
                                    load = Some(Load::hydrate(self, snapshot));
                                } else if load.is_none() && !restored && extent.needs_more(snapshot)
                                {
                                    load = Some(Load::older(
                                        self,
                                        session,
                                        snapshot.page.next_before.expect("older turn range"),
                                    ));
                                } else if snapshot.missing.is_empty()
                                    && !extent.needs_more(snapshot)
                                {
                                    restored = true;
                                }
                                if restored {
                                    extent = Extent::from(snapshot);
                                    if requested.is_some_and(|range| !range.needs_more(snapshot)) {
                                        requested = None;
                                    }
                                    if requested.is_some() && load.is_none() {
                                        load = Some(Load::older(
                                            self,
                                            session,
                                            snapshot
                                                .page
                                                .next_before
                                                .expect("requested turn range"),
                                        ));
                                    }
                                    updates.send_modify(|view| {
                                        view.replace(Arc::new(snapshot.clone()));
                                        view.loading_older = load.is_some();
                                    });
                                }
                            }
                            changed = false;
                        }
                        let reading = matches!(load, Some(Load::Reading { .. }));
                        tokio::select! {
                            _ = stop.cancelled() => return Ok(()),
                            _ = updates.closed() => return Ok(()),
                            update = &mut incoming => break update,
                            result = async {
                                match load.as_mut() {
                                    Some(Load::Reading { future, .. }) => future.await,
                                    _ => std::future::pending().await,
                                }
                            }, if reading => {
                                // Drop repeated page clicks, but never lose a newer search target.
                                while let Ok(request) = requests.try_recv() {
                                    if let HistoryRequest::Through(sequence) = request {
                                        requested = Some(Extent::Through(sequence.get()));
                                    }
                                }
                                let Some(Load::Reading { before, .. }) = load.take() else { unreachable!() };
                                match result {
                                    Ok(history) => load = Some(Load::Ready { before, history: Box::new(history) }),
                                    Err(error) if error.code == ErrorCode::RevisionConflict => break 'connected recovery(),
                                    Err(error) if restored => {
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
                                        if let Some(before) = projection.snapshot().and_then(|snapshot| snapshot.page.next_before) {
                                            load = Some(Load::older(self, session, before));
                                            updates.send_modify(|view| { view.loading_older = true; view.older_error = None; });
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
                        Ok(Apply::Recover) => break recovery(),
                        Err(error) => break error,
                    }
                },
                Err(error) => error,
            };
            updates.send_modify(|view| {
                view.connected = false;
                view.loading_older = false;
                view.error = Some(failure.clone());
            });
            if !matches!(failure.code, ErrorCode::Unavailable | ErrorCode::Busy) {
                return Err(failure);
            }
            tokio::select! {
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                _ = tokio::time::sleep(delay) => {},
            }
            delay = (delay * 2).min(Duration::from_secs(5));
        }
    }
}

fn recovery() -> Fault {
    Fault::new(ErrorCode::Unavailable, "conversation requires recovery")
}
