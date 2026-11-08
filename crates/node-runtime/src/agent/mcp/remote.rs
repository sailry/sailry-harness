//! MCP calls use the SDK directly while Node owns input and remote task cleanup.
//!
//! Task cancellation and MRTR handling are adapted from adk-rust 675c02b6,
//! adk-tool/src/mcp/{toolset,input}.rs (Apache-2.0). See
//! third_party_licenses/adk-LICENSE.
use super::*;
use futures::{StreamExt, stream};
use rmcp::{
    Peer,
    model::{
        CallToolRequest, CallToolRequestParams, CallToolResponse, CallToolResult, CancelTaskParams,
        ClientRequest, ClientResult, CustomNotification, ErrorCode as RpcCode, GetExtensions,
        GetMeta, GetTaskParams, InputRequest, InputRequests, InputResponses, NumberOrString,
        ServerRequest, ServerResult, Task, TaskPayload, UpdateTaskParams,
    },
    service::{PeerRequestOptions, RequestContext, Service as _},
};
use std::collections::BTreeMap;
use tokio::sync::Mutex;

pub(super) struct Remote {
    service: Service,
    tasks: Mutex<BTreeSet<String>>,
    broken: AtomicBool,
}

impl Remote {
    pub(super) fn new(service: Service) -> Arc<Self> {
        Arc::new(Self {
            service,
            tasks: Mutex::new(BTreeSet::new()),
            broken: AtomicBool::new(false),
        })
    }

    pub(super) fn is_closed(&self) -> bool {
        self.broken.load(Ordering::Relaxed)
            || self.service.is_closed()
            || self.service.peer().is_transport_closed()
    }

    fn failed(&self, error: rmcp::ServiceError) -> Fault {
        let failure = match &error {
            rmcp::ServiceError::McpError(error) => rpc_failure(error),
            rmcp::ServiceError::Cancelled { .. } => crate::agent::observation::Failure {
                failure: sailry_protocol::tool::Failure::runtime(Some(ErrorCode::Cancelled)),
                message: None,
            },
            _ => crate::agent::observation::Failure {
                failure: sailry_protocol::tool::Failure::runtime(None),
                message: None,
            },
        };
        self.service.service().failed(failure);
        if matches!(
            error,
            rmcp::ServiceError::UnexpectedResponse
                | rmcp::ServiceError::TransportSend(_)
                | rmcp::ServiceError::TransportClosed
        ) {
            self.broken.store(true, Ordering::Relaxed);
        }
        failed(error)
    }

    fn unexpected(&self, message: &str) -> Fault {
        self.broken.store(true, Ordering::Relaxed);
        invalid(message)
    }

    pub(super) async fn call(
        &self,
        name: &str,
        arguments: serde_json::Map<String, Value>,
        stop: &CancellationToken,
    ) -> Result<CallToolResult, Fault> {
        let mut params = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
        let mut state_rounds = 0u32;
        for _ in 0..10 {
            if stop.is_cancelled() {
                return Err(cancelled());
            }
            match self.send(params.clone(), stop).await? {
                CallToolResponse::Complete(result) => return Ok(result),
                CallToolResponse::Task(created) => {
                    return Box::pin(self.task(created.task, stop)).await;
                }
                CallToolResponse::InputRequired(input) => {
                    if stop.is_cancelled() {
                        return Err(cancelled());
                    }
                    let requests = input.input_requests.filter(|requests| !requests.is_empty());
                    if requests.is_none() && input.request_state.is_none() {
                        return Err(invalid("MCP input has neither requests nor state"));
                    }
                    let responses = if let Some(requests) = requests {
                        state_rounds = 0;
                        Box::pin(self.fulfill(requests)).await?
                    } else {
                        tokio::time::sleep(Duration::from_millis(
                            (50 << state_rounds.min(3)).min(250),
                        ))
                        .await;
                        state_rounds += 1;
                        InputResponses::new()
                    };
                    params.input_responses = (!responses.is_empty()).then_some(responses);
                    params.request_state = input.request_state;
                }
                _ => return Err(invalid("MCP tool returned an unsupported response")),
            }
        }
        Err(invalid("MCP tool exceeded the SDK input round limit"))
    }

    async fn send(
        &self,
        params: CallToolRequestParams,
        stop: &CancellationToken,
    ) -> Result<CallToolResponse, Fault> {
        let handle = self
            .service
            .peer()
            .send_cancellable_request(
                ClientRequest::CallToolRequest(CallToolRequest::new(params)),
                PeerRequestOptions::no_options(),
            )
            .await
            .map_err(|error| self.failed(error))?;
        let cancel = Cancel {
            peer: Some(handle.peer.clone()),
            target: Target::Request(handle.id.clone()),
        };
        let response = handle.await_response();
        tokio::pin!(response);
        let result = tokio::select! {
            biased;
            result = &mut response => result,
            _ = stop.cancelled() => {
                cancel.notify().await;
                response.await
            }
        };
        cancel.disarm();
        match result.map_err(|error| self.failed(error))? {
            ServerResult::CallToolResult(result) => Ok(CallToolResponse::Complete(result)),
            ServerResult::InputRequiredResult(result) => {
                Ok(CallToolResponse::InputRequired(result))
            }
            ServerResult::CreateTaskResult(result) => Ok(CallToolResponse::Task(result)),
            _ => Err(self.unexpected("MCP tool returned an unexpected response")),
        }
    }

    async fn task(&self, initial: Task, stop: &CancellationToken) -> Result<CallToolResult, Fault> {
        let id = initial.task_id.clone();
        if id.is_empty() || id.len() > 16 * 1024 {
            return Err(invalid("MCP task identity is invalid"));
        }
        self.tasks.lock().await.insert(id.clone());
        let cancel = Cancel {
            peer: Some(self.service.peer().clone()),
            target: Target::Task(id.clone()),
        };
        let interval = Duration::from_millis(initial.poll_interval_ms.unwrap_or(1000).max(1));
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => Err(cancelled()),
            result = self.poll_task(initial) => result,
        };
        if self.tasks.lock().await.contains(&id) {
            stop.cancel();
            cancel.notify().await;
            loop {
                if self.is_closed() {
                    return Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "MCP task termination is unconfirmed",
                    ));
                }
                match tokio::time::timeout(Duration::from_secs(120), self.settled(&id)).await {
                    Ok(Ok(true)) => {
                        self.tasks.lock().await.remove(&id);
                        break;
                    }
                    _ => tokio::time::sleep(interval).await,
                }
            }
        }
        cancel.disarm();
        result
    }

    async fn poll_task(&self, initial: Task) -> Result<CallToolResult, Fault> {
        let id = initial.task_id;
        if !self
            .service
            .peer_info()
            .is_some_and(|info| info.capabilities.supports_tasks())
        {
            return Err(invalid("MCP server did not negotiate task support"));
        }
        let mut interval = initial.poll_interval_ms.unwrap_or(1000).max(1);
        let mut answered: BTreeMap<String, (InputRequest, Value)> = BTreeMap::new();
        let mut rounds = 0usize;
        loop {
            tokio::time::sleep(Duration::from_millis(interval)).await;
            let detailed = self
                .service
                .peer()
                .get_task(GetTaskParams::new(&id))
                .await
                .map_err(|error| self.failed(error))?
                .task;
            if detailed.task.task_id != id {
                return Err(self.unexpected("MCP task identity changed"));
            }
            interval = detailed.task.poll_interval_ms.unwrap_or(interval).max(1);
            if detailed.task.status.is_terminal() {
                self.tasks.lock().await.remove(&id);
                return match detailed.payload {
                    TaskPayload::Completed { result } => {
                        serde_json::from_value(Value::Object(result)).map_err(failed)
                    }
                    TaskPayload::Cancelled => {
                        Err(Fault::new(ErrorCode::Cancelled, "MCP task was cancelled"))
                    }
                    TaskPayload::Failed { error } => {
                        let failure =
                            serde_json::from_value::<ErrorData>(Value::Object(error.clone()))
                                .map(|error| rpc_failure(&error))
                                .unwrap_or_else(|_| crate::agent::observation::Failure {
                                    failure: sailry_protocol::tool::Failure::tool(None),
                                    message: Some(
                                        detailed
                                            .task
                                            .status_message
                                            .clone()
                                            .unwrap_or_else(|| "MCP task failed".into()),
                                    ),
                                });
                        self.service.service().failed(failure);
                        Err(Fault::new(
                            ErrorCode::Unavailable,
                            detailed
                                .task
                                .status_message
                                .unwrap_or_else(|| Value::Object(error).to_string()),
                        ))
                    }
                    _ => Err(invalid("MCP terminal task has an unexpected payload")),
                };
            }
            match detailed.payload {
                TaskPayload::Working => answered.clear(),
                TaskPayload::InputRequired { input_requests } => {
                    if input_requests.is_empty() {
                        return Err(invalid("MCP task input is empty"));
                    }
                    let mut pending = InputRequests::new();
                    let mut responses = InputResponses::new();
                    for (key, request) in &input_requests {
                        if let Some((previous, response)) = answered.get(key) {
                            if previous != request {
                                return Err(invalid("MCP task input changed after its answer"));
                            }
                            responses.insert(key.clone(), response.clone());
                        } else {
                            pending.insert(key.clone(), request.clone());
                        }
                    }
                    if !pending.is_empty() {
                        rounds += 1;
                        if rounds > 10 {
                            return Err(invalid("MCP task exceeded the SDK input round limit"));
                        }
                        for (key, response) in Box::pin(self.fulfill(pending)).await? {
                            answered.insert(
                                key.clone(),
                                (input_requests[&key].clone(), response.clone()),
                            );
                            responses.insert(key, response);
                        }
                    }
                    self.service
                        .peer()
                        .update_task(UpdateTaskParams::new(&id, responses))
                        .await
                        .map_err(|error| self.failed(error))?;
                }
                _ => return Err(invalid("MCP task has an unsupported status")),
            }
        }
    }

    async fn fulfill(&self, requests: InputRequests) -> Result<InputResponses, Fault> {
        let capabilities = self.service.service().get_info().capabilities;
        for input in requests.values() {
            match input {
                InputRequest::Elicitation(_) => {}
                InputRequest::CreateMessage(_) if capabilities.sampling.is_some() => {}
                InputRequest::ListRoots(_) if capabilities.roots.is_some() => {}
                _ => {
                    return Err(invalid(
                        "MCP server requested an undeclared input capability",
                    ));
                }
            }
        }
        let mut responses = InputResponses::new();
        for (key, input) in requests {
            let mut request = match &input {
                InputRequest::Elicitation(request) => ServerRequest::ElicitRequest(request.clone()),
                InputRequest::CreateMessage(request) => {
                    ServerRequest::CreateMessageRequest(request.clone())
                }
                InputRequest::ListRoots(request) => {
                    ServerRequest::ListRootsRequest(request.clone())
                }
                _ => return Err(invalid("MCP input type is unsupported")),
            };
            let mut context = RequestContext::new(
                NumberOrString::String(key.clone().into()),
                self.service.peer().clone(),
            );
            std::mem::swap(&mut context.meta, request.get_meta_mut());
            std::mem::swap(&mut context.extensions, request.extensions_mut());
            let result = Box::pin(self.service.service().handle_request(request, context))
                .await
                .map_err(failed)?;
            let value = match (input, result) {
                (InputRequest::Elicitation(_), ClientResult::ElicitResult(value)) => {
                    serde_json::to_value(value)
                }
                (InputRequest::CreateMessage(_), ClientResult::CreateMessageResult(value)) => {
                    serde_json::to_value(value)
                }
                (InputRequest::ListRoots(_), ClientResult::ListRootsResult(value)) => {
                    serde_json::to_value(value)
                }
                _ => return Err(invalid("MCP input response type is invalid")),
            }
            .map_err(failed)?;
            responses.insert(key, value);
        }
        Ok(responses)
    }

    pub(super) async fn cancel_pending(&self) -> Result<(), Fault> {
        let ids: Vec<_> = self.tasks.lock().await.iter().cloned().collect();
        let results: Vec<_> = stream::iter(ids)
            .map(|id| async move {
                if !self.settled(&id).await? {
                    if let Err(error) = self
                        .service
                        .peer()
                        .cancel_task(CancelTaskParams::new(&id))
                        .await
                        && !finished(&error)
                    {
                        return Err(failed(error));
                    }
                    if !self.settled(&id).await? {
                        return Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "MCP task termination is unconfirmed",
                        ));
                    }
                }
                self.tasks.lock().await.remove(&id);
                Ok::<_, Fault>(())
            })
            .buffer_unordered(16)
            .collect()
            .await;
        for result in results {
            result?;
        }
        Ok(())
    }

    async fn settled(&self, id: &str) -> Result<bool, Fault> {
        match self.service.peer().get_task(GetTaskParams::new(id)).await {
            Ok(result) if result.task.task.task_id != id => {
                Err(self.unexpected("MCP task identity changed"))
            }
            Ok(result) => Ok(result.task.status().is_terminal()),
            Err(error) if finished(&error) => Ok(true),
            Err(error) => Err(self.failed(error)),
        }
    }
}

fn invalid(message: impl Into<String>) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}
fn rpc_failure(error: &ErrorData) -> crate::agent::observation::Failure {
    let fault = error
        .data
        .as_ref()
        .and_then(|data| serde_json::from_value::<Fault>(data.clone()).ok());
    match fault {
        Some(fault) => crate::agent::observation::Failure {
            failure: sailry_protocol::tool::Failure::runtime(Some(fault.code)),
            message: Some(fault.message),
        },
        None => crate::agent::observation::Failure {
            failure: sailry_protocol::tool::Failure::tool(None),
            message: Some(error.message.to_string()),
        },
    }
}
fn failed(error: impl std::fmt::Display) -> Fault {
    invalid(format!("MCP request failed: {error}"))
}
fn finished(error: &rmcp::ServiceError) -> bool {
    matches!(error, rmcp::ServiceError::McpError(error)
        if error.code == RpcCode::INVALID_PARAMS || error.code == RpcCode::RESOURCE_NOT_FOUND)
}

struct Cancel {
    peer: Option<Peer<RoleClient>>,
    target: Target,
}

#[derive(Clone)]
enum Target {
    Request(rmcp::model::RequestId),
    Task(String),
}

impl Cancel {
    async fn notify(&self) {
        if let Some(peer) = &self.peer {
            notify(peer.clone(), self.target.clone()).await;
        }
    }

    fn disarm(mut self) {
        self.peer = None;
    }
}
impl Drop for Cancel {
    fn drop(&mut self) {
        let Some(peer) = self.peer.take() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let target = self.target.clone();
        runtime.spawn(notify(peer, target));
    }
}

async fn notify(peer: Peer<RoleClient>, target: Target) {
    let _ = tokio::time::timeout(Duration::from_secs(2), async move {
        match target {
            Target::Request(id) => {
                let notification = CustomNotification::new(
                    "notifications/cancelled",
                    Some(json!({
                        "requestId": id,
                        "reason": "the client stopped waiting for this tools/call",
                    })),
                );
                let _ = peer.send_notification(notification.into()).await;
            }
            Target::Task(id) => {
                let _ = peer.cancel_task(CancelTaskParams::new(id)).await;
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests;
