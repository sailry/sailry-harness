//! Provider execution and artifacts retain Code Pi d9b56405 behavior and ADK history.
use super::*;
use sailry_protocol::{
    Command, NodeId, Output, Request, SessionId, WorktreeId,
    media::{Action, Kind, Source},
};
use serde_json::{Value, json};
mod generation;
mod usage;
mod vision;

/// Model snapshots belong to the admitted turn, never current global settings.
#[derive(Clone)]
pub(crate) struct Binding {
    session: SessionId,
    worktree: WorktreeId,
    config: sailry_protocol::SessionConfig,
    models: crate::store::media::Snapshot,
}

impl Binding {
    pub(super) fn new(invocation: &Invocation) -> Self {
        Self {
            session: invocation.turn.session,
            worktree: invocation.worktree,
            config: invocation.turn.config.clone(),
            models: invocation.media.clone(),
        }
    }
}

struct Media {
    ingress: Arc<Ingress>,
    turn: TurnId,
    session: String,
    worktree: WorktreeId,
    config: sailry_protocol::SessionConfig,
    kind: Kind,
    model: crate::store::media::Model,
    stop: CancellationToken,
}

pub(crate) fn validate(action: &Action) -> Result<(), Fault> {
    match action {
        Action::Inspect {
            source: Source::Path(path),
            ..
        } => {
            crate::files::path::components(path, false)?;
        }
        Action::Image { path, .. } | Action::Video { path, .. } => {
            crate::files::path::entry_components(path)?;
        }
        Action::Inspect {
            source: Source::Attachment(_),
            ..
        } => {}
    }
    Ok(())
}

pub(crate) async fn run(
    ingress: &Arc<Ingress>,
    caller: NodeId,
    request: &Request,
    stop: CancellationToken,
) -> Result<Output, Fault> {
    let Command::UseMedia {
        turn,
        session,
        worktree,
        action,
    } = &request.command
    else {
        unreachable!("media command expected")
    };
    ingress.check_plugin(caller, request.clone()).await?;
    validate(action)?;
    let binding = ingress.agents.media(*turn).ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "admitted media configuration is unavailable",
        )
    })?;
    if binding.session != *session || binding.worktree != *worktree {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "media operation is outside the admitted turn",
        ));
    }
    let kind = action.kind();
    let model = binding.models.get(&kind).cloned().ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "media model is unavailable for this turn",
        )
    })?;
    let media = Media {
        ingress: ingress.clone(),
        turn: *turn,
        session: session.to_string(),
        worktree: *worktree,
        config: binding.config,
        kind,
        model,
        stop: stop.clone(),
    };
    let work = async {
        match action {
            Action::Inspect { prompt, source } => {
                media.inspect(prompt.clone(), source.clone()).await
            }
            Action::Image { prompt, path } | Action::Video { prompt, path } => {
                media.generate(prompt.clone(), path.clone()).await
            }
        }
    };
    tokio::select! {
        biased;
        _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "media operation stopped; a submitted provider job may continue")),
        result = tokio::time::timeout(Duration::from_secs(if kind == Kind::Video {540} else {240}), work) => {
            result.map_err(|_| Fault::new(ErrorCode::OutcomeUnknown,"media deadline exceeded; provider outcome may be uncertain"))?.map(Output::Media)
        }
    }
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
