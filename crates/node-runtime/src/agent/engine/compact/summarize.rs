//! Derived from Goose 14741e9a, goose-context-management/src/summarize.rs.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.
use super::{
    Message, MessageContent, Model, PROMPT, ProviderError, ProviderUsage, format,
    structured::Summary, templates, tokens,
};

const REQUEST: &str = "Please summarize the conversation history provided in the system prompt.";

pub(super) async fn run(
    model: &Model,
    messages: &[Message],
) -> anyhow::Result<(Message, ProviderUsage)> {
    let request = [Message::user().with_text(REQUEST)];
    let transcript = messages
        .iter()
        .map(format::message)
        .collect::<Vec<_>>()
        .join("\n");
    let system = templates::render(PROMPT, &serde_json::json!({"messages":transcript}))?;
    let (mut response, mut usage) = match model.complete(&system, &request).await {
        Ok(output) => output,
        Err(ProviderError::ContextLengthExceeded(_)) => {
            anyhow::bail!(
                "Failed to compact: the base prompt (system prompt, tool schemas, and conversation) exceeds the model's effective context window, and there are no tool responses to remove. Use a model or configuration with a larger usable context, disable some extensions to reduce the tool-schema payload, or start a new session."
            );
        }
        Err(error) => return Err(error.into()),
    };
    response.role = rmcp::model::Role::User;
    estimate(&mut usage, &system, &request, &response);
    if let Some(summary) = Summary::parse(&response.as_concat_text()) {
        match summary.render() {
            Ok(rendered) if !rendered.trim().is_empty() => {
                response.content = vec![MessageContent::text(rendered)];
            }
            Ok(_) => tracing::warn!(
                "Structured compaction summary rendered empty (broken template override?), keeping raw output"
            ),
            Err(error) => tracing::warn!(
                "Failed to render structured compaction summary, keeping raw output: {error}"
            ),
        }
    }
    Ok((response, usage))
}

fn estimate(usage: &mut ProviderUsage, system: &str, request: &[Message], response: &Message) {
    if usage.usage.input_tokens.is_none() {
        usage.usage.input_tokens = Some(tokens::chat(system, request) as i32);
    }
    if usage.usage.output_tokens.is_none() {
        let text = response
            .content
            .iter()
            .map(|content| format!("{content}"))
            .collect::<Vec<_>>()
            .join(" ");
        usage.usage.output_tokens = Some(tokens::text(&text) as i32);
    }
    if let (Some(input), Some(output)) = (usage.usage.input_tokens, usage.usage.output_tokens) {
        usage.usage.total_tokens = Some(input + output);
    }
}
