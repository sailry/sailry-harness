//! Plugin operations share ordinary durable receipts and replay recovery.
use super::super::{Ingress, Job, database::Database, external::Completed};
use futures::FutureExt;
use sailry_link::{CancellationToken, Response};
use sailry_protocol::*;
use std::{panic::AssertUnwindSafe, sync::Arc};
use tokio::{
    runtime::Handle,
    sync::{Semaphore, mpsc, oneshot},
};

const MAX_SCRIPTS: usize = 32;

pub(in crate::store) enum Input {
    Text {
        provider: conversation::Provider,
        config: SessionConfig,
        prompt: String,
    },
    Http(crate::plugins::http::Input),
    Script(Box<plugin::Info>),
    Command(Request),
    Cancel(RequestId),
}

pub(in crate::store) fn prepare(
    db: &Database,
    caller: NodeId,
    request: &Request,
) -> Result<Input, Fault> {
    if request.plugin.is_none() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "plugin provenance is required",
        ));
    }
    if let Command::CancelPluginCall { request } = &request.command {
        return Ok(Input::Cancel(*request));
    }
    if let Command::CallPlugin { handler, input } = &request.command {
        let info = db.plugin_package(caller, request)?;
        if !info
            .extension
            .as_ref()
            .and_then(|extension| extension.host.as_ref())
            .is_some_and(|host| host.valid() && host.handlers.contains(handler))
            || serde_json::to_vec(input)
                .map_err(super::storage_error)?
                .len()
                > plugin::host::MAX_DATA_BYTES
        {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "invalid plugin callback or input",
            ));
        }
        return Ok(Input::Script(Box::new(info)));
    }
    if let Command::RequestPluginHttp(http) = &request.command {
        return super::http::prepare(db, request, http).map(Input::Http);
    }
    let Command::GeneratePluginText {
        prompt,
        model,
        effort,
    } = &request.command
    else {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "unsupported plugin execution command",
        ));
    };
    if prompt.trim().is_empty() || prompt.len() > 64 * 1024 {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "plugin prompt must contain 1–65536 bytes",
        ));
    }
    let (provider, mut config) = super::models::resolve(&db.connection, model)?;
    if let Some(effort) = effort {
        config.effort = *effort;
    }
    super::super::commands::validate_config(&db.connection, db.node, &config)?;
    Ok(Input::Text {
        provider,
        config,
        prompt: prompt.clone(),
    })
}

pub(in crate::store) struct Execution {
    pub caller: NodeId,
    pub request: Request,
    pub input: Input,
    pub reply: oneshot::Sender<Response>,
}

pub(in crate::store) struct Worker {
    pub pending: usize,
    scripts: usize,
    script_slots: Arc<Semaphore>,
    active: std::collections::BTreeMap<(NodeId, RequestId), CancellationToken>,
    ingress: Arc<Ingress>,
    runtime: Handle,
    completed: mpsc::Sender<Job>,
    closed: CancellationToken,
}

impl Worker {
    pub fn new(ingress: Arc<Ingress>) -> Self {
        Self {
            pending: 0,
            scripts: 0,
            script_slots: ingress.plugins.scripts.clone(),
            active: Default::default(),
            runtime: Handle::current(),
            completed: ingress.sender.clone(),
            closed: ingress.closed.clone(),
            ingress,
        }
    }

    pub fn submit(&mut self, execution: Execution) -> Result<(), Box<Execution>> {
        let script = matches!(execution.input, Input::Script(_));
        let cancel = matches!(execution.input, Input::Cancel(_));
        if (!cancel
            && if script {
                self.scripts >= MAX_SCRIPTS
            } else {
                self.pending - self.scripts >= 4
            })
            || self.closed.is_cancelled()
        {
            return Err(Box::new(execution));
        }
        let stop = if let Some(turn) = execution
            .request
            .plugin
            .as_ref()
            .and_then(|context| context.turn)
        {
            let Some(stop) = self.ingress.agents.begin_operation(turn) else {
                return Err(Box::new(execution));
            };
            stop.child_token()
        } else {
            self.closed.child_token()
        };
        if script {
            self.active
                .insert((execution.caller, execution.request.id), stop.clone());
        }
        let cancelled = if let Input::Cancel(id) = &execution.input {
            self.active.get(&(execution.caller, *id)).map(|stop| {
                stop.cancel();
                *id
            })
        } else {
            None
        };
        self.pending += 1;
        self.scripts += usize::from(script);
        let ingress = self.ingress.clone();
        let completed = self.completed.clone();
        let script_slots = self.script_slots.clone();
        self.runtime.spawn(async move {
            let input = execution.input;
            let operation = async {
                match input {
                    Input::Text { provider, config, prompt } => tokio::select! {
                        biased;
                        _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "plugin model request cancelled")),
                        result = crate::agent::completion::generate(
                            &ingress, &provider, &config, prompt, &stop) => result.map(Output::PluginText),
                    },
                    Input::Http(input) => crate::plugins::http::run(input, stop.clone()).await.map(Output::PluginHttp),
                    Input::Script(info) => {
                        // Admission is already durable. Wait for a VM slot without allocating
                        // a VM or failing a valid dispatch group merely because two VMs are busy.
                        let permit = tokio::select! {
                            biased;
                            _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "plugin callback cancelled")),
                            permit = script_slots.acquire_owned() => permit.expect("script pool remains open"),
                        };
                        let result = super::script::run(ingress.clone(), execution.caller, execution.request.clone(), *info, stop.clone()).await;
                        drop(permit);
                        result
                    },
                    Input::Command(child) => super::command::run(&ingress, execution.caller, child).await,
                    Input::Cancel(_) => cancelled.map(|request| Output::PluginCallStopping { request })
                        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "plugin callback is not running")),
                }
            };
            let result = AssertUnwindSafe(operation).catch_unwind().await
                .unwrap_or_else(|_| Err(Fault::new(ErrorCode::OutcomeUnknown, "plugin operation result is unknown")));
            let _ = completed.send(Job::PluginExecution(Box::new(Completed {
                caller: execution.caller, request: execution.request, result, reply: execution.reply,
            }))).await;
        });
        Ok(())
    }

    pub fn finished(&mut self, caller: NodeId, request: &Request) {
        self.pending -= 1;
        self.scripts -= usize::from(matches!(request.command, Command::CallPlugin { .. }));
        self.active.remove(&(caller, request.id));
    }
}
