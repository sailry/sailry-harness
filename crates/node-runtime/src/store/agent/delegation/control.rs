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
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::ChildState {
                parent,
                child,
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
        self.sender
            .send(Job::Agent(Box::new(Operation::PendingGuidance {
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

pub(in crate::store) fn owned(
    db: &Connection,
    parent: TurnId,
    child: SessionId,
) -> Result<Run, Fault> {
    let parent = calls::active(db, parent)?;
    let founding = read(db, child)?.ok_or_else(|| {
        Fault::new(
            ErrorCode::PermissionDenied,
            "recipient is not a delegated child",
        )
    })?;
    if read(db, parent.session)?.is_some() || founding.session != parent.session {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "child belongs to another parent session",
        ));
    }
    let body: Vec<u8> = db.query_row(
        "SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 ORDER BY CASE json_extract(a.body,'$.status') WHEN 'running' THEN 0 WHEN 'stopping' THEN 0 WHEN 'queued' THEN 1 ELSE 2 END,t.rowid DESC LIMIT 1",
        [child.to_string()], |row| row.get(0),
    ).map_err(storage_error)?;
    let run: Run = serde_json::from_slice(&body).map_err(storage_error)?;
    let association =
        origin(db, run.turn)?.ok_or_else(|| storage_error("child run has no delegation"))?;
    if association.session != founding.session || association.role != founding.role {
        return Err(storage_error(
            "child ownership differs from its founding delegation",
        ));
    }
    Ok(run)
}

pub(in crate::store) fn state(
    db: &Connection,
    parent: TurnId,
    child: SessionId,
) -> Result<State, Fault> {
    let run = owned(db, parent, child)?;
    let mut query = db
        .prepare("SELECT body FROM agent_events WHERE turn=?1 ORDER BY sequence DESC")
        .map_err(storage_error)?;
    let mut response = None;
    if run.status == Status::Completed {
        for body in query
            .query_map([run.turn.to_string()], |row| row.get::<_, Vec<u8>>(0))
            .map_err(storage_error)?
        {
            let event: AdkEvent =
                serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error)?;
            if !event.llm_response.partial
                && event.is_final_response()
                && let Some(content) = event.content().filter(|content| content.role == "model")
            {
                let text: String = content
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        adk_core::Part::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                if !text.is_empty() {
                    response = Some(text);
                    break;
                }
            }
        }
    }
    let paused = queue::read(db, child)?.paused;
    Ok(State {
        run,
        response,
        paused,
    })
}
