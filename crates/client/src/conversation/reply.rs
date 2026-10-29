//! Shared final-reply boundary; closing interactions leave preceding prose visible.
use sailry_protocol::conversation::Part;

pub fn terminal_interaction(name: &str) -> bool {
    matches!(name, "ask_user" | "plan_review" | "mcp_elicitation")
}

pub fn is_work(part: &Part) -> bool {
    match part {
        Part::Text(_) | Part::Image(_) | Part::Attachment(_) | Part::Reference(_) => false,
        Part::ToolCall { name, .. } | Part::ToolResult { name, .. } => !terminal_interaction(name),
        Part::Resource(value)
            if value["type"] == "tool_started" || value["type"] == "provider_context" =>
        {
            false
        }
        _ => true,
    }
}

pub fn boundary(work: impl IntoIterator<Item = bool>) -> usize {
    work.into_iter()
        .enumerate()
        .filter_map(|(index, work)| work.then_some(index + 1))
        .last()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_closing_interactions_after_prose() {
        for name in ["plan_review", "ask_user", "mcp_elicitation"] {
            let parts = [
                Part::Thinking("Inspecting".into()),
                Part::Text("Published answer".into()),
                Part::ToolCall {
                    id: Some("closing".into()),
                    name: name.into(),
                    summary: Default::default(),
                    presentation: Default::default(),
                    grouping: Default::default(),
                },
                Part::Resource(json!({"type":"tool_started","id":"closing","name":name})),
                Part::ToolResult {
                    id: Some("closing".into()),
                    name: name.into(),
                    summary: Default::default(),
                    images: vec![],
                },
                Part::Resource(json!({"type":"provider_context"})),
            ];
            assert_eq!(boundary(parts.iter().map(is_work)), 1);
            assert_eq!(boundary(parts[1..].iter().map(is_work)), 0);
        }
    }

    #[test]
    fn keeps_ordinary_tool_work_collapsed() {
        let parts = [
            Part::Text("Progress".into()),
            Part::ToolResult {
                id: Some("read".into()),
                name: "read_file".into(),
                summary: Default::default(),
                images: vec![],
            },
            Part::Text("Final answer".into()),
        ];
        assert_eq!(boundary(parts.iter().map(is_work)), 2);
    }
}
