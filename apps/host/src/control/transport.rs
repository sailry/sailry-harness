//! Profile-private forwarding preserves the Node's authenticated command and subscription boundary.
use sailry_link::{CancellationToken, Transport};
use sailry_protocol::{
    ErrorCode, Fault, MAX_FRAME_BYTES, NodeId, Output, Receipt, Request, Topic, Update,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(test)]
mod tests;

#[derive(Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Frame {
    #[serde(rename = "node_hello")]
    Hello { version: u8 },
    #[serde(rename = "node_request")]
    Request { version: u8, request: Box<Request> },
    #[serde(rename = "node_subscribe")]
    Subscribe { version: u8, topic: Topic },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: u8,
    node: NodeId,
    body: Reply,
}

#[derive(Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Reply {
    Hello,
    Admitted(Receipt),
    Completed(Box<std::result::Result<Output, Fault>>),
    Failed(Fault),
    Subscribed,
    Update(Box<Update>),
}

pub(super) async fn serve<R, W>(
    line: &str,
    mut reader: R,
    mut writer: W,
    transport: Arc<dyn Transport>,
    stop: CancellationToken,
) -> std::result::Result<(), super::Error>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if line.len() > MAX_FRAME_BYTES || !line.ends_with('\n') {
        return Err(Fault::invalid("invalid Host transport frame").into());
    }
    let frame: Frame = serde_json::from_str(line)?;
    let version = match &frame {
        Frame::Hello { version }
        | Frame::Request { version, .. }
        | Frame::Subscribe { version, .. } => *version,
    };
    let node = transport.target();
    if version != 1 {
        return respond(
            &mut writer,
            node,
            Reply::Failed(Fault::invalid("unsupported Host transport version")),
        )
        .await;
    }
    match frame {
        Frame::Hello { .. } => respond(&mut writer, node, Reply::Hello).await,
        Frame::Request { request, .. } => {
            let admission = match transport.dispatch(*request).await {
                Ok(admission) => admission,
                Err(error) => return respond(&mut writer, node, Reply::Failed(error)).await,
            };
            // The Node owns durable admission; this socket adds no ledger or execution owner.
            respond(&mut writer, node, Reply::Admitted(admission.receipt)).await?;
            let result = admission.completion.await.unwrap_or_else(|_| {
                Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "Node completion is unconfirmed",
                ))
            });
            respond(&mut writer, node, Reply::Completed(Box::new(result))).await
        }
        Frame::Subscribe { topic, .. } => {
            let mut updates = match transport.subscribe(topic).await {
                Ok(updates) => updates,
                Err(error) => return respond(&mut writer, node, Reply::Failed(error)).await,
            };
            respond(&mut writer, node, Reply::Subscribed).await?;
            let mut disconnected = [0];
            loop {
                let update = tokio::select! {
                    _ = stop.cancelled() => return Ok(()),
                    _ = reader.read(&mut disconnected) => return Ok(()),
                    update = updates.next() => update,
                };
                match update {
                    Ok(update) => {
                        respond(&mut writer, node, Reply::Update(Box::new(update))).await?
                    }
                    Err(error) => return respond(&mut writer, node, Reply::Failed(error)).await,
                }
            }
        }
    }
}

fn encode(value: &impl Serialize) -> std::result::Result<Vec<u8>, Fault> {
    let mut bytes =
        serde_json::to_vec(value).map_err(|_| Fault::invalid("invalid Host transport frame"))?;
    if bytes.len() + 1 > MAX_FRAME_BYTES {
        return Err(Fault::invalid("Host transport frame exceeds its limit"));
    }
    bytes.push(b'\n');
    Ok(bytes)
}

async fn send(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &impl Serialize,
) -> std::result::Result<(), Fault> {
    writer
        .write_all(&encode(value)?)
        .await
        .map_err(|_| Fault::new(ErrorCode::Unavailable, "Host transport disconnected"))
}

async fn respond(
    writer: &mut (impl AsyncWrite + Unpin),
    node: NodeId,
    body: Reply,
) -> std::result::Result<(), super::Error> {
    send(
        writer,
        &Response {
            version: 1,
            node,
            body,
        },
    )
    .await?;
    Ok(())
}
