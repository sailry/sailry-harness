use futures_util::StreamExt;
use iroh::endpoint::ConnectionError;
use serde_json::{Value, json};

use crate::runtime::{Output, Owner, copy_input, error, spawn, state};

fn close_reason(reason: ConnectionError) -> Value {
    match reason {
        ConnectionError::ApplicationClosed(close) => json!({
            "kind": "application",
            "code": close.error_code.into_inner(),
            "reason": String::from_utf8_lossy(&close.reason),
        }),
        ConnectionError::LocallyClosed => json!({
            "kind": "local",
            "code": 0,
            "reason": "closed",
        }),
        reason => json!({
            "kind": "transport",
            "code": 0,
            "reason": reason.to_string(),
        }),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_open_bi(connection: u64) -> usize {
    let owner = state().connection_owner(connection);
    spawn(owner, async move {
        let entry = state().connection(connection)?;
        let (send, receive) = entry.connection.open_bi().await.map_err(error)?;
        Ok(Output::Streams(owner, send, receive))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_accept_bi(connection: u64) -> usize {
    let owner = state().connection_owner(connection);
    spawn(owner, async move {
        let entry = state().connection(connection)?;
        let (send, receive) = entry.connection.accept_bi().await.map_err(error)?;
        Ok(Output::Streams(owner, send, receive))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_conn_info(connection: u64) -> usize {
    let owner = state().connection_owner(connection);
    spawn(owner, async move {
        let entry = state().connection(connection)?;
        let connection = entry.connection;
        let reason = connection.close_reason();
        let paths = connection.paths();
        let selected = paths.iter().find(|path| path.is_selected());
        let (route, rtt) = if reason.is_none() {
            match selected {
                Some(path) if path.is_ip() => (
                    "p2p",
                    Some(path.rtt().as_millis().min(u64::MAX as u128) as u64),
                ),
                Some(path) if path.is_relay() => (
                    "relay",
                    Some(path.rtt().as_millis().min(u64::MAX as u128) as u64),
                ),
                _ => ("", None),
            }
        } else {
            ("", None)
        };
        Ok(Output::Json(json!({
            "remote_id": connection.remote_id().to_string(),
            "alpn": String::from_utf8_lossy(connection.alpn()),
            "route": route,
            "rtt_ms": rtt,
            "closed": reason.map(close_reason),
        })))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_conn_wait_path(connection: u64) -> usize {
    let owner = state().connection_owner(connection);
    spawn(owner, async move {
        let entry = state().connection(connection)?;
        let mut events = entry.connection.path_events();
        tokio::select! {
            _ = entry.connection.closed() => {},
            _ = events.next() => {},
        }
        Ok(Output::Empty)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sailry_iroh_conn_close(
    connection: u64,
    code: u64,
    reason: *const u8,
    length: usize,
) -> usize {
    let reason = unsafe { copy_input(reason, length) };
    let owner = Owner {
        connection: 0,
        ..state().connection_owner(connection)
    };
    spawn(owner, async move {
        state().close_connection(connection, code, reason).await?;
        Ok(Output::Empty)
    })
}
