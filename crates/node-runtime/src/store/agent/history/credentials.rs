//! Public calls omit typed SSH credentials; canonical ADK history retains its exact arguments.
use super::*;

pub(super) async fn project(
    db: &Connection,
    turn: TurnId,
    name: &str,
    args: &Value,
) -> Result<Value, Fault> {
    let Some(fields) = args.as_object() else {
        return Ok(args.clone());
    };
    if !fields.contains_key("credential") && !fields.contains_key("key_path") {
        return Ok(args.clone());
    }
    if super::super::plugins::operation(db, turn, name).await?
        != Some(sailry_protocol::tool::Operation::SaveSshConnection)
    {
        return Ok(args.clone());
    }
    let mut fields = fields.clone();
    fields.remove("credential");
    fields.remove("key_path");
    Ok(Value::Object(fields))
}
