//! The official SDK worker envelopes bind native session authority.
use super::Result;
pub(super) use cua_driver_sdk::worker::ChannelResponse as Response;
use cua_driver_sdk::{
    CuaDriver, CuaDriverSession,
    worker::{ActionCompletion, ChannelRequest, PRIVATE_WORKER_PROTOCOL_VERSION},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, Read, Write},
    sync::Arc,
};

const MAX_REQUEST: u64 = sailry_protocol::MAX_FRAME_BYTES as u64;
pub(super) type Sessions = HashMap<String, Arc<CuaDriverSession>>;

pub(super) fn read(reader: &mut impl BufRead, generation: &str) -> Result<Option<ChannelRequest>> {
    let mut bytes = Vec::new();
    if reader.take(MAX_REQUEST + 1).read_until(b'\n', &mut bytes)? == 0 {
        return Ok(None);
    }
    if bytes.len() as u64 > MAX_REQUEST {
        return Err("computer worker request is too large".into());
    }
    let request: ChannelRequest = serde_json::from_slice(&bytes)?;
    if request.protocol_version != PRIVATE_WORKER_PROTOCOL_VERSION
        || request.generation != generation
    {
        return Err("computer worker request belongs to another runtime generation".into());
    }
    Ok(Some(request))
}

pub(super) fn write(writer: &mut impl Write, response: Response) -> Result<()> {
    serde_json::to_writer(&mut *writer, &response)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub(super) async fn execute(
    driver: &CuaDriver,
    sessions: &mut Sessions,
    generation: &str,
    request: ChannelRequest,
) -> Response {
    if request.operation == "call"
        && request
            .session_handle
            .as_ref()
            .is_some_and(|handle| !sessions.contains_key(handle))
    {
        return Response::error(
            request.request_id,
            generation,
            "session_not_bound",
            "private worker session handle is not live on this channel",
            ActionCompletion::NotStarted,
        );
    }
    let result = dispatch(driver, sessions, &request).await;
    match result {
        Ok(value) => Response::ok(request.request_id, generation, value),
        Err(error) => Response::error(
            request.request_id,
            generation,
            "worker_request_failed",
            error.to_string(),
            ActionCompletion::Completed,
        ),
    }
}

async fn dispatch(
    driver: &CuaDriver,
    sessions: &mut Sessions,
    request: &ChannelRequest,
) -> Result<Value> {
    match request.operation.as_str() {
        "metadata" => Ok(serde_json::to_value(driver.metadata().await?)?),
        "list" => Ok(serde_json::from_str(&driver.list_tools_json().await?)?),
        "sessions_list" => Ok(serde_json::from_str(
            &driver.list_host_sessions_json().await?,
        )?),
        "bind_session" => {
            let options = serde_json::from_value(
                request
                    .arguments
                    .clone()
                    .ok_or("bind_session omitted options")?,
            )?;
            let session = driver.create_trusted_session(options)?;
            let handle = uuid::Uuid::new_v4().to_string();
            sessions.insert(handle.clone(), session);
            Ok(json!({"session_handle":handle}))
        }
        "close_session" => {
            let handle = request
                .session_handle
                .as_deref()
                .ok_or("close_session omitted session_handle")?;
            if let Some(session) = sessions.remove(handle) {
                session.close();
            }
            Ok(json!({"closed":true}))
        }
        "call" => {
            let name = request
                .name
                .as_deref()
                .ok_or("computer worker call has no name")?;
            let arguments = request.arguments.clone().unwrap_or_else(|| json!({}));
            let result = match request.session_handle.as_deref() {
                Some(handle) => {
                    sessions
                        .get(handle)
                        .ok_or("session handle is not bound")?
                        .call_tool(name.to_owned(), arguments.to_string())
                        .await?
                }
                None => {
                    driver
                        .call_tool_from_trusted_adapter(name, arguments)
                        .await?
                }
            };
            Ok(serde_json::from_str(&result.raw_json)?)
        }
        "shutdown" => {
            for (_, session) in sessions.drain() {
                session.close();
            }
            driver.shutdown().await?;
            Ok(json!({"shutdown":true}))
        }
        _ => Err("unsupported computer worker operation".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(generation: &str, handle: Option<&str>) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "protocol_version":PRIVATE_WORKER_PROTOCOL_VERSION,
            "request_id":1,"generation":generation,"operation":"call",
            "session_handle":handle,
        }))
        .unwrap()
    }

    #[test]
    fn validates_channel_identity() {
        assert!(read(&mut frame("first", None).as_slice(), "second").is_err());
        let request = read(&mut frame("first", Some("bound")).as_slice(), "first")
            .unwrap()
            .unwrap();
        assert_eq!(request.session_handle.as_deref(), Some("bound"));
        assert!(read(&mut &b""[..], "first").unwrap().is_none());
    }

    #[test]
    fn bounds_requests() {
        let oversized = vec![b' '; MAX_REQUEST as usize + 1];
        assert!(
            read(&mut oversized.as_slice(), "first")
                .unwrap_err()
                .to_string()
                .contains("too large")
        );
    }
}
