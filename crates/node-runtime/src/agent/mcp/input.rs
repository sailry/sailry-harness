//! Server callbacks are scoped to one admitted ADK call; no unsolicited prompts.
use super::*;
use crate::store::agent::elicitation;
use adk_session::SessionService;
use rmcp::{
    ClientHandler,
    model::{
        ElicitRequestParams, ElicitResult, ElicitationAction, ElicitationCapability,
        FormElicitationCapability, UrlElicitationCapability,
    },
    service::RequestContext,
};
use sailry_protocol::conversation::question::{Answer, Response};
use std::sync::Mutex;
use tokio::sync::watch;

pub(super) struct Handler {
    active: Mutex<Option<Arc<Call>>>,
    pub serial: tokio::sync::Mutex<()>,
}

struct Call {
    ingress: Arc<Ingress>,
    turn: TurnId,
    session: String,
    parent: ToolConfirmationRequest,
    stop: CancellationToken,
    waiting: watch::Sender<usize>,
}

pub(super) struct Active<'a> {
    handler: &'a Handler,
    call: Arc<Call>,
}

impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.handler.active.lock().unwrap().take();
        self.call.stop.cancel();
    }
}

impl Handler {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
            serial: tokio::sync::Mutex::new(()),
        }
    }
    pub fn begin(
        &self,
        ingress: Arc<Ingress>,
        turn: TurnId,
        session: String,
        parent: ToolConfirmationRequest,
        stop: &CancellationToken,
    ) -> Active<'_> {
        let (waiting, _) = watch::channel(0);
        let call = Arc::new(Call {
            ingress,
            turn,
            session,
            parent,
            stop: stop.child_token(),
            waiting,
        });
        *self.active.lock().unwrap() = Some(call.clone());
        Active {
            handler: self,
            call,
        }
    }
}

impl Active<'_> {
    pub async fn run<T>(&self, future: impl Future<Output = T>) -> Result<T, ()> {
        deadline(self.call.waiting.subscribe(), &self.call.stop, future).await
    }
}

async fn deadline<T>(
    mut waiting: watch::Receiver<usize>,
    stop: &CancellationToken,
    future: impl Future<Output = T>,
) -> Result<T, ()> {
    let mut remaining = Duration::from_secs(120);
    tokio::pin!(future);
    loop {
        let paused = *waiting.borrow_and_update() > 0;
        let start = tokio::time::Instant::now();
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(()),
            result = &mut future => return Ok(result),
            _ = tokio::time::sleep(remaining), if !paused => return Err(()),
            result = waiting.changed() => if result.is_err() { return Err(()) },
        }
        if !paused {
            remaining = remaining.saturating_sub(start.elapsed());
        }
    }
}

struct Pending {
    call: Arc<Call>,
    ingress: Arc<Ingress>,
    turn: TurnId,
    id: String,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.call.waiting.send_modify(|waiting| *waiting -= 1);
        let ingress = self.ingress.clone();
        let turn = self.turn;
        let id = self.id.clone();
        tokio::spawn(async move {
            ingress.cancel_input(turn, id).await;
        });
    }
}

impl ClientHandler for Handler {
    fn get_info(&self) -> ClientInfo {
        let mut info = ClientInfo::default();
        info.capabilities = rmcp::model::ClientCapabilities::builder()
            .enable_tasks()
            .build();
        info.capabilities.elicitation = Some(
            ElicitationCapability::new()
                .with_form(FormElicitationCapability::default())
                .with_url(UrlElicitationCapability::default()),
        );
        info
    }

    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, rmcp::ErrorData> {
        let call = self
            .active
            .lock()
            .unwrap()
            .clone()
            .filter(|call| !call.stop.is_cancelled())
            .ok_or_else(|| {
                rmcp::ErrorData::invalid_request("MCP input requires an active tool call", None)
            })?;
        let (spec, schema) = match request {
            ElicitRequestParams::FormElicitationParams {
                message,
                requested_schema,
                ..
            } => {
                let order = requested_schema.property_order.clone();
                let schema = serde_json::to_value(requested_schema).map_err(|_| failed())?;
                let spec = super::schema::spec(message, &schema, order.as_deref())?;
                (spec, schema)
            }
            ElicitRequestParams::UrlElicitationParams {
                message,
                url,
                elicitation_id,
                ..
            } => {
                let spec = sailry_protocol::conversation::question::Spec {
                    prompt: message,
                    input: sailry_protocol::conversation::question::Input::Url {
                        url,
                        elicitation_id,
                    },
                };
                spec.validate().map_err(|_| {
                    rmcp::ErrorData::invalid_params("invalid MCP URL request", None)
                })?;
                (spec, Value::Null)
            }
            _ => {
                return Err(rmcp::ErrorData::invalid_params(
                    "unsupported MCP input mode",
                    None,
                ));
            }
        };
        let input = elicitation::Input {
            parent: call.parent.clone(),
            request: serde_json::to_value(&context.id).map_err(|_| failed())?,
            spec: spec.clone(),
            schema,
        };
        let event = elicitation::event(call.turn, &input);
        let id = event.id.clone();
        // Pause only the in-flight tool deadline while actual user input is pending.
        call.waiting.send_modify(|waiting| *waiting += 1);
        let _pending = Pending {
            call: call.clone(),
            ingress: call.ingress.clone(),
            turn: call.turn,
            id: id.clone(),
        };
        let sessions = call.ingress.sessions(call.turn);
        let work = async {
            sessions
                .append_event(&call.session, event)
                .await
                .map_err(|_| failed())?;
            let response = call
                .ingress
                .ask(
                    call.turn,
                    ToolConfirmationRequest {
                        tool_name: elicitation::TOOL.into(),
                        function_call_id: Some(id.clone()),
                        args: serde_json::to_value(spec).map_err(|_| failed())?,
                    },
                )
                .await
                .map_err(|_| failed())?;
            sessions
                .append_event(&call.session, elicitation::result(call.turn, id, &response))
                .await
                .map_err(|_| failed())?;
            Ok(match response {
                Response::Answer(Answer::Opened) => ElicitResult::new(ElicitationAction::Accept),
                Response::Answer(Answer::Form(values)) => {
                    ElicitResult::new(ElicitationAction::Accept).with_content(Value::Object(values))
                }
                Response::Cancel => ElicitResult::new(ElicitationAction::Cancel),
                Response::Decline => ElicitResult::new(ElicitationAction::Decline),
                _ => return Err(failed()),
            })
        };
        tokio::select! {
            biased;
            _ = call.stop.cancelled() => Ok(ElicitResult::new(ElicitationAction::Cancel)),
            _ = context.ct.cancelled() => Ok(ElicitResult::new(ElicitationAction::Cancel)),
            result = work => result,
        }
    }
}

fn failed() -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error("MCP input is unavailable", None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn excludes_user_wait() {
        let (waiting, updates) = watch::channel(0);
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let job = tokio::spawn(async move {
            deadline(updates, &cancelled, std::future::pending::<()>()).await
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(60)).await;
        waiting.send_replace(1);
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(600)).await;
        assert!(!job.is_finished());
        waiting.send_replace(0);
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(59)).await;
        assert!(!job.is_finished());
        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(job.await.unwrap().is_err());

        let (_waiting, updates) = watch::channel(1);
        let cancelled = stop.clone();
        let job = tokio::spawn(async move {
            deadline(updates, &cancelled, std::future::pending::<()>()).await
        });
        tokio::task::yield_now().await;
        stop.cancel();
        assert!(job.await.unwrap().is_err());
    }
}
