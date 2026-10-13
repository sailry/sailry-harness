//! Close only the captured project's terminals on the existing mutation worker.
use super::*;
use crate::store::Job;
use tokio::sync::{mpsc, oneshot};

pub(in crate::store) struct Plan {
    pub worktrees: Vec<Worktree>,
    pub terminals: Vec<terminal::Info>,
}

pub(in crate::store) fn prepare(db: &Connection, expected: &Project) -> Result<Plan, Fault> {
    check(db, expected)?;
    let active: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN sessions s ON s.id=t.session WHERE s.project=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))",
        [expected.id.to_string()], |row| row.get(0)).map_err(storage_error)?;
    if active {
        let mut error = Fault::new(ErrorCode::Busy, "project has pending tasks");
        error.details = Some(FaultDetails::ProjectRemoval {
            tasks_active: true,
            terminals_may_be_closed: false,
        });
        return Err(error);
    }
    let worktrees: Vec<_> = super::super::worktrees::list(db)?
        .into_iter()
        .filter(|tree| tree.project == Some(expected.id))
        .collect();
    let terminals = super::super::terminals::list(db)?
        .into_iter()
        .filter(|info| {
            info.status != terminal::Status::Closed
                && worktrees.iter().any(|tree| info.worktree == Some(tree.id))
        })
        .collect();
    Ok(Plan {
        worktrees,
        terminals,
    })
}

pub(in crate::store) fn execute(
    validation: &mpsc::Sender<Job>,
    terminals: &crate::terminal::Terminals,
    caller: NodeId,
    request: &Request,
    expected: &Project,
) -> sailry_link::Response {
    let (reply, response) = oneshot::channel();
    validation
        .blocking_send(Job::ProjectRemovalPlan {
            caller,
            request: request.id,
            expected: expected.clone(),
            reply,
        })
        .map_err(|_| unavailable())?;
    let plan = response.blocking_recv().map_err(|_| unavailable())??;
    for info in plan {
        let worktree = info
            .worktree
            .ok_or_else(|| Fault::invalid("project terminal has no worktree"))?;
        // Closing joins the PTY owner, whose final metadata callback uses the Store queue.
        let result = terminals.close(worktree, info.id);
        let (reply, response) = oneshot::channel();
        validation
            .blocking_send(Job::ProjectTerminalClosed {
                worktree,
                terminal: info.id,
                result,
                reply,
            })
            .map_err(|_| unconfirmed())?;
        if let Err(mut error) = response.blocking_recv().map_err(|_| unconfirmed())? {
            error.details = Some(FaultDetails::ProjectRemoval {
                tasks_active: false,
                terminals_may_be_closed: true,
            });
            return Err(error);
        }
    }
    Ok(Output::ProjectRemoved { id: expected.id })
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "Node store is unavailable")
}

fn unconfirmed() -> Fault {
    let mut error = Fault::new(
        ErrorCode::OutcomeUnknown,
        "project terminal closure could not be confirmed",
    );
    error.details = Some(FaultDetails::ProjectRemoval {
        tasks_active: false,
        terminals_may_be_closed: true,
    });
    error
}
