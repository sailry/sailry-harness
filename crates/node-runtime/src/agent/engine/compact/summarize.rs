//! Derived from Goose 14741e9a, goose-context-management/src/summarize.rs.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.
use super::{
    Content, ErrorKind, Message, Model, PROMPT, Role, Usage, format, structured::Summary,
    templates, tokens,
};

const REQUEST: &str = "Please summarize the conversation history provided in the system prompt.";

pub(super) async fn run(model: &Model, messages: &[Message]) -> anyhow::Result<(Message, Usage)> {
    let request = [Message::user().with_text(REQUEST)];
    let transcript = messages
        .iter()
        .map(format::message)
        .collect::<Vec<_>>()
        .join("\n");
    let system = templates::render(PROMPT, &serde_json::json!({"messages":transcript}))?;
    let (mut response, mut usage) = match model.complete(&system, &request).await {
        Ok(output) => output,
        Err(error) if error.kind == ErrorKind::ContextOverflow => {
            anyhow::bail!(
                "Failed to compact: the base prompt (system prompt, tool schemas, and conversation) exceeds the model's effective context window, and there are no tool responses to remove. Use a model or configuration with a larger usable context, disable some extensions to reduce the tool-schema payload, or start a new session."
            );
        }
        Err(error) => return Err(error.into()),
    };
    response.role = Role::User;
    estimate(&mut usage, &system, &request, &response);
    if let Some(summary) = Summary::parse(&response.as_concat_text()) {
        match summary.render() {
            Ok(rendered) if !rendered.trim().is_empty() => {
                response.content = vec![Content::Text { text: rendered }];
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

fn estimate(usage: &mut Usage, system: &str, request: &[Message], response: &Message) {
    if usage.input.is_none() {
        usage.input = Some(tokens::chat(system, request) as u64);
    }
    if usage.output.is_none() {
        usage.output = Some(tokens::text(&response.as_concat_text()) as u64);
    }
    if let (Some(input), Some(output)) = (usage.input, usage.output) {
        usage.total = Some(input + output);
    }
}
