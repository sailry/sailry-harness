use super::*;
use adk_core::{AdkError, AfterToolCallbackFull, BeforeToolCallback};

pub(super) fn before(binding: Arc<Binding>) -> BeforeToolCallback {
    Box::new(move |context| {
        let binding = binding.clone();
        Box::pin(async move {
            let ordinal = binding.before.fetch_add(1, Ordering::Relaxed);
            binding
                .execute(Phase::BeforeTool, ordinal, context.tool_name(), None)
                .await
                .map_err(|error| AdkError::tool(error.message))?;
            Ok(None)
        })
    })
}

pub(super) fn after(binding: Arc<Binding>) -> AfterToolCallbackFull {
    Box::new(move |context, tool, _, _| {
        let binding = binding.clone();
        Box::pin(async move {
            let ordinal = binding.after.fetch_add(1, Ordering::Relaxed);
            let status = context.tool_outcome().map(|outcome| {
                if outcome.success {
                    "succeeded"
                } else {
                    "failed"
                }
            });
            if let Err(error) = binding
                .execute(Phase::AfterTool, ordinal, Some(tool.name()), status)
                .await
            {
                // Keep the actual tool response intact: its side effects already happened.
                binding.remember(error);
            }
            Ok(None)
        })
    })
}
