//! Hook configuration and launch receipts share the existing Node storage owner.
use super::{
    Ingress, Job,
    agent::{Invocation, Operation},
    database::{encode, storage_error},
};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    hooks::{Phase, Settings},
    *,
};
use tokio::sync::oneshot;

pub(super) fn controller(request: &Request) -> Result<(), Fault> {
    if request.plugin.is_some() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "hooks require an authenticated native controller",
        ));
    }
    Ok(())
}

pub(super) fn state(worktree: WorktreeId, settings: Settings) -> hooks::State {
    hooks::State {
        worktree,
        settings,
        sandboxed: crate::process::sandbox::SUPPORTED,
    }
}

pub(super) fn read(db: &Connection, worktree: WorktreeId) -> Result<Settings, Fault> {
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM worktrees WHERE id=?1)",
            [worktree.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if !exists {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "worktree does not exist on this Node",
        ));
    }
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM hook_settings WHERE worktree=?1",
            [worktree.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
        .map(|settings| settings.unwrap_or_default())
}

pub(super) fn save(
    db: &Connection,
    worktree: WorktreeId,
    settings: &Settings,
    expected: u64,
) -> Result<Settings, Fault> {
    let current = read(db, worktree)?;
    if current.revision != expected || settings.revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "hook settings changed; reload before saving",
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    for entry in &settings.additional {
        if entry.id.trim().is_empty()
            || entry.id.len() > 128
            || entry.id.contains('\0')
            || !ids.insert(&entry.id)
            || Phase::ALL.iter().any(|phase| phase.name() == entry.id)
        {
            return Err(Fault::invalid("hook entry identity is invalid"));
        }
    }
    for (_, _, script) in settings.entries() {
        if script.name.len() > 1024
            || script.name.contains('\0')
            || script.command.trim().is_empty()
            || script.command.len() > 64 * 1024
            || script.command.contains('\0')
            || !(1..=60_000).contains(&script.timeout_ms)
        {
            return Err(Fault::invalid("hook command or timeout is invalid"));
        }
    }
    if settings.enabled && settings.entries().next().is_none() {
        return Err(Fault::invalid("enabled hooks require a command"));
    }
    let mut settings = settings.clone();
    settings.revision = expected
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("hook revision limit exceeded"))?;
    db.execute("INSERT INTO hook_settings(worktree,body) VALUES(?1,?2) ON CONFLICT(worktree) DO UPDATE SET body=excluded.body", params![worktree.to_string(), encode(&settings)?]).map_err(storage_error)?;
    Ok(settings)
}

pub(in crate::store) fn frozen(db: &Connection, turn: TurnId) -> Result<Settings, Fault> {
    let body: Vec<u8> = db
        .query_row(
            "SELECT hooks FROM agent_runs WHERE turn=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    serde_json::from_slice(&body).map_err(storage_error)
}

pub(in crate::store) fn claim(
    db: &Connection,
    turn: TurnId,
    phase: Phase,
    ordinal: u64,
) -> Result<(), Fault> {
    if phase == Phase::TurnEnd && ordinal != 0 {
        return Err(Fault::invalid("invalid end-hook receipt"));
    }
    let (config, status, kind): (Vec<u8>, String, String) = db.query_row("SELECT r.config,json_extract(a.body,'$.status'),t.kind FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision JOIN agent_runs a ON a.turn=t.id WHERE t.id=?1", [turn.to_string()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(storage_error)?;
    let config: SessionConfig = serde_json::from_slice(&config).map_err(storage_error)?;
    if kind != "task" || config.mode == WorkMode::Plan || status != "running" {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "hooks cannot execute in this turn",
        ));
    }
    if frozen(db, turn)?.scripts(phase).next().is_none() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "hook is not enabled in the admitted turn",
        ));
    }
    let ordinal =
        i64::try_from(ordinal).map_err(|_| Fault::invalid("hook sequence limit exceeded"))?;
    let inserted = db
        .execute(
            "INSERT OR IGNORE INTO hook_executions(turn,phase,ordinal) VALUES(?1,?2,?3)",
            params![turn.to_string(), phase.name(), ordinal],
        )
        .map_err(storage_error)?;
    if inserted != 1 {
        return Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "hook was already admitted; side effects were not replayed",
        ));
    }
    Ok(())
}

pub(in crate::store) fn finish(
    db: &Connection,
    turn: TurnId,
    phase: Phase,
    ordinal: u64,
    result: &Result<process::Outcome, Fault>,
) -> Result<(), Fault> {
    let count = db.execute("UPDATE hook_executions SET result=?4 WHERE turn=?1 AND phase=?2 AND ordinal=?3 AND result IS NULL", params![turn.to_string(),phase.name(),i64::try_from(ordinal).map_err(storage_error)?,encode(result)?]).map_err(storage_error)?;
    if count != 1 {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "hook receipt has already completed or does not exist",
        ));
    }
    Ok(())
}

impl Ingress {
    pub(crate) async fn claim_hook(
        &self,
        turn: TurnId,
        phase: Phase,
        ordinal: u64,
    ) -> Result<(), Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::ClaimHook {
                turn,
                phase,
                ordinal,
                reply,
            })))
            .await
            .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }

    pub(crate) async fn finish_hook(
        &self,
        turn: TurnId,
        phase: Phase,
        ordinal: u64,
        result: Result<process::Outcome, Fault>,
    ) -> Result<(), Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::FinishHook {
                turn,
                phase,
                ordinal,
                result,
                reply,
            })))
            .await
            .map_err(|_| super::unavailable())?;
        response.await.map_err(|_| super::unavailable())?
    }

    pub(crate) async fn launch_hook(
        &self,
        invocation: &crate::hooks::Binding,
        command: String,
        timeout_ms: u64,
        metadata: std::collections::BTreeMap<&'static str, String>,
        stop: &sailry_link::CancellationToken,
    ) -> Result<process::Completion, Fault> {
        let root = self.worktree_root(invocation.worktree).await?;
        let scratch = self
            .profile
            .as_ref()
            .map(|profile| {
                self.agents.scratch(
                    invocation.turn,
                    profile,
                    invocation.worktree,
                    invocation.session,
                )
            })
            .transpose()?;
        let result = crate::process::execute_hook(
            crate::process::Launch {
                pty: false,
                root,
                profile: self.profile.clone(),
                command,
                access: process::Access::Workspace,
                cwd: String::new(),
                timeout_ms,
                inputs: None,
                scratch,
            },
            &self.environment,
            stop,
            &self.closed,
            &metadata,
        )
        .await;
        if result.is_ok() {
            self.code.watched(invocation.worktree).await;
        }
        result
    }
}

pub(crate) fn admitted(invocation: &Invocation) -> bool {
    invocation.turn.kind == conversation::RunKind::Task
        && invocation.turn.config.mode != WorkMode::Plan
        && !invocation
            .child
            .as_ref()
            .and_then(|child| child.role.as_ref())
            .is_some_and(|role| role.read_only)
}
