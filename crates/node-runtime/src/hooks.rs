//! User scripts reuse managed Node processes and durable execution receipts.

use crate::store::{Ingress, agent::Invocation};
use futures::FutureExt;
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, SessionId, TurnId, WorktreeId,
    hooks::{Phase, Settings},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub(crate) struct Binding {
    ingress: Arc<Ingress>,
    pub(crate) worktree: WorktreeId,
    pub(crate) session: SessionId,
    pub(crate) turn: TurnId,
    settings: Settings,
    stop: CancellationToken,
    before: AtomicU64,
    after: AtomicU64,
    failure: Mutex<Option<Fault>>,
}

impl Binding {
    pub(crate) fn capture(
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        stop: CancellationToken,
    ) -> Option<Arc<Self>> {
        (crate::store::hooks::admitted(invocation) && invocation.hooks.enabled).then(|| {
            Arc::new(Self {
                ingress: ingress.clone(),
                worktree: invocation.worktree,
                session: invocation.turn.session,
                turn: invocation.turn.id,
                settings: invocation.hooks.clone(),
                stop,
                before: AtomicU64::new(0),
                after: AtomicU64::new(0),
                failure: Mutex::new(None),
            })
        })
    }

    pub(crate) async fn before(self: &Arc<Self>, tool: &str) -> Result<(), Fault> {
        let ordinal = self.before.fetch_add(1, Ordering::Relaxed);
        self.execute(Phase::BeforeTool, ordinal, Some(tool), None)
            .await
    }

    pub(crate) async fn after(self: &Arc<Self>, tool: &str, success: bool) {
        let ordinal = self.after.fetch_add(1, Ordering::Relaxed);
        let status = if success { "succeeded" } else { "failed" };
        if let Err(error) = self
            .execute(Phase::AfterTool, ordinal, Some(tool), Some(status))
            .await
        {
            // The actual tool response owns side effects that already happened.
            self.remember(error);
        }
    }

    pub(crate) async fn complete(self: &Arc<Self>, status: &str) -> Result<(), Fault> {
        let status = if self
            .failure
            .lock()
            .map_err(|_| Fault::new(ErrorCode::Internal, "hook result lock is unavailable"))?
            .is_some()
        {
            "failed"
        } else {
            status
        };
        if let Err(error) = self.execute(Phase::TurnEnd, 0, None, Some(status)).await {
            self.remember(error);
        }
        self.failure
            .lock()
            .map_err(|_| Fault::new(ErrorCode::Internal, "hook result lock is unavailable"))?
            .clone()
            .map_or(Ok(()), Err)
    }

    fn remember(&self, error: Fault) {
        if let Ok(mut failure) = self.failure.lock() {
            match failure.as_mut() {
                None => *failure = Some(error),
                Some(current) => {
                    if error.code == ErrorCode::OutcomeUnknown {
                        current.code = ErrorCode::OutcomeUnknown;
                    }
                    if current.message.len() < 4096 && !current.message.contains(&error.message) {
                        current.message.push_str("; ");
                        let remaining = 4096usize.saturating_sub(current.message.len());
                        current
                            .message
                            .extend(error.message.chars().take(remaining));
                    }
                }
            }
        }
    }

    async fn execute(
        self: &Arc<Self>,
        phase: Phase,
        ordinal: u64,
        tool: Option<&str>,
        status: Option<&str>,
    ) -> Result<(), Fault> {
        let scripts = self
            .settings
            .scripts(phase)
            .map(|(id, script)| (id.to_owned(), script.clone()))
            .collect::<Vec<_>>();
        if scripts.is_empty() {
            return Ok(());
        }
        if self.stop.is_cancelled() {
            return Ok(());
        }
        // The durable marker precedes native launch. A crash leaves an uncertain
        // receipt; neither Node recovery nor a repeated callback replays it.
        self.ingress.claim_hook(self.turn, phase, ordinal).await?;
        let stop = self
            .ingress
            .agents
            .begin_operation(self.turn)
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::Conflict,
                    "turn is no longer accepting hook execution",
                )
            })?;
        let lease = Lease {
            controls: self.ingress.agents.clone(),
            turn: self.turn,
        };
        let binding = self.clone();
        let tool = tool.map(str::to_owned);
        let status = status.map(str::to_owned);
        let job = tokio::spawn(async move {
            let _lease = lease;
            let result = std::panic::AssertUnwindSafe(binding.run(
                phase,
                ordinal,
                scripts,
                tool.as_deref(),
                status.as_deref(),
                stop,
            ))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "hook execution failed; side effects were not replayed",
                ))
            });
            if phase != Phase::BeforeTool
                && let Err(error) = &result
            {
                binding.remember(error.clone());
            }
            result
        });
        job.await.map_err(|_| {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                "hook completion is uncertain; side effects were not replayed",
            )
        })?
    }

    async fn run(
        &self,
        phase: Phase,
        ordinal: u64,
        scripts: Vec<(String, sailry_protocol::hooks::Script)>,
        tool: Option<&str>,
        status: Option<&str>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let mut metadata = BTreeMap::from([
            ("SAILRY_HOOK_EVENT", phase.name().to_string()),
            (
                "SAILRY_HOOK_NODE",
                self.ingress
                    .node
                    .0
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
            ),
            ("SAILRY_HOOK_WORKTREE", self.worktree.to_string()),
            ("SAILRY_HOOK_SESSION", self.session.to_string()),
            ("SAILRY_HOOK_TURN", self.turn.to_string()),
        ]);
        if let Some(tool) = tool {
            metadata.insert("SAILRY_HOOK_TOOL", tool.to_string());
        }
        if let Some(status) = status {
            metadata.insert("SAILRY_HOOK_STATUS", status.to_string());
        }
        // Raw tool arguments, responses, provider configuration and credentials
        // are deliberately absent from the script environment and launch receipt.
        let mut result = Ok(sailry_protocol::process::Outcome::Exited(0));
        for (_, script) in scripts {
            if stop.is_cancelled() {
                if matches!(&result, Ok(sailry_protocol::process::Outcome::Exited(0))) {
                    result = Ok(sailry_protocol::process::Outcome::Cancelled);
                }
                break;
            }
            let outcome = self
                .ingress
                .launch_hook(
                    self,
                    script.command,
                    script.timeout_ms,
                    metadata.clone(),
                    &stop,
                )
                .await
                .map(|completion| completion.outcome);
            let success = matches!(&outcome, Ok(sailry_protocol::process::Outcome::Exited(0)));
            if !success {
                let uncertain = matches!(&outcome, Err(error) if error.code == ErrorCode::OutcomeUnknown)
                    || matches!(&outcome, Ok(sailry_protocol::process::Outcome::Unknown(_)));
                if matches!(&result, Ok(sailry_protocol::process::Outcome::Exited(0))) || uncertain
                {
                    result = outcome;
                }
                if phase == Phase::BeforeTool || uncertain {
                    break;
                }
            }
        }
        self.ingress
            .finish_hook(self.turn, phase, ordinal, result.clone())
            .await
            .map_err(|error| {
                Fault::new(
                    ErrorCode::OutcomeUnknown,
                    format!(
                        "hook completion could not be recorded: {error}; script was not replayed"
                    ),
                )
            })?;
        match result? {
            sailry_protocol::process::Outcome::Exited(0) => Ok(()),
            sailry_protocol::process::Outcome::Unknown(reason) => Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                format!(
                    "{} hook outcome is uncertain: {reason}; script was not replayed",
                    phase.name()
                ),
            )),
            outcome => Err(Fault::new(
                if phase == Phase::BeforeTool {
                    ErrorCode::PermissionDenied
                } else {
                    ErrorCode::Unavailable
                },
                format!("{} hook failed: {outcome:?}", phase.name()),
            )),
        }
    }
}

struct Lease {
    controls: Arc<crate::agent::Controls>,
    turn: TurnId,
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.controls.finish_operation(self.turn);
    }
}
