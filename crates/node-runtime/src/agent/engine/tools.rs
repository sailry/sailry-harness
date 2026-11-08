//! Ordered cancellation recovery adapted from Goose 14741e9a tool.rs (Apache-2.0).
//! See third_party_licenses/goose-Apache-2.0.txt.
use super::{Effect, Session, conversation, operation::*};
use crate::agent::{
    catalog::ToolProvider,
    controls::Controls,
    model::{
        Content, Message,
        types::{ToolCall, ToolResult},
    },
};
use futures::{StreamExt, stream};
use rmcp::model::{CallToolRequestParams, CallToolResult, ErrorData, Tool};
use sailry_protocol::TurnId;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

const MAX_CONCURRENT_READS: usize = 8;

struct Slot {
    request: ToolCall,
    result: Option<Result<CallToolResult, ErrorData>>,
}

pub(in crate::agent) struct Tools {
    provider: Arc<dyn ToolProvider<Session>>,
    controls: Arc<Controls>,
    turn: TurnId,
    responses: Mutex<Vec<Slot>>,
}

impl Tools {
    pub(in crate::agent) fn new(
        provider: Arc<dyn ToolProvider<Session>>,
        controls: Arc<Controls>,
        turn: TurnId,
    ) -> Self {
        Self {
            provider,
            controls,
            turn,
            responses: Mutex::default(),
        }
    }

    pub(super) async fn definitions(&self, session: &Session) -> anyhow::Result<Vec<Tool>> {
        self.provider.tools(session).await
    }

    async fn dispatch(&self, session: &Session, index: usize, request: &ToolCall, emit: &Emitter) {
        let result = if let Some(error) = &request.error {
            Err(ErrorData {
                code: rmcp::model::ErrorCode(error.code),
                message: error.message.clone().into(),
                data: error.data.clone(),
            })
        } else {
            self.provider
                .call(session, &request.id, parameters(request), emit)
                .await
        };
        self.responses.lock().unwrap()[index].result = Some(result);
    }

    fn take(&self, interrupted: bool) -> Vec<Effect> {
        let slots = std::mem::take(&mut *self.responses.lock().unwrap());
        let mut answered: BTreeSet<_> = slots
            .iter()
            .filter(|slot| slot.result.is_some())
            .map(|slot| slot.request.id.clone())
            .collect();
        let content = slots
            .into_iter()
            .filter_map(|slot| {
                let result = match slot.result {
                    Some(result) => result,
                    None if interrupted && answered.insert(slot.request.id.clone()) => {
                        interrupted_call()
                    }
                    None => return None,
                };
                Some(Content::ToolResult(ToolResult {
                    id: slot.request.id,
                    name: slot.request.name,
                    metadata: slot.request.metadata,
                    result,
                }))
            })
            .collect::<Vec<_>>();
        if content.is_empty() {
            Vec::new()
        } else {
            vec![
                Message {
                    content,
                    ..Message::user()
                }
                .into(),
            ]
        }
    }

    pub(in crate::agent) fn finalize_cancellation(&self) -> Vec<Effect> {
        self.take(true)
    }

    pub(in crate::agent) async fn run(
        &self,
        session: &Session,
        messages: &[Message],
        emit: &Emitter,
    ) -> anyhow::Result<Outcome> {
        let definitions = self.definitions(session).await?;
        let names: BTreeSet<_> = definitions.iter().map(|tool| tool.name.as_ref()).collect();
        let requests: Vec<_> = conversation::pending(messages)?
            .into_iter()
            .filter(|call| {
                call.error.is_some()
                    || (conversation::request_executable(call) == Some(true)
                        && names.contains(call.name.as_str()))
            })
            .cloned()
            .collect();
        if requests.is_empty() {
            return not_applicable();
        }
        *self.responses.lock().unwrap() = requests
            .iter()
            .cloned()
            .map(|request| Slot {
                request,
                result: None,
            })
            .collect();
        let token = self
            .controls
            .tool_token(self.turn)
            .unwrap_or_else(|| emit.cancel_token().clone());
        let scoped = emit.scoped(token.clone());
        let dispatch = async {
            let mut start = 0;
            while start < requests.len() {
                if token.is_cancelled() {
                    break;
                }
                let concurrent = |request: &ToolCall| {
                    request.error.is_some()
                        || self.provider.concurrent(session, &parameters(request))
                };
                let end = if concurrent(&requests[start]) {
                    requests[start..]
                        .iter()
                        .position(|request| !concurrent(request))
                        .map_or(requests.len(), |offset| start + offset)
                } else {
                    start + 1
                };
                stream::iter(start..end)
                    .for_each_concurrent(MAX_CONCURRENT_READS, |index| {
                        self.dispatch(session, index, &requests[index], &scoped)
                    })
                    .await;
                start = end;
            }
        };
        tokio::select! {
            biased;
            _ = token.cancelled() => {}
            _ = dispatch => {}
        }
        applied(self.take(token.is_cancelled()))
    }
}

fn parameters(request: &ToolCall) -> CallToolRequestParams {
    CallToolRequestParams::new(request.name.clone())
        .with_arguments(request.arguments.as_object().cloned().unwrap_or_default())
}

/// A stopped call is a Sailry runtime outcome, encoded as a fault so its row
/// reads "interrupted" rather than a tool-reported error.
pub(super) fn interrupted_call() -> Result<CallToolResult, ErrorData> {
    Err(crate::agent::catalog::fault(sailry_protocol::Fault::new(
        sailry_protocol::ErrorCode::OutcomeUnknown,
        "Tool call was interrupted before completing",
    )))
}
