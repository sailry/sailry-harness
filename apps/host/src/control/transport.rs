//! Profile-private forwarding preserves the Node's authenticated command and subscription boundary.
use sailry_link::{Admission, CancellationToken, Pending, Subscription, Transport};
use sailry_protocol::{
    ErrorCode, Fault, MAX_FRAME_BYTES, NodeId, Output, Receipt, Request, Topic, Update,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    io::{
        AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
        BufReader,
    },
    net::UnixStream,
    sync::oneshot,
};
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

pub(crate) struct Socket {
    path: PathBuf,
    node: NodeId,
}

impl Socket {
    pub(crate) async fn connect(profile: &Path) -> std::result::Result<Self, Fault> {
        let path = profile.join("control.sock");
        let mut stream = connect(&path).await?;
        send(&mut stream, &Frame::Hello { version: 1 }).await?;
        let response: Response = receive(&mut BufReader::new(stream)).await?;
        if response.version != 1 || !matches!(response.body, Reply::Hello) {
            return Err(Fault::invalid("invalid Host connection response"));
        }
        Ok(Self {
            path,
            node: response.node,
        })
    }

    async fn open_frame(
        &self,
        frame: &Frame,
        command: bool,
    ) -> std::result::Result<BufReader<UnixStream>, Fault> {
        let bytes = encode(frame)?;
        let mut stream = connect(&self.path).await?;
        stream.write_all(&bytes).await.map_err(|_| {
            Fault::new(
                if command {
                    ErrorCode::OutcomeUnknown
                } else {
                    ErrorCode::Unavailable
                },
                "Host request transmission is unconfirmed",
            )
        })?;
        Ok(BufReader::new(stream))
    }
}

impl Transport for Socket {
    fn target(&self) -> NodeId {
        self.node
    }

    fn dispatch(&self, request: Request) -> Pending<'_, std::result::Result<Admission, Fault>> {
        Box::pin(async move {
            let id = request.id;
            let durable = request.command.durable();
            let mut reader = self
                .open_frame(
                    &Frame::Request {
                        version: 1,
                        request: Box::new(request),
                    },
                    true,
                )
                .await?;
            let response: Response = receive(&mut reader).await.map_err(unconfirmed)?;
            check(&response, self.node).map_err(unconfirmed)?;
            let receipt = match response.body {
                Reply::Admitted(receipt) if receipt.id == id && receipt.durable == durable => {
                    receipt
                }
                Reply::Failed(error) => return Err(error),
                _ => {
                    return Err(unconfirmed(Fault::invalid(
                        "Host admission response is mismatched",
                    )));
                }
            };
            let (sender, completion) = oneshot::channel();
            let node = self.node;
            tokio::spawn(async move {
                let result = async {
                    let response: Response = receive(&mut reader).await.map_err(unconfirmed)?;
                    check(&response, node).map_err(unconfirmed)?;
                    match response.body {
                        Reply::Completed(result) => *result,
                        _ => Err(unconfirmed(Fault::invalid(
                            "Host completion response is mismatched",
                        ))),
                    }
                }
                .await;
                let _ = sender.send(result);
            });
            Ok(Admission {
                receipt,
                completion,
            })
        })
    }

    fn subscribe(
        &self,
        topic: Topic,
    ) -> Pending<'_, std::result::Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            let mut reader = self
                .open_frame(&Frame::Subscribe { version: 1, topic }, false)
                .await?;
            let response: Response = receive(&mut reader).await?;
            check(&response, self.node)?;
            match response.body {
                Reply::Subscribed => Ok(Box::new(Updates {
                    reader,
                    node: self.node,
                    payload: Vec::new(),
                }) as Box<dyn Subscription>),
                Reply::Failed(error) => Err(error),
                _ => Err(Fault::invalid("invalid Host subscription response")),
            }
        })
    }
}

struct Updates {
    reader: BufReader<UnixStream>,
    node: NodeId,
    payload: Vec<u8>,
}

impl Subscription for Updates {
    fn next(&mut self) -> Pending<'_, std::result::Result<Update, Fault>> {
        Box::pin(async move {
            let response: Response = receive_buffered(&mut self.reader, &mut self.payload).await?;
            check(&response, self.node)?;
            match response.body {
                Reply::Update(update) => Ok(*update),
                Reply::Failed(error) => Err(error),
                _ => Err(Fault::invalid("invalid Host subscription frame")),
            }
        })
    }
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

async fn connect(path: &Path) -> std::result::Result<UnixStream, Fault> {
    UnixStream::connect(path).await.map_err(|_| {
        Fault::new(
            ErrorCode::Unavailable,
            "Host is not running; use sailry start",
        )
    })
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

async fn receive<T: serde::de::DeserializeOwned>(
    reader: &mut (impl AsyncBufRead + Unpin),
) -> std::result::Result<T, Fault> {
    receive_buffered(reader, &mut Vec::new()).await
}

async fn receive_buffered<T: serde::de::DeserializeOwned>(
    reader: &mut (impl AsyncBufRead + Unpin),
    payload: &mut Vec<u8>,
) -> std::result::Result<T, Fault> {
    loop {
        let buffer = reader
            .fill_buf()
            .await
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "Host transport disconnected"))?;
        if buffer.is_empty() {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "Host transport disconnected",
            ));
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(buffer.len(), |position| position + 1);
        if payload.len() + count > MAX_FRAME_BYTES {
            return Err(Fault::invalid("Host transport frame exceeds its limit"));
        }
        payload.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if newline.is_some() {
            let response = serde_json::from_slice(payload)
                .map_err(|_| Fault::invalid("invalid Host transport frame"));
            payload.clear();
            return response;
        }
    }
}

fn check(response: &Response, node: NodeId) -> std::result::Result<(), Fault> {
    if response.version != 1 || response.node != node {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "Host response identity is mismatched",
        ));
    }
    Ok(())
}

fn unconfirmed(error: Fault) -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        format!("Host outcome is unconfirmed: {}", error.message),
    )
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
