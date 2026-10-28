//! Delegated work uses the existing Node admission and GDK child operation.
use super::*;
use crate::agent::catalog::CallContext;
use sailry_protocol::conversation::Input;
use serde_json::{Value, json};
mod control;
mod observation;
pub(in crate::agent) mod scheduler;
pub(super) mod selected;
pub(in crate::agent) use control::Control;

fn selected_call(turn: TurnId) -> String {
    format!("sailry-selected-{turn}")
}

pub(super) fn selection(
    invocation: &Invocation,
    name: String,
) -> Result<
    Arc<
        dyn goose_agent::operation::Operation<
                crate::store::agent::RuntimeSession,
                super::engine::Effect,
            >,
    >,
    Fault,
> {
    Ok(Arc::new(Selection {
        arguments: selected::arguments(invocation)?,
        name,
        call: selected_call(invocation.turn.id),
    }))
}

struct Selection {
    arguments: Value,
    name: String,
    call: String,
}

#[async_trait::async_trait]
impl goose_agent::operation::Operation<crate::store::agent::RuntimeSession, super::engine::Effect>
    for Selection
{
    fn name(&self) -> &'static str {
        "selected_role"
    }

    async fn run(
        &self,
        _: &crate::store::agent::RuntimeSession,
        conversation: &goose_provider_types::conversation::Conversation,
        _: &goose_agent::operation::Emitter,
    ) -> anyhow::Result<goose_agent::operation::OperationResult<super::engine::Effect>> {
        use goose_provider_types::conversation::message::{Message, MessageContentBlock};
        if conversation.messages().iter().flat_map(|message| &message.content).any(|block| {
            matches!(block, MessageContentBlock::ToolRequest(request) if request.id == self.call)
        }) {
            return goose_agent::operation::not_applicable();
        }
        let call = rmcp::model::CallToolRequestParams::new(self.name.clone())
            .with_arguments(self.arguments.as_object().cloned().unwrap_or_default());
        let message = Message::assistant().with_content(MessageContentBlock::tool_request(
            self.call.clone(),
            Ok(call),
        ));
        goose_agent::operation::applied([message.into()])
    }
}

pub(super) struct Delegate {
    ingress: Arc<Ingress>,
    tasks: Tasks,
    turn: TurnId,
    session: String,
    parent: sailry_protocol::SessionId,
    stop: CancellationToken,
    name: String,
    description: String,
    parameters: Value,
    selected: Option<Input>,
}

impl Delegate {
    fn is_read_only(&self) -> bool {
        false
    }
    fn record_failure(&self, context: &CallContext, error: &Fault) {
        if let Some(timeline) = self.ingress.agents.timeline(self.turn) {
            timeline.fail_tool(
                context.branch(),
                context.function_call_id(),
                &self.name,
                Some(error.code),
            );
        }
    }

    pub fn new(
        ingress: &Arc<Ingress>,
        tasks: &Tasks,
        invocation: &Invocation,
        stop: &CancellationToken,
        name: String,
        description: String,
        parameters: Value,
    ) -> Self {
        Self {
            ingress: ingress.clone(),
            tasks: tasks.clone(),
            turn: invocation.turn.id,
            session: invocation.turn.session.to_string(),
            parent: invocation.turn.session,
            stop: stop.clone(),
            name,
            parameters,
            description,
            selected: invocation.message.selected_role().map(|_| {
                let mut message = invocation.message.clone();
                message.references.retain(|reference| {
                    !matches!(
                        reference.target,
                        sailry_protocol::conversation::reference::Target::Agent(_)
                    )
                });
                if message.is_empty() {
                    message.text = "Complete the request using the selected context".into();
                }
                message
            }),
        }
    }
}

struct Active {
    ingress: Arc<Ingress>,
    turn: TurnId,
}

struct Child {
    ingress: Arc<Ingress>,
    parent: sailry_protocol::SessionId,
    child: sailry_protocol::SessionId,
    turn: TurnId,
}

impl Drop for Child {
    fn drop(&mut self) {
        self.ingress
            .agents
            .finish_child(self.parent, self.child, self.turn);
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        self.ingress.agents.finish_operation(self.turn);
    }
}

impl Delegate {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(self.parameters.clone())
    }
    async fn execute(
        &self,
        context: Arc<CallContext>,
        arguments: Value,
    ) -> Result<rmcp::model::CallToolResult, Fault> {
        self.execute_prepared(context, arguments.clone(), arguments)
            .await
    }
}

impl Delegate {
    pub(super) async fn execute_prepared(
        &self,
        context: Arc<CallContext>,
        arguments: Value,
        original: Value,
    ) -> Result<rmcp::model::CallToolResult, Fault> {
        if context.session_id() != self.session
            || context.is_cancelled()
            || self.stop.is_cancelled()
        {
            return Err(Fault::invalid("delegation is no longer active"));
        }
        let background = arguments
            .get("background")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let children = if background {
            None
        } else {
            Some(
                context
                    .children()
                    .ok_or_else(|| Fault::invalid("child execution is unavailable"))?
                    .clone(),
            )
        };
        let parent_stop = self
            .ingress
            .agents
            .begin_operation(self.turn)
            .ok_or_else(|| Fault::invalid("parent execution has ended"))?;
        let active = Active {
            ingress: self.ingress.clone(),
            turn: self.turn,
        };
        let message = (context.function_call_id() == selected_call(self.turn))
            .then(|| self.selected.clone())
            .flatten();
        let request = context.request_with(original);
        if let Some(children) = children {
            let ingress = self.ingress.clone();
            let turn = self.turn;
            // Admission can commit after the calling tool future is cancelled.
            // Keep its handoff and parent lease owned by the Node task supervisor.
            let admitted = self
                .tasks
                .spawn_child(&self.ingress.agents.stopped, async move {
                    let _active = active;
                    let invocation = ingress
                        .delegate(
                            turn,
                            request.into(),
                            arguments,
                            message,
                            parent_stop.child_token(),
                        )
                        .await?;
                    let session = invocation.turn.session;
                    let turn = invocation.turn.id;
                    children.register(invocation).map_err(|_| {
                        Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "child was admitted but handoff failed; execution was not replayed",
                        )
                    })?;
                    Ok::<_, Fault>((session, turn))
                })
                .await
                .map_err(|_| Fault::invalid("child supervisor is unavailable"))?;
            let (session, turn) = admitted.await.map_err(|_| {
                Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "child admission outcome is unknown",
                )
            })??;
            let mut result = crate::agent::catalog::json(
                json!({"admitted":true,"pending":true,"session":session,"turn":turn,"status":"queued"}),
            )?;
            let meta = result.meta.get_or_insert_with(Default::default);
            meta.insert("foreground_subagent".into(), json!(true));
            meta.insert("subagent_session_id".into(), json!(session.to_string()));
            return Ok(result);
        }
        let ingress = self.ingress.clone();
        let tasks = self.tasks.clone();
        let turn = self.turn;
        let parent = self.parent;
        let stop = self.ingress.agents.stopped.child_token();
        let (admitted, admission) = tokio::sync::oneshot::channel();
        let result = self.tasks.spawn_child(&self.ingress.agents.stopped, async move {
            let _cancel = stop.clone().drop_guard();
            let mut token = stop.child_token();
            let mut invocation = ingress.delegate(turn, request.into(), arguments, message, token.clone()).await?;
            ingress.agents.register_child(parent, invocation.turn.session, invocation.turn.id, stop.clone());
            let _child = Child { ingress:ingress.clone(),parent,child:invocation.turn.session,turn:invocation.turn.id };
            if parent_stop.is_cancelled() {stop.cancel();}
            drop(active);
            let _ = admitted.send((invocation.turn.session,invocation.turn.id));
            loop {
                let session = invocation.turn.session;
                let turn = invocation.turn.id;
                let mut result = scheduler::run(&ingress,&tasks,invocation,token,stop.clone()).await;
                let next_token = stop.child_token();
                let next = if result.is_err() || stop.is_cancelled() || ingress.agents.stopped.is_cancelled() {
                    ingress.retain_guidance(turn).await?;
                    None
                } else {
                    match ingress.claim_guidance(turn,next_token.clone()).await {
                        Ok(next) => next,
                        Err(error) => {ingress.retain_guidance(turn).await?;result=Err(error);None}
                    }
                };
                if let Some(next) = next {invocation=next;token=next_token;continue;}
                return Ok::<_,Fault>(match result {
                    Ok((status,response))=>json!({"session":session,"turn":turn,"status":status,"response":response}),
                    Err(error)=>json!({"isError":true,"session":session,"turn":turn,"error":error}),
                });
            }
        }).await.map_err(|_|Fault::invalid("child supervisor is unavailable"))?;
        match admission.await {
            Ok((session, turn)) => crate::agent::catalog::json(
                json!({"admitted":true,"pending":true,"session":session,"turn":turn,"status":"queued"}),
            ),
            Err(_) => match result.await {
                Ok(Err(error)) => {
                    self.record_failure(context.as_ref(), &error);
                    crate::agent::catalog::json(json!({"isError":true,"error":error}))
                }
                _ => Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "child admission outcome is unknown",
                )),
            },
        }
    }
}

crate::agent::catalog::provider!(Delegate, crate::agent::catalog::result);
