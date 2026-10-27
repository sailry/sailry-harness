//! Guidance joins the running ADK invocation after its preceding tool batch.
use super::*;
use adk_core::{
    AdkError, AfterModelCallback, BeforeModelCallback, BeforeModelResult, Content, LlmRequest,
};
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Default)]
struct Inputs {
    insertions: Vec<(usize, Vec<Content>)>,
    queued: Option<Vec<sailry_protocol::conversation::QueuedMessage>>,
    calls: bool,
    rounds: u32,
}

impl Inputs {
    fn apply(&self, request: &mut LlmRequest) -> adk_core::Result<()> {
        for (index, contents) in self.insertions.iter().rev() {
            if *index > request.contents.len() {
                return Err(AdkError::agent("guidance model history changed"));
            }
            request.contents.splice(*index..*index, contents.clone());
        }
        Ok(())
    }
}

pub(super) fn bind(
    ingress: &Arc<Ingress>,
    invocation: &Arc<Invocation>,
    context: Arc<compaction::active::Context>,
    resources: Arc<crate::plugins::resources::Resources>,
    tools: BTreeMap<String, Vec<String>>,
    stop: CancellationToken,
) -> (BeforeModelCallback, AfterModelCallback) {
    let ingress = ingress.clone();
    let invocation = invocation.clone();
    let owner = invocation.turn.id;
    let session = invocation.turn.session;
    let child = invocation.child.is_some();
    let inputs = Arc::new(Mutex::new(Inputs::default()));
    let tools = Arc::new(tools);
    let limit = invocation
        .child
        .as_ref()
        .and_then(|child| child.role.as_ref())
        .and_then(|role| role.max_turns);
    let continuation = continue_pending(&ingress, owner, inputs.clone(), stop.clone(), limit);
    let before: BeforeModelCallback = Box::new(move |callback, mut request| {
        let ingress = ingress.clone();
        let invocation = invocation.clone();
        let context = context.clone();
        let inputs = inputs.clone();
        let resources = resources.clone();
        let tools = tools.clone();
        let stop = stop.clone();
        Box::pin(async move {
            let frontier = request.contents.len();
            {
                let mut inputs = inputs
                    .lock()
                    .map_err(|_| AdkError::agent("guidance history is unavailable"))?;
                inputs.apply(&mut request)?;
                inputs.calls = false;
                inputs.rounds = inputs.rounds.saturating_add(1);
            }
            if stop.is_cancelled() {
                return Err(AdkError::agent("guidance was cancelled"));
            }
            let queued = inputs
                .lock()
                .map_err(|_| AdkError::agent("guidance history is unavailable"))?
                .queued
                .take();
            let queued = match queued {
                Some(queued) => Some(queued),
                None => ingress.take_guidance(owner).await.map_err(model_error)?,
            };
            if let Some(queued) = queued {
                if stop.is_cancelled() {
                    return Err(AdkError::agent("guidance was cancelled"));
                }
                let mut batch = inputs::Batch::guidance(&ingress, owner, &queued)
                    .await
                    .map_err(model_error)?;
                batch
                    .select(&invocation, &resources, &tools)
                    .await
                    .map_err(model_error)?;
                let context_event = prompt::Session::prepare(&ingress, &invocation, &queued, &stop)
                    .await
                    .map_err(model_error)?;
                let mut contents = context_event
                    .as_ref()
                    .and_then(|event| event.content().cloned())
                    .into_iter()
                    .collect::<Vec<_>>();
                contents.extend(batch.pending());
                if stop.is_cancelled() {
                    return Err(AdkError::agent("guidance was cancelled"));
                }
                context.preflight(&contents, request.tools.clone()).await?;
                if stop.is_cancelled() {
                    return Err(AdkError::agent("guidance was cancelled"));
                }
                batch
                    .append_guidance(
                        &ingress,
                        session,
                        owner,
                        callback.invocation_id(),
                        if child { "parent" } else { "user" },
                        context_event,
                    )
                    .await
                    .map_err(model_error)?;
                request.contents.extend(contents.clone());
                inputs
                    .lock()
                    .map_err(|_| AdkError::agent("guidance history is unavailable"))?
                    .insertions
                    .push((frontier, contents));
            }
            Ok(BeforeModelResult::Continue(request))
        })
    });
    (before, continuation)
}

fn model_error(error: Fault) -> AdkError {
    AdkError::agent("queued guidance could not be applied").with_source(error)
}

/// Claim input before requesting continuation, after the complete response settles.
fn continue_pending(
    ingress: &Arc<Ingress>,
    owner: TurnId,
    inputs: Arc<Mutex<Inputs>>,
    stop: CancellationToken,
    limit: Option<u32>,
) -> AfterModelCallback {
    let ingress = ingress.clone();
    Box::new(move |_, mut response| {
        let ingress = ingress.clone();
        let inputs = inputs.clone();
        let stop = stop.clone();
        Box::pin(async move {
            let (calls, claimed, rounds) = {
                let mut inputs = inputs
                    .lock()
                    .map_err(|_| AdkError::agent("guidance history is unavailable"))?;
                inputs.calls |= response.content.as_ref().is_some_and(|content| {
                    content
                        .parts
                        .iter()
                        .any(|part| matches!(part, adk_core::Part::FunctionCall { .. }))
                });
                (inputs.calls, inputs.queued.is_some(), inputs.rounds)
            };
            if response.partial
                || stop.is_cancelled()
                || calls
                || limit.is_some_and(|limit| rounds >= limit)
                || !(response.turn_complete || response.finish_reason.is_some())
                || !ingress.agents.running(owner)
            {
                return Ok(None);
            }
            let queued = if claimed {
                None
            } else {
                ingress.take_guidance(owner).await.map_err(model_error)?
            };
            let continuation = claimed || queued.is_some();
            if let Some(queued) = queued {
                inputs
                    .lock()
                    .map_err(|_| AdkError::agent("guidance history is unavailable"))?
                    .queued = Some(queued);
            }
            if continuation && !stop.is_cancelled() && ingress.agents.running(owner) {
                let metadata = response
                    .provider_metadata
                    .get_or_insert_with(|| serde_json::json!({}));
                metadata["continue_turn"] = serde_json::Value::Bool(true);
                return Ok(Some(response));
            }
            Ok(None)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Callback;

    #[async_trait::async_trait]
    impl adk_core::ReadonlyContext for Callback {
        fn invocation_id(&self) -> &str {
            "guidance-fixture"
        }
        fn agent_name(&self) -> &str {
            "assistant"
        }
        fn user_id(&self) -> &str {
            "user"
        }
        fn app_name(&self) -> &str {
            "app"
        }
        fn session_id(&self) -> &str {
            "session"
        }
        fn branch(&self) -> &str {
            ""
        }
        fn user_content(&self) -> &Content {
            static CONTENT: std::sync::OnceLock<Content> = std::sync::OnceLock::new();
            CONTENT.get_or_init(|| Content::new("user"))
        }
    }

    impl adk_core::CallbackContext for Callback {
        fn artifacts(&self) -> Option<Arc<dyn adk_core::Artifacts>> {
            None
        }
    }

    #[tokio::test]
    async fn continues_already_claimed_input() {
        let directory = tempfile::tempdir().unwrap();
        let node = crate::Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let owner = TurnId::new();
        let stop = CancellationToken::new();
        node.ingress
            .agents
            .register(owner, SessionId::new(), stop.clone());
        let inputs = Arc::new(Mutex::new(Inputs {
            queued: Some(Vec::new()),
            rounds: 1,
            ..Default::default()
        }));
        let callback = continue_pending(&node.ingress, owner, inputs.clone(), stop.clone(), None);
        for _ in 0..2 {
            let response = callback(
                Arc::new(Callback),
                adk_core::LlmResponse {
                    turn_complete: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(response.provider_metadata.unwrap()["continue_turn"], true);
            assert!(inputs.lock().unwrap().queued.is_some());
        }
        stop.cancel();
        assert!(
            callback(
                Arc::new(Callback),
                adk_core::LlmResponse {
                    turn_complete: true,
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .is_none()
        );
        assert!(inputs.lock().unwrap().queued.is_some());
        node.ingress.agents.finish(owner);
        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn leaves_input_at_iteration_limit() {
        let directory = tempfile::tempdir().unwrap();
        let node = crate::Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let owner = TurnId::new();
        let stop = CancellationToken::new();
        node.ingress
            .agents
            .register(owner, SessionId::new(), stop.clone());
        let inputs = Arc::new(Mutex::new(Inputs {
            rounds: 1,
            ..Default::default()
        }));
        let callback = continue_pending(&node.ingress, owner, inputs.clone(), stop, Some(1));
        assert!(
            callback(
                Arc::new(Callback),
                adk_core::LlmResponse {
                    turn_complete: true,
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .is_none()
        );
        assert!(inputs.lock().unwrap().queued.is_none());
        node.ingress.agents.finish(owner);
        node.shutdown().await.unwrap();
    }

    #[test]
    fn preserves_each_prefix_across_requests() {
        let mut inputs = Inputs::default();
        let original = vec![Content::new("user").with_text("Task")];
        let first = Content::new("user").with_text("First guidance");
        inputs.insertions.push((1, vec![first.clone()]));
        let mut request = adk_core::LlmRequest::new("fixture", original.clone());
        inputs.apply(&mut request).unwrap();
        assert_eq!(request.contents[0].parts, original[0].parts);
        assert_eq!(request.contents[1].parts, first.parts);
        let prefix = serde_json::to_vec(&request.contents).unwrap();
        request.contents = original;
        request
            .contents
            .push(Content::new("model").with_text("First result"));
        let second = Content::new("user").with_text("Second guidance");
        inputs.insertions.push((2, vec![second.clone()]));
        inputs.apply(&mut request).unwrap();
        assert_eq!(serde_json::to_vec(&request.contents[..2]).unwrap(), prefix);
        assert_eq!(request.contents[3].parts, second.parts);
        assert_eq!(request.contents.len(), 4);
    }
}
