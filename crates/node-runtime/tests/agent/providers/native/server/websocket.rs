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
}

struct Closed(Arc<AtomicUsize>);
impl Drop for Closed {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Capture(Arc<Mutex<Vec<Request>>>);
impl handshake::Callback for Capture {
    fn on_request(
        self,
        request: &handshake::Request,
        response: handshake::Response,
    ) -> Result<handshake::Response, handshake::ErrorResponse> {
        self.0.lock().unwrap().push(Request {
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
    let result = tokio_tungstenite::accept_hdr_async(stream, Capture(upgrades)).await;
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
