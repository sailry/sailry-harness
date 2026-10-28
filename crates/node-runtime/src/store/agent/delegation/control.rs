//! Child control resolves canonical ancestry and reads the existing ADK history.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(crate) struct State {
    pub run: Run,
    pub response: Option<String>,
    pub paused: bool,
}

impl Ingress {
    pub(crate) async fn child_state(
        &self,
        parent: TurnId,
        child: SessionId,
    ) -> Result<State, Fault> {
        self.child_state_at(parent, child, None).await
    }

    pub(crate) async fn child_state_at(
        &self,
        parent: TurnId,
        child: SessionId,
        exact: Option<TurnId>,
    ) -> Result<State, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::ChildState {
            parent,
            child,
            exact,
            reply,
        })))
        .await
        .map_err(|_| super::super::super::unavailable())?;
        response
            .await
            .map_err(|_| super::super::super::unavailable())?
    }

    pub(crate) async fn pending_guidance(&self, recipient: TurnId) -> Result<bool, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::PendingGuidance {
            recipient,
            reply,
        })))
        .await
        .map_err(|_| super::super::super::unavailable())?;
        response
            .await
            .map_err(|_| super::super::super::unavailable())?
    }
}

pub(in crate::store) async fn owned(
    db: &Connection,
    parent: TurnId,
    child: SessionId,
) -> Result<Run, Fault> {
    let parent = calls::active(db, parent).await?;
    let founding = read(db, child).await?.ok_or_else(|| {
        Fault::new(
            ErrorCode::PermissionDenied,
            "recipient is not a delegated child",
        )
    })?;
    if read(db, parent.session).await?.is_some() || founding.session != parent.session {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "child belongs to another parent session",
        ));
    }
    let body: Vec<u8> = {
        let mut statement = db.prepare("SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.joined') IS NULL ORDER BY CASE json_extract(a.body,'$.status') WHEN 'running' THEN 0 WHEN 'stopping' THEN 0 WHEN 'queued' THEN 1 ELSE 2 END,t.rowid DESC LIMIT 1").await.map_err(storage_error)?;
        let row = statement
            .query_row([child.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    let run: Run = serde_json::from_slice(&body).map_err(storage_error)?;
    let association = origin(db, run.turn)
        .await?
        .ok_or_else(|| storage_error("child run has no delegation"))?;
    if association.session != founding.session || association.role != founding.role {
        return Err(storage_error(
            "child ownership differs from its founding delegation",
        ));
    }
    Ok(run)
}

#[cfg(test)]
pub(in crate::store) async fn state(
    db: &Connection,
    parent: TurnId,
    child: SessionId,
) -> Result<State, Fault> {
    state_at(db, parent, child, None).await
}

pub(in crate::store) async fn state_at(
    db: &Connection,
    parent: TurnId,
    child: SessionId,
    exact: Option<TurnId>,
) -> Result<State, Fault> {
    let latest = owned(db, parent, child).await?;
    let run = if let Some(turn) = exact {
        let run = runs::get(db, turn).await?;
        let parent = calls::active(db, parent).await?;
        let origin = origin(db, turn).await?.ok_or_else(|| {
            Fault::new(
                ErrorCode::PermissionDenied,
                "turn is not a delegated execution",
            )
        })?;
        if run.session != child || origin.session != parent.session {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "child execution belongs to another owner",
            ));
        }
        run
    } else {
        latest
    };
    let response = if run.status == Status::Completed {
        response(db, run.turn).await?
    } else {
        None
    };
    let paused = queue::read(db, child).await?.paused;
    Ok(State {
        run,
        response,
        paused,
    })
}

pub(in crate::store) async fn response(
    db: &Connection,
    turn: TurnId,
) -> Result<Option<String>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM agent_events WHERE turn=?1 ORDER BY sequence DESC")
        .await
        .map_err(storage_error)?;
    let mut response = None;
    let mut rows = query
        .query([turn.to_string()])
        .await
        .map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let body = row.get::<Vec<u8>>(0).map_err(storage_error)?;
        let event: crate::agent::canonical::Record =
            serde_json::from_slice(&body).map_err(storage_error)?;
        if let Some(message) = event.message().filter(|message| {
            message.role == crate::agent::canonical::Role::Assistant
                && message.content.iter().all(|block| {
                    !matches!(
                        &block.kind,
                        crate::agent::canonical::BlockKind::ToolRequest(_)
                            | crate::agent::canonical::BlockKind::ToolResponse(_)
                    )
                })
        }) {
            let text: String = message
                .content
                .iter()
                .filter_map(|block| match &block.kind {
                    crate::agent::canonical::BlockKind::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            if !text.is_empty() {
                response = Some(text);
                break;
            }
        }
    }
    Ok(response)
}
