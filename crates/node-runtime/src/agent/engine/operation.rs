use super::{Effect, Session};
use crate::agent::model::Message;
use async_trait::async_trait;
use sailry_link::CancellationToken;
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RunStatus {
    Continuing,
    Yielded,
    Cancelled,
}

pub(crate) struct Transition {
    pub effects: Vec<Effect>,
    pub status: RunStatus,
}

pub(crate) enum Outcome {
    NotApplicable,
    Applied(Transition),
}

pub(crate) fn not_applicable() -> anyhow::Result<Outcome> {
    Ok(Outcome::NotApplicable)
}

pub(crate) fn applied(effects: impl IntoIterator<Item = Effect>) -> anyhow::Result<Outcome> {
    Ok(Outcome::Applied(Transition {
        effects: effects.into_iter().collect(),
        status: RunStatus::Continuing,
    }))
}

pub(crate) fn yielded() -> anyhow::Result<Outcome> {
    yielded_with([])
}

pub(crate) fn yielded_with(effects: impl IntoIterator<Item = Effect>) -> anyhow::Result<Outcome> {
    Ok(Outcome::Applied(Transition {
        effects: effects.into_iter().collect(),
        status: RunStatus::Yielded,
    }))
}

#[derive(Clone)]
pub(crate) struct Emitter {
    sender: mpsc::UnboundedSender<Message>,
    stop: CancellationToken,
}

impl Emitter {
    pub(crate) fn new(sender: mpsc::UnboundedSender<Message>, stop: CancellationToken) -> Self {
        Self { sender, stop }
    }

    pub(crate) fn message(&self, mut message: Message) -> Message {
        message.ensure_id();
        let _ = self.sender.send(message.clone());
        message
    }

    pub(crate) fn cancel_token(&self) -> &CancellationToken {
        &self.stop
    }

    pub(crate) async fn cancelled(&self) {
        self.stop.cancelled().await;
    }

    pub(crate) fn scoped(&self, stop: CancellationToken) -> Self {
        Self {
            sender: self.sender.clone(),
            stop,
        }
    }
}

#[async_trait]
pub(crate) trait Callback: Send + Sync {
    async fn run(
        &self,
        session: &Session,
        conversation: &[Message],
        emit: &Emitter,
    ) -> anyhow::Result<Outcome>;
}
