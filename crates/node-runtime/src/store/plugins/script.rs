//! Headless callbacks compose existing scoped commands on a separate worker.
use super::super::Ingress;
use sailry_link::{CancellationToken, Handler};
use sailry_protocol::*;
use std::{sync::Arc, time::Instant};

impl Ingress {
    pub(crate) async fn script_dispatch(
        &self,
        caller: NodeId,
        request: Request,
        deadline: Instant,
        stop: &CancellationToken,
    ) -> sailry_link::Response {
        let durable = request.command.durable();
        let id = request.id;
        #[cfg(any(test, feature = "test-support"))]
        let original = request.clone();
        let completion = async {
            let admitted = self.dispatch(caller, request).await?;
            #[cfg(any(test, feature = "test-support"))]
            {
                let wait = {
                    let mut pending = self.plugins.script_wait.lock().unwrap();
                    if pending.as_ref().is_some_and(|wait| wait.durable == durable) {
                        pending.take()
                    } else {
                        None
                    }
                };
                if let Some(wait) = wait {
                    let _ = wait.entered.send(original);
                    wait.release.cancelled().await;
                }
            }
            admitted.completion.await.map_err(|_| {
                Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "plugin SDK completion is unknown",
                )
            })?
        };
        // Admission can itself wait for I/O. Bound the entire dispatch, not only
        // the completion receiver. Dropping this observer never replays a write.
        let mut result = tokio::select! {
            biased;
            result = completion => result,
            _ = stop.cancelled() => Err(interrupted(durable, id, false)),
            _ = tokio::time::sleep_until(deadline.into()) => Err(interrupted(durable, id, true)),
        };
        if let Err(fault) = &mut result
            && fault.details.is_none()
        {
            use notification::{DiagnosticPhase, DiagnosticReason, SystemDiagnostic, SystemSource};
            let mut report =
                SystemDiagnostic::new(RequestId::new(), SystemSource::PluginCallback, fault.code);
            report.phase = DiagnosticPhase::Sdk;
            report.request = Some(id);
            if fault.code == ErrorCode::Cancelled {
                report.reason = DiagnosticReason::Cancelled;
            }
            fault.details = Some(FaultDetails::SystemDiagnostic(report));
        }
        result
    }
}

fn interrupted(durable: bool, id: RequestId, timeout: bool) -> Fault {
    use notification::{DiagnosticPhase, DiagnosticReason, SystemDiagnostic, SystemSource};
    let mut fault = if durable {
        Fault::new(
            ErrorCode::OutcomeUnknown,
            "plugin SDK completion is unknown",
        )
    } else if timeout {
        Fault::new(ErrorCode::Expired, "plugin SDK response timed out")
    } else {
        Fault::new(ErrorCode::Cancelled, "plugin SDK wait stopped")
    };
    let mut diagnostic =
        SystemDiagnostic::new(RequestId::new(), SystemSource::PluginCallback, fault.code);
    diagnostic.phase = DiagnosticPhase::Sdk;
    diagnostic.reason = if timeout {
        DiagnosticReason::Timeout
    } else {
        DiagnosticReason::Cancelled
    };
    diagnostic.request = Some(id);
    fault.details = Some(FaultDetails::SystemDiagnostic(diagnostic));
    fault
}

impl Ingress {
    /// Store the safe projection once; never retry the failed callback or its SDK write.
    pub(crate) async fn script_failure(
        &self,
        caller: NodeId,
        fault: Fault,
        info: &plugin::Info,
    ) -> Fault {
        use notification::{DiagnosticPlugin, DiagnosticReason};
        let mut fault = fault;
        if let Some(FaultDetails::SystemDiagnostic(report)) = &mut fault.details {
            report.plugin = Some(DiagnosticPlugin {
                name: info.summary.name.clone(),
                version: info
                    .summary
                    .version
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
            });
            if fault.code == ErrorCode::OutcomeUnknown
                || (report.reason != DiagnosticReason::Cancelled
                    && fault.code != ErrorCode::Cancelled)
            {
                let mut request =
                    Request::new(self.node, Command::ReportDiagnostic(report.clone()));
                request.id = report.incident;
                let result = match self.dispatch(caller, request).await {
                    Ok(admission) => admission.completion.await.unwrap_or_else(|_| {
                        Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "diagnostic completion is unknown",
                        ))
                    }),
                    Err(fault) => Err(fault),
                };
                if let Err(error) = result {
                    persist_failed(self.node, caller, report, info, &error);
                }
            }
        }
        fault
    }
}

fn persist_failed(
    node: NodeId,
    caller: NodeId,
    report: &notification::SystemDiagnostic,
    info: &plugin::Info,
    error: &Fault,
) {
    use crate::logging::{Area, Context, Entry, Phase};
    let mut entry = Entry::new(Area::Plugin, Phase::Failed);
    entry.context = Context {
        node: Some(node),
        caller: Some(caller),
        request: report.request,
        incident: Some(report.incident),
        package: Some(info.summary.reference()),
        session: report.session,
        turn: report.turn,
        worktree: report.worktree,
        ..Default::default()
    };
    entry.operation = Some("diagnostic_persist");
    entry.stage = Some("store");
    entry.source_kind = Some("plugin_callback");
    entry.code = Some(error.code);
    entry.emit_warn();
}

#[cfg(test)]
#[path = "script/tests.rs"]
mod tests;

pub(super) async fn run(
    ingress: Arc<Ingress>,
    caller: NodeId,
    request: Request,
    info: plugin::Info,
    stop: CancellationToken,
) -> Result<Output, Fault> {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let read_only = matches!(request.command, Command::ReadPluginCallback { .. });
        let (Command::CallPlugin { handler, input }
        | Command::ReadPluginCallback { handler, input }) = request.command
        else {
            unreachable!()
        };
        let mut context = request.plugin.expect("admitted plugin callback");
        if !read_only {
            context.invocation = Some(request.id);
        }
        let bundle = ingress.plugins.read_host(&info, stop.clone())?;
        let target = ingress.node;
        let stopped = stop.clone();
        let deadline = ingress.plugins.script_deadline();
        let reporter = ingress.clone();
        let report_runtime = runtime.clone();
        crate::plugins::script::run_callback(
            bundle,
            &handler,
            input,
            crate::plugins::script::Environment {
                context,
                target,
                stop,
                deadline,
            },
            read_only,
            move |request| {
                runtime.block_on(ingress.script_dispatch(caller, request, deadline, &stopped))
            },
        )
        .map_err(|fault| report_runtime.block_on(reporter.script_failure(caller, fault, &info)))
        .map(Output::PluginResult)
    })
    .await
    .map_err(|_| Fault::new(ErrorCode::OutcomeUnknown, "plugin callback worker failed"))?
}
