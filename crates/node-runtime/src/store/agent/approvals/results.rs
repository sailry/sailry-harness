//! Denial explanations annotate the canonical result and the current model request.
use super::*;

#[derive(Clone, Debug)]
pub(crate) struct Denial {
    pub call: String,
    pub tool: String,
    pub reason: String,
}

pub(super) fn validate_reason(reason: &str) -> Result<(), Fault> {
    if reason.trim().is_empty() || reason.contains('\0') || reason.chars().count() > 2000 {
        return Err(Fault::invalid(
            "denial reason must contain 1 to 2000 characters without NUL",
        ));
    }
    Ok(())
}

pub(crate) async fn read(db: &Connection, turn: TurnId) -> Result<Vec<Denial>, Fault> {
    let mut rows = db.query("SELECT body,call FROM agent_approvals WHERE turn=?1 AND json_extract(body,'$.state')='denied' AND json_type(body,'$.reason')='text' ORDER BY rowid", [turn.to_string()]).await.map_err(storage_error)?;
    let mut selected = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let body: Vec<u8> = row.get(0).map_err(storage_error)?;
        let call: String = row.get(1).map_err(storage_error)?;
        let approval: Approval = serde_json::from_slice(&body).map_err(storage_error)?;
        let request = super::request(db, &approval).await?;
        selected.push(Denial {
            call,
            tool: request.tool_name,
            reason: approval
                .reason
                .ok_or_else(|| Fault::invalid("denied approval has no explanation"))?,
        });
    }
    Ok(selected)
}

pub(crate) fn enrich(content: &mut crate::agent::canonical::Message, denials: &[Denial]) {
    use crate::agent::canonical::{BlockKind, ToolFormat};
    for part in &mut content.content {
        if let BlockKind::ToolResponse(response) = &mut part.kind
            && let Some(id) = &response.id
            && let Some(denial) = denials
                .iter()
                .find(|denial| &denial.call == id && denial.tool == response.name)
        {
            let value = serde_json::json!({"error":format!("Tool execution was denied. Reason: {}",denial.reason)});
            response.revision = blake3::hash(value.to_string().as_bytes()).to_hex().to_string();
            response.result = value.into();
            response.format = ToolFormat::Json;
            response.status = crate::agent::canonical::ToolStatus::Error;
        }
    }
}

pub(in crate::store::agent) async fn append(
    db: &Connection,
    turn: TurnId,
    event: &mut crate::agent::canonical::Record,
) -> Result<(), Fault> {
    if let crate::agent::canonical::Kind::Message(content) = &mut event.kind {
        enrich(content, &read(db, turn).await?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_unicode_reason() {
        assert!(validate_reason("  ").is_err());
        assert!(validate_reason("reason\0").is_err());
        assert!(validate_reason(&"🙂".repeat(2000)).is_ok());
        assert!(validate_reason(&"🙂".repeat(2001)).is_err());
    }

    #[test]
    fn requires_exact_call_and_tool() {
        let response = |id: &str, tool: &str| adk_core::Part::FunctionResponse {
            id: Some(id.into()),
            function_response: adk_core::FunctionResponseData::new(
                tool,
                serde_json::json!({"error":"denied"}),
            ),
            annotations: None,
        };
        let mut content = adk_core::Content::new("function");
        content.parts = vec![
            response("call", "tool"),
            response("other", "tool"),
            response("call", "other"),
        ];
        let original = content.clone();
        enrich(
            &mut content,
            &[Denial {
                call: "call".into(),
                tool: "tool".into(),
                reason: "Read the file first".into(),
            }],
        );
        assert_ne!(content.parts[0], original.parts[0]);
        assert_eq!(content.parts[1..], original.parts[1..]);
    }
}
