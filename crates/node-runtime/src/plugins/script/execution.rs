//! Typed staging for the Runner's authoritative tool-result commit.
use super::*;
use serde_json::{Value, json};

pub(crate) struct State {
    pub call_id: Option<sailry_protocol::RequestId>,
    pub mode: sailry_protocol::WorkMode,
    pub read_only: bool,
    pub turn: Option<Value>,
    pub staged_turn: Option<Value>,
}

impl State {
    pub(super) fn call(&mut self, operation: &str, body: &str) -> Result<Value, Fault> {
        if operation == "id.call" {
            return self
                .call_id
                .map(|id| json!(id))
                .ok_or_else(|| invalid("operation requires an Agent tool call"));
        }
        if matches!(operation, "turn.read" | "turn.stage") {
            let current = self
                .turn
                .as_ref()
                .ok_or_else(|| invalid("package has no turn policy"))?;
            if operation == "turn.read" {
                return Ok(self.staged_turn.as_ref().unwrap_or(current).clone());
            }
            if body.len() > plugin::host::MAX_DATA_BYTES {
                return Err(invalid("plugin turn state exceeds limit"));
            }
            let value: Value =
                serde_json::from_str(body).map_err(|_| invalid("invalid turn state"))?;
            self.staged_turn = Some(value.clone());
            return Ok(value);
        }
        Err(invalid("unknown tool state operation"))
    }
}
