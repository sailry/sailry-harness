use super::{Result, State};
use sailry_client::{
    Client,
    conversation::{HistoryRequest, View},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    conversation::{ApprovalState, Page, Part, Status, question},
    *,
};
use std::{collections::BTreeSet, num::NonZeroU64, sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
};

enum Event {
    View(SessionId, TurnId, View),
    Failed(Fault),
}

struct Observers {
    stop: CancellationToken,
    tasks: JoinSet<()>,
    sender: mpsc::Sender<Event>,
    turns: BTreeSet<(SessionId, TurnId)>,
}

impl Drop for Observers {
    fn drop(&mut self) {
        self.stop.cancel();
        self.tasks.abort_all();
    }
}

impl Observers {
    fn add(
        &mut self,
        client: Arc<Client>,
        session: SessionId,
        turn: TurnId,
        sequence: u64,
    ) -> std::result::Result<(), Fault> {
        if !self.turns.insert((session, turn)) {
            return Ok(());
        }
        let sequence = NonZeroU64::new(sequence)
            .ok_or_else(|| Fault::invalid("turn admission sequence is missing"))?;
        let (updates, mut receiver) = watch::channel(View::default());
        let (requests, requested) = mpsc::channel(1);
        requests
            .try_send(HistoryRequest::Through(sequence))
            .map_err(|_| {
                Fault::new(
                    ErrorCode::Internal,
                    "history request channel is unavailable",
                )
            })?;
        let stop = self.stop.clone();
        let sender = self.sender.clone();
        self.tasks.spawn(async move {
            let _requests = requests;
            if let Err(error) = client
                .watch_conversation(session, updates, stop, requested)
                .await
            {
                let _ = sender.send(Event::Failed(error)).await;
            }
        });
        let sender = self.sender.clone();
        self.tasks.spawn(async move {
            while receiver.changed().await.is_ok() {
                let view = receiver.borrow_and_update().clone();
                if sender.send(Event::View(session, turn, view)).await.is_err() {
                    break;
                }
            }
        });
        Ok(())
    }
}

pub(super) async fn wait(
    client: Arc<Client>,
    session: SessionId,
    turn: TurnId,
    timeout: u64,
    stop: &CancellationToken,
) -> std::result::Result<Result, Fault> {
    let mut result = Result::new(Some(client.target()));
    result.session = Some(session);
    result.turn = Some(turn);
    let deadline = tokio::time::sleep(Duration::from_secs(timeout));
    tokio::pin!(deadline);
    let run = tokio::select! {
        _ = stop.cancelled() => { result.state = State::ObservationCancelled; return Ok(result); },
        _ = &mut deadline => { result.state = State::TimedOut; return Ok(result); },
        run = locate(&client, session, turn) => run?,
    };
    result.revision = Some(run.revision);
    let outcome: std::result::Result<(), Fault> = async {
    let (sender, mut events) = mpsc::channel(32);
    let mut observers = Observers { stop: CancellationToken::new(), tasks: JoinSet::new(), sender, turns: BTreeSet::new() };
    observers.add(client.clone(), session, turn, run.sequence)?;
    loop {
        let event = tokio::select! {
            _ = stop.cancelled() => { result.state = State::ObservationCancelled; return Ok(()); },
            _ = &mut deadline => { result.state = State::TimedOut; return Ok(()); },
            event = events.recv() => event.ok_or_else(|| Fault::new(ErrorCode::Unavailable, "conversation observation closed"))?,
        };
        let (source, current, view) = match event {
            Event::View(source, current, view) => (source, current, view),
            Event::Failed(error) => return Err(error),
        };
        if let Some(error) = &view.error
            && !matches!(error.code, ErrorCode::Unavailable | ErrorCode::Busy) {
            return Err(error.clone());
        }
        if let Some(error) = &view.older_error { return Err(error.clone()); }
        if !view.connected { continue; }
        let Some(snapshot) = view.snapshot else { continue; };
        let page = snapshot.page.as_ref();
        if source == session && current == turn {
            result.text = text(page, turn);
        }
        for child in &page.children {
            if child.origin.turn == current && matches!(child.run.status, Status::Queued | Status::Running | Status::Stopping) {
                observers.add(client.clone(), child.run.session, child.run.turn, child.run.sequence)?;
            }
        }
        let approvals: Vec<_> = page.approvals.iter().filter(|approval| approval.turn == current && approval.state == ApprovalState::Pending).cloned().collect();
        let questions: Vec<_> = page.questions.iter().filter(|question| question.turn == current && question.state == question::State::Pending).cloned().collect();
        if !approvals.is_empty() || !questions.is_empty() {
            result.state = if approvals.is_empty() { State::AwaitingInput } else { State::AwaitingApproval };
            result.approvals = approvals;
            result.questions = questions;
            return Ok(());
        }
        if source != session || current != turn { continue; }
        if page.queue.paused && page.queue.items.iter().any(|item| item.turn == turn) {
            result.state = State::Paused;
            return Ok(());
        }
        let Some(run) = page.runs.iter().find(|run| run.turn == turn) else { continue; };
        if run.status == Status::Completed && page.children.iter().any(|child|
            child.origin.turn == turn && matches!(child.run.status, Status::Queued | Status::Running | Status::Stopping)) {
            continue;
        }
        result.state = match run.status {
            Status::Queued | Status::Running | Status::Stopping => continue,
            Status::Completed => State::Completed,
            Status::Continued => State::Continued,
            Status::Cancelled => State::Cancelled,
            Status::Interrupted => State::Interrupted,
            Status::Failed => State::Failed,
        };
        result.error = run.error.clone();
        return Ok(());
    }
    }.await;
    if let Err(error) = outcome {
        result.state = State::Uncertain;
        result.error = Some(error);
    }
    Ok(result)
}

async fn locate(
    client: &Client,
    session: SessionId,
    turn: TurnId,
) -> std::result::Result<conversation::Run, Fault> {
    let mut before = None;
    loop {
        let history = client.read_conversation(session, before, 20).await?;
        if let Some(run) = history.page.runs.iter().find(|run| run.turn == turn) {
            return Ok(run.clone());
        }
        match history.page.next_before {
            Some(cursor) if Some(cursor) != before => before = Some(cursor),
            _ => {
                return Err(Fault::new(
                    ErrorCode::NotFound,
                    "turn is not in this conversation",
                ));
            }
        }
    }
}

fn text(page: &Page, turn: TurnId) -> String {
    let mut text = String::new();
    for entry in &page.entries {
        if entry.turn != turn || entry.author == "user" {
            continue;
        }
        let parts: Vec<_> = entry
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        if !parts.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.extend(parts);
        }
    }
    text
}
