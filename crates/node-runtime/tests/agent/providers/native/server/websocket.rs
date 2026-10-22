use super::*;
use futures::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_tungstenite::tungstenite::{
    Message, handshake::server as handshake, protocol::CloseFrame,
};

#[derive(Clone, Copy)]
pub enum Mode {
    Refuse(u16),
    Accept,
    Close(u16),
    Limit,
    Partial,
    MissingContinuation,
    AckTimeout,
    FirstTimeout,
    ErrorOnce,
    InvalidFrame,
    NativeComplete,
    Interrupt {
        failures: usize,
        partial: bool,
        after_tool: bool,
    },
    RefuseAfter {
        accepted: usize,
        status: u16,
    },
}

struct Closed(Arc<AtomicUsize>);
impl Drop for Closed {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Capture {
    records: Arc<Mutex<Vec<Request>>>,
    mode: Mode,
}
impl handshake::Callback for Capture {
    fn on_request(
        self,
        request: &handshake::Request,
        response: handshake::Response,
    ) -> Result<handshake::Response, handshake::ErrorResponse> {
        let mut records = self.records.lock().unwrap();
        let index = records.len();
        records.push(Request {
            received: Instant::now(),
            received_at: SystemTime::now(),
            path: request.uri().path().to_owned(),
            headers: request
                .headers()
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_str().unwrap().to_owned()))
                .collect(),
            body: serde_json::Value::Null,
        });
        if let Mode::RefuseAfter { accepted, status } = self.mode
            && index >= accepted
        {
            let mut error = handshake::ErrorResponse::new(Some("fixture refusal".into()));
            *error.status_mut() = status.try_into().unwrap();
            return Err(error);
        }
        Ok(response)
    }
}

pub(super) async fn respond(
    stream: TcpStream,
    records: Arc<Mutex<Vec<Request>>>,
    upgrades: Arc<Mutex<Vec<Request>>>,
    closed: Arc<AtomicUsize>,
    reply: Reply,
    mode: Mode,
) {
    let result = tokio_tungstenite::accept_hdr_async(
        stream,
        Capture {
            records: upgrades,
            mode,
        },
    )
    .await;
    let Ok(mut socket) = result else {
        return;
    };
    let _closed = Closed(closed);
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let body: serde_json::Value = serde_json::from_str(&text).unwrap();
        let tool = matches!(reply, Reply::Tool) && !body.to_string().contains("工具内容 🙂");
        let missing = body.get("previous_response_id").is_some();
        let index = {
            let mut records = records.lock().unwrap();
            let index = records.len();
            records.push(Request {
                received: Instant::now(),
                received_at: SystemTime::now(),
                path: "/v1/responses".into(),
                headers: BTreeMap::new(),
                body,
            });
            index
        };
        match mode {
            Mode::Interrupt {
                failures,
                partial,
                after_tool,
            } if index >= usize::from(after_tool) && index < usize::from(after_tool) + failures => {
                let _ = socket.send(Message::Text(json!({"type":"response.created","response":{"id":format!("failed_{index}"),"status":"in_progress"}}).to_string().into())).await;
                if partial {
                    let _ = socket
                        .send(Message::Text(text_delta("FAILED_DRAFT").to_string().into()))
                        .await;
                }
                let _ = socket.close(None).await;
                return;
            }
            Mode::NativeComplete => {
                let _ = socket.send(Message::Text(json!({"type":"response.output_item.done","sequence_number":0,"output_index":0,"item":{"id":"native-search","type":"web_search_call","status":"completed","action":{"type":"search","query":"fixture"}}}).to_string().into())).await;
                let _ = socket.close(None).await;
                return;
            }
            Mode::MissingContinuation if missing => {
                let _ = socket
                    .send(Message::Text(
                        json!({"type":"error",
                    "error":{"code":"previous_response_not_found"}})
                        .to_string()
                        .into(),
                    ))
                    .await;
                continue;
            }
            Mode::FirstTimeout => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                return;
            }
            Mode::AckTimeout => {
                let mut response = response_events(false, false).pop().unwrap()["response"].clone();
                response["status"] = json!("in_progress");
                response["output"] = json!([]);
                let _ = socket
                    .send(Message::Text(
                        json!({"type":"response.created",
                    "sequence_number":0,"response":response})
                        .to_string()
                        .into(),
                    ))
                    .await;
                tokio::time::sleep(Duration::from_secs(1)).await;
                return;
            }
            Mode::ErrorOnce if index == 0 => {
                let mut response = response_events(false, false).pop().unwrap()["response"].clone();
                response["status"] = json!("in_progress");
                response["output"] = json!([]);
                let _ = socket
                    .send(Message::Text(
                        json!({"type":"response.created",
                    "sequence_number":0,"response":response})
                        .to_string()
                        .into(),
                    ))
                    .await;
                let _ = socket
                    .send(Message::Text(
                        json!({"type":"response.failed", "response":{"error":{"code":"rate_limit_exceeded","message":KEY,"headers":{"retry-after":"0"}}}})
                        .to_string()
                        .into(),
                    ))
                    .await;
                continue;
            }
            Mode::InvalidFrame => {
                let mut response = response_events(false, false).pop().unwrap()["response"].clone();
                response["status"] = json!("in_progress");
                response["output"] = json!([]);
                let _ = socket
                    .send(Message::Text(
                        json!({"type":"response.created",
                    "sequence_number":0,"response":response})
                        .to_string()
                        .into(),
                    ))
                    .await;
                let _ = socket.get_mut().write_all(&[0x83, 0x00]).await;
                return;
            }
            Mode::Close(code) => {
                let _ = socket
                    .close(Some(CloseFrame {
                        code: code.into(),
                        reason: "fixture refusal".into(),
                    }))
                    .await;
                return;
            }
            Mode::Limit => {
                let _ = socket.send(Message::Text(json!({"type":"error", "error":{"code":"websocket_connection_limit_reached"}}).to_string().into())).await;
                continue;
            }
            Mode::Partial if index == 0 => {
                let _ = socket
                    .send(Message::Text(text_delta("原").to_string().into()))
                    .await;
                let _ = socket.close(None).await;
                return;
            }
            _ => {}
        }
        if let Reply::Wait(closed) = &reply {
            let _ = socket
                .send(Message::Text(text_delta("waiting").to_string().into()))
                .await;
            let _ = socket.next().await;
            closed.notify_one();
            return;
        }
        for (index, event) in response_events(tool, matches!(reply, Reply::Stream(_)))
            .into_iter()
            .enumerate()
        {
            if index > 0
                && let Reply::Stream(release) = &reply
            {
                release.notified().await;
            }
            if socket
                .send(Message::Text(event.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
        }
    }
}
