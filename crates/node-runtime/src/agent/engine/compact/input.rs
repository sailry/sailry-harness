//! Bounded summary input; canonical history and the latest turn stay intact.
use super::{Content, Message, PROMPT, Role, format, templates::render, tokens};

const EDGE_CHARS: usize = 1_000;
// Covers the summary request, role wrappers and token-estimation boundaries.
const REQUEST_TOKENS: usize = 256;

#[derive(Clone, Copy)]
enum Detail {
    Preview,
    WithoutTools,
    UserOnly,
    Omitted,
}

/// Summarize a transcript rather than live tool messages, preserving the
/// latest turn's tool responses after its context-budget check.
pub(super) fn prepare(
    messages: &[Message],
    context: usize,
    output: usize,
) -> anyhow::Result<Vec<Message>> {
    let prompt = render(PROMPT, &serde_json::json!({"messages":""}))?;
    let budget = context
        .checked_sub(output.saturating_add(tokens::text(&prompt) + REQUEST_TOKENS))
        .ok_or_else(|| anyhow::anyhow!("summary prompt and output exceed the context limit"))?;
    let turns = turns(messages);
    let Some((latest, older)) = turns.split_last() else {
        return Ok(vec![]);
    };
    let latest = transcript(latest, None);
    if tokens::text(&latest) > budget {
        anyhow::bail!("latest turn exceeds the summary context limit");
    }
    let mut history: Vec<_> = older
        .iter()
        .map(|turn| transcript(turn, Some(Detail::Preview)))
        .collect();
    history.push(latest);
    let mut sizes: Vec<_> = history.iter().map(|text| tokens::text(text) + 2).collect();
    let mut total: usize = sizes.iter().sum();
    if let Some(messages) = fitted(&history, total, budget) {
        return Ok(messages);
    }
    // Every older turn passes through one level before any turn reaches the
    // next. Start with the oldest; the latest transcript is never changed.
    for detail in [Detail::WithoutTools, Detail::UserOnly, Detail::Omitted] {
        for (index, turn) in older.iter().enumerate() {
            history[index] = transcript(turn, Some(detail));
            total = total.saturating_sub(sizes[index]);
            sizes[index] = tokens::text(&history[index]) + 2;
            total += sizes[index];
            if let Some(messages) = fitted(&history, total, budget) {
                return Ok(messages);
            }
        }
    }
    // Empty older turns need no separator or token reserve.
    let latest = history.pop().unwrap();
    if tokens::text(&latest) <= budget {
        return Ok(vec![Message::user().with_text(latest)]);
    }
    anyhow::bail!("latest turn exceeds the summary context limit")
}

fn fitted(history: &[String], estimate: usize, budget: usize) -> Option<Vec<Message>> {
    if estimate > budget {
        return None;
    }
    let text = history
        .iter()
        .filter(|text| !text.is_empty())
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    (tokens::text(&text) <= budget).then(|| vec![Message::user().with_text(text)])
}

fn turns(messages: &[Message]) -> Vec<&[Message]> {
    let mut starts = vec![0];
    for (index, message) in messages.iter().enumerate().skip(1) {
        if user_input(message) {
            starts.push(index);
        }
    }
    starts.push(messages.len());
    starts
        .windows(2)
        .filter_map(|range| (range[0] < range[1]).then_some(&messages[range[0]..range[1]]))
        .collect()
}

fn user_input(message: &Message) -> bool {
    message.role == Role::User
        && message.content.iter().any(|content| {
            matches!(
                content,
                Content::Text { .. } | Content::Image { .. } | Content::Document { .. }
            )
        })
        && !message
            .content
            .iter()
            .any(|content| matches!(content, Content::ToolCall(_) | Content::ToolResult(_)))
}

fn transcript(messages: &[Message], detail: Option<Detail>) -> String {
    if matches!(detail, Some(Detail::Omitted)) {
        return String::new();
    }
    let mut parts = Vec::new();
    for message in messages {
        if matches!(detail, Some(Detail::UserOnly)) && !user_input(message) {
            continue;
        }
        for content in &message.content {
            if matches!(
                content,
                Content::Thinking { .. } | Content::RedactedThinking { .. }
            ) {
                continue;
            }
            let mut projected = Message {
                role: message.role,
                created: 0,
                content: vec![content.clone()],
                ..Message::default()
            };
            if matches!(detail, Some(Detail::WithoutTools))
                && matches!(content, Content::ToolResult(_))
            {
                projected.content = vec![Content::Text {
                    text: "tool_response: [omitted]".into(),
                }];
            }
            let formatted = format::message(&projected);
            let formatted = match (detail, content) {
                (Some(Detail::Preview), Content::ToolResult(_)) => {
                    match formatted.split_once("tool_response: ") {
                        Some((role, body)) => format!("{role}tool_response: {}", edges(body)),
                        None => formatted,
                    }
                }
                (Some(Detail::WithoutTools), Content::ToolCall(_)) => edges(&formatted),
                _ => formatted,
            };
            parts.push(formatted);
            if let Content::ToolResult(response) = content
                && let Ok(result) = &response.result
            {
                let source = message.metadata.operation_note("tool_output", &response.id)
                    .and_then(serde_json::Value::as_str)
                    .map(|revision| serde_json::json!({"call":response.id,"revision":revision}))
                    .or_else(|| result.structured_content.as_ref().map(|original| serde_json::json!({
                        "call":response.id,
                        "revision":blake3::hash(original.to_string().as_bytes()).to_hex().to_string(),
                    })));
                if let Some(source) = source {
                    parts.push(serde_json::json!({"output":source,"next_offset":null}).to_string());
                }
            }
        }
    }
    parts.join("\n")
}

fn edges(text: &str) -> String {
    let length = text.chars().count();
    if length <= EDGE_CHARS * 2 {
        return text.into();
    }
    let head: String = text.chars().take(EDGE_CHARS).collect();
    let tail: String = text.chars().skip(length - EDGE_CHARS).collect();
    format!(
        "{head}\n[{} characters omitted]\n{tail}",
        length - EDGE_CHARS * 2
    )
}

#[cfg(test)]
mod tests;
