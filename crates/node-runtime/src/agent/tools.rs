//! Shared execution policy for plugin-defined worktree operations.
pub(super) mod scope;
use super::catalog::Registration;
use super::*;
use crate::agent::catalog::CallContext;
use async_trait::async_trait;
use goose_agent::{operation::Emitter, tool::ToolProvider};
use rmcp::model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations};
use sailry_link::{Local, Transport};
use sailry_protocol::{Command, Output, Request, WorktreeId};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

#[async_trait]
pub(super) trait Definition: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn parameters(&self) -> Value;
    fn read_only(&self) -> bool;
    fn presentation(&self) -> sailry_protocol::tool::Presentation {
        Default::default()
    }
    fn display(&self) -> Option<sailry_protocol::tool::Display> {
        None
    }
    fn command(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault>;
    async fn prepare(&self, binding: &Binding, arguments: Value) -> Result<Command, Fault> {
        self.command(binding, arguments)
    }
    async fn output(&self, _: &Binding, output: Output) -> Result<Value, Fault> {
        serde_json::to_value(output)
            .map_err(|_| Fault::new(ErrorCode::Internal, "tool result could not be encoded"))
    }
}

pub(super) struct Binding {
    pub ingress: Arc<Ingress>,
    pub turn: TurnId,
    pub transport: Arc<dyn Transport>,
    pub session: String,
    pub worktree: WorktreeId,
    pub stop: CancellationToken,
}

impl Binding {
    pub fn new(
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        stop: &CancellationToken,
    ) -> Arc<Self> {
        Arc::new(Self {
            ingress: ingress.clone(),
            turn: invocation.turn.id,
            transport: Arc::new(Local::new(ingress.node, invocation.caller, ingress.clone())),
            session: invocation.turn.session.to_string(),
            worktree: invocation.worktree,
            stop: stop.clone(),
        })
    }
}

struct Operation<D> {
    binding: Arc<Binding>,
    definition: D,
}

pub(super) fn bind<D: Definition + 'static>(
    binding: Arc<Binding>,
    definitions: impl IntoIterator<Item = D>,
) -> Vec<Registration> {
    definitions
        .into_iter()
        .map(|definition| {
            let presentation = definition.presentation();
            let display = definition.display();
            let tool = Arc::new(Operation {
                binding: binding.clone(),
                definition,
            });
            let mut registration = Registration::new(tool.descriptor(), tool, presentation);
            registration.display =
                display.map(|display| (registration.tool.name.to_string(), display));
            registration
        })
        .collect()
}

impl<D: Definition + 'static> Operation<D> {
    fn name(&self) -> &str {
        self.definition.name()
    }
    fn description(&self) -> &str {
        self.definition.description()
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(self.definition.parameters())
    }
    fn is_read_only(&self) -> bool {
        self.definition.read_only()
    }
    async fn execute(&self, context: Arc<CallContext>, arguments: Value) -> Result<Value, Fault> {
        if context.session_id() != self.binding.session {
            return Err(Fault::invalid("tool belongs to another session"));
        }
        if context.is_cancelled() || self.binding.stop.is_cancelled() {
            return Err(Fault::invalid("tool invocation was cancelled"));
        }
        let authorization = context.request();
        let result = async {
            let command = self.definition.prepare(&self.binding, arguments).await?;
            scope::validate(&self.binding, &command).await?;
            let mut request = Request::new(self.binding.transport.target(), command);
            if !self.is_read_only() {
                // Claim the exact persisted call once, including when a provider reuses a call ID.
                request.id = self
                    .binding
                    .ingress
                    .authorize(
                        self.binding.turn,
                        authorization.into(),
                        Some(request.command.clone()),
                    )
                    .await?;
                if context.is_cancelled() || self.binding.stop.is_cancelled() {
                    return Err(Fault::new(
                        ErrorCode::Cancelled,
                        "tool invocation was cancelled",
                    ));
                }
            }
            let admission = self.binding.transport.dispatch(request).await?;
            let output = admission.completion.await.map_err(|_| {
                Fault::new(ErrorCode::Unavailable, "tool response is unavailable")
            })??;
            self.definition.output(&self.binding, output).await
        };
        tokio::select! {
            biased;
            _ = self.binding.stop.cancelled() => Err(Fault::invalid("tool invocation was cancelled")),
            result = result => match result {
                Ok(output) => Ok(output),
                Err(error) => Ok(json!({"error": error})),
            },
        }
    }
}

pub(super) fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Fault> {
    serde_json::from_value(value)
        .map_err(|error| Fault::invalid(format!("invalid tool arguments: {error}")))
}

impl<D: Definition + 'static> Operation<D> {
    fn descriptor(&self) -> Tool {
        Tool::new(
            self.name().to_owned(),
            self.description().to_owned(),
            self.definition
                .parameters()
                .as_object()
                .cloned()
                .unwrap_or_default(),
        )
        .with_annotations(ToolAnnotations::new().read_only(self.is_read_only()))
    }
}
#[async_trait]
impl<D: Definition + 'static> ToolProvider<CallContext> for Operation<D> {
    async fn tools(&self, _: &CallContext) -> anyhow::Result<Vec<Tool>> {
        Ok(vec![self.descriptor()])
    }
    async fn call(
        &self,
        context: &CallContext,
        _: &str,
        call: CallToolRequestParams,
        _: &Emitter,
    ) -> Result<CallToolResult, ErrorData> {
        self.execute(
            Arc::new(context.clone()),
            Value::Object(call.arguments.unwrap_or_default()),
        )
        .await
        .and_then(crate::agent::catalog::json)
        .map_err(crate::agent::catalog::fault)
    }
}
