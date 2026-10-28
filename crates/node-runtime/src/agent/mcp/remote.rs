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
        CancelledNotification, CancelledNotificationParam, ClientRequest, ClientResult,
        ErrorCode as RpcCode, GetExtensions, GetMeta, GetTaskParams, InputRequest, InputRequests,
        InputResponses, NumberOrString, ServerRequest, ServerResult, Task, TaskPayload,
        UpdateTaskParams,
    },
    service::{PeerRequestOptions, RequestContext, Service as _},
};
use std::collections::BTreeMap;
use tokio::sync::Mutex;

pub(super) struct Remote {
    service: Service,
    tasks: Mutex<BTreeSet<String>>,
}

impl Remote {
    pub(super) fn new(service: Service) -> Arc<Self> {
        Arc::new(Self {
            service,
            tasks: Mutex::new(BTreeSet::new()),
        })
    }

    pub(super) fn is_closed(&self) -> bool {
        self.service.is_closed()
    }

    pub(super) async fn call(
        &self,
        name: &str,
        arguments: serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, Fault> {
        let mut params = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
        let mut state_rounds = 0u32;
        for _ in 0..10 {
            match self.send(params.clone()).await? {
                CallToolResponse::Complete(result) => return Ok(result),
                CallToolResponse::Task(created) => return Box::pin(self.task(created.task)).await,
                CallToolResponse::InputRequired(input) => {
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

    async fn send(&self, params: CallToolRequestParams) -> Result<CallToolResponse, Fault> {
        let handle = self
            .service
            .peer()
            .send_cancellable_request(
                ClientRequest::CallToolRequest(CallToolRequest::new(params)),
                PeerRequestOptions::no_options(),
            )
            .await
            .map_err(failed)?;
        let cancel = Cancel {
            peer: Some(handle.peer.clone()),
            id: handle.id.clone(),
        };
        let result = handle.await_response().await;
        cancel.disarm();
        match result.map_err(failed)? {
            ServerResult::CallToolResult(result) => Ok(CallToolResponse::Complete(result)),
            ServerResult::InputRequiredResult(result) => {
                Ok(CallToolResponse::InputRequired(result))
            }
            ServerResult::CreateTaskResult(result) => Ok(CallToolResponse::Task(result)),
            _ => Err(invalid("MCP tool returned an unexpected response")),
        }
    }

    async fn task(&self, initial: Task) -> Result<CallToolResult, Fault> {
        let id = initial.task_id;
        if id.is_empty() || id.len() > 16 * 1024 {
            return Err(invalid("MCP task identity is invalid"));
        }
        self.tasks.lock().await.insert(id.clone());
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
                .map_err(failed)?
                .task;
            if detailed.task.task_id != id {
                return Err(invalid("MCP task identity changed"));
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
                    TaskPayload::Failed { error } => Err(Fault::new(
                        ErrorCode::Unavailable,
                        detailed
                            .task
                            .status_message
                            .unwrap_or_else(|| Value::Object(error).to_string()),
                    )),
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
                        .map_err(failed)?;
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
                Err(invalid("MCP task identity changed"))
            }
            Ok(result) => Ok(result.task.status().is_terminal()),
            Err(error) if finished(&error) => Ok(true),
            Err(error) => Err(failed(error)),
        }
    }
}

fn invalid(message: impl Into<String>) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
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
    id: rmcp::model::RequestId,
}
impl Cancel {
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
        let notification = CancelledNotification::new(CancelledNotificationParam::new(
            Some(self.id.clone()),
            Some("the client stopped waiting for this tools/call".into()),
        ));
        runtime.spawn(async move {
            let _ = tokio::time::timeout(
                Duration::from_secs(2),
                peer.send_notification(notification.into()),
            )
            .await;
        });
    }
}
