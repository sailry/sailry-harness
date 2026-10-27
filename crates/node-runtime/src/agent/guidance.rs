//! Guidance joins the running ADK invocation after its preceding tool batch.
use super::*;
use adk_core::{
    AdkError, AfterModelCallback, BeforeModelCallback, BeforeModelResult, Content, LlmRequest,
};
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Default)]
struct Inputs {
    insertions: Vec<(usize, Vec<Content>)>,
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
) -> BeforeModelCallback {
    let ingress = ingress.clone();
    let invocation = invocation.clone();
    let owner = invocation.turn.id;
    let session = invocation.turn.session;
    let child = invocation.child.is_some();
    let inputs = Arc::new(Mutex::new(Inputs::default()));
    let tools = Arc::new(tools);
    Box::new(move |callback, mut request| {
        let ingress = ingress.clone();
        let invocation = invocation.clone();
        let context = context.clone();
        let inputs = inputs.clone();
        let resources = resources.clone();
        let tools = tools.clone();
        let stop = stop.clone();
        Box::pin(async move {
            let frontier = request.contents.len();
            inputs
                .lock()
                .map_err(|_| AdkError::agent("guidance history is unavailable"))?
                .apply(&mut request)?;
            if stop.is_cancelled() {
                return Ok(BeforeModelResult::Continue(request));
            }
            if let Some(queued) = ingress.take_guidance(owner).await.map_err(model_error)? {
                let batch = inputs::Batch::guidance(&ingress, owner, &queued)
                    .await
                    .map_err(model_error)?;
                let pending = batch.pending();
                let capabilities = references::capabilities::instruction(
                    &invocation,
                    queued.iter().flat_map(|input| &input.message.references),
                    &resources,
                    &tools,
                )
                .await
                .map_err(model_error)?;
                let mut contents =
                    prompt::Session::refresh(&ingress, &invocation, &queued, &capabilities, &stop)
                        .await
                        .map_err(model_error)?
                        .into_iter()
                        .collect::<Vec<_>>();
                context.preflight(&pending, request.tools.clone()).await?;
                contents.extend(pending);
                if stop.is_cancelled() {
                    return Ok(BeforeModelResult::Continue(request));
                }
                batch
                    .append_guidance(
                        &ingress,
                        session,
                        owner,
                        callback.invocation_id(),
                        if child { "parent" } else { "user" },
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
    })
}

fn model_error(error: Fault) -> AdkError {
    AdkError::agent("queued guidance could not be applied").with_source(error)
}

/// Finish the current response normally, then collect pending input at the next boundary.
pub(super) fn continue_pending(ingress: &Arc<Ingress>, owner: TurnId) -> AfterModelCallback {
    let ingress = ingress.clone();
    Box::new(move |_, mut response| {
        let ingress = ingress.clone();
        Box::pin(async move {
            if response.partial
                || !(response.turn_complete || response.finish_reason.is_some())
                || !ingress.agents.running(owner)
            {
                return Ok(None);
            }
            if ingress.pending_guidance(owner).await.map_err(model_error)? {
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
