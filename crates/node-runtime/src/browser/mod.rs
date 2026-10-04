//! The execution Node routes tools to the initiating controller, not another desktop.
mod execute;
pub(crate) use execute::{execute, validate};

use sailry_link::{Pending, Subscription};
use sailry_protocol::{
    ErrorCode, Fault, NodeId, RequestId, SessionId, Update,
    browser::{self, Action, Call},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, oneshot};

type Replies = Arc<Mutex<HashMap<RequestId, oneshot::Sender<browser::Result>>>>;
struct Connection {
    id: RequestId,
    sender: mpsc::Sender<Call>,
    replies: Replies,
}
#[derive(Default)]
pub(crate) struct Browsers {
    connections: Mutex<HashMap<NodeId, Connection>>,
}
struct Calls {
    owner: Arc<Browsers>,
    caller: NodeId,
    id: RequestId,
    receiver: mpsc::Receiver<Call>,
}
struct PendingCall {
    id: RequestId,
    replies: Replies,
}
impl Drop for PendingCall {
    fn drop(&mut self) {
        self.replies.lock().unwrap().remove(&self.id);
    }
}
impl Browsers {
    pub(crate) fn available(&self, caller: NodeId) -> bool {
        self.connections
            .lock()
            .unwrap()
            .get(&caller)
            .is_some_and(|connection| !connection.sender.is_closed())
    }
    pub(crate) fn subscribe(
        self: &Arc<Self>,
        caller: NodeId,
    ) -> Result<Box<dyn Subscription>, Fault> {
        let mut connections = self.connections.lock().unwrap();
        if connections
            .get(&caller)
            .is_some_and(|connection| !connection.sender.is_closed())
        {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "browser controller is already connected",
            ));
        }
        let id = RequestId::new();
        let (sender, receiver) = mpsc::channel(16);
        connections.insert(
            caller,
            Connection {
                id,
                sender,
                replies: Arc::default(),
            },
        );
        Ok(Box::new(Calls {
            owner: self.clone(),
            caller,
            id,
            receiver,
        }))
    }
    pub(crate) async fn call(
        &self,
        caller: NodeId,
        session: SessionId,
        id: RequestId,
        action: Action,
    ) -> browser::Result {
        let read_only = action.read_only();
        let (reply, response) = oneshot::channel();
        let guard = {
            let connections = self.connections.lock().unwrap();
            let connection = connections.get(&caller).ok_or_else(disconnected)?;
            let mut replies = connection.replies.lock().unwrap();
            if replies.contains_key(&id) {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "browser call is already pending",
                ));
            }
            replies.insert(id, reply);
            let guard = PendingCall {
                id,
                replies: connection.replies.clone(),
            };
            drop(replies);
            let expires_at_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
                + 30_000;
            connection
                .sender
                .try_send(Call {
                    id,
                    session,
                    action,
                    expires_at_ms,
                })
                .map_err(|_| disconnected())?;
            guard
        };
        let result = match tokio::time::timeout(Duration::from_secs(30), response).await {
            Ok(Ok(result)) => result,
            _ => Err(Fault::new(
                if read_only {
                    ErrorCode::Unavailable
                } else {
                    ErrorCode::OutcomeUnknown
                },
                "browser response unavailable; read the page before trying another action",
            )),
        };
        drop(guard);
        result
    }
    pub(crate) fn complete(
        &self,
        caller: NodeId,
        id: RequestId,
        result: browser::Result,
    ) -> Result<(), Fault> {
        if serde_json::to_vec(&result).map_or(true, |bytes| bytes.len() > 256 * 1024) {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "browser result is too large",
            ));
        }
        let connections = self.connections.lock().unwrap();
        let connection = connections.get(&caller).ok_or_else(disconnected)?;
        let reply = connection
            .replies
            .lock()
            .unwrap()
            .remove(&id)
            .ok_or_else(|| Fault::new(ErrorCode::NotFound, "browser call is no longer pending"))?;
        reply.send(result).map_err(|_| disconnected())
    }
}
fn disconnected() -> Fault {
    Fault::new(ErrorCode::Unavailable, "browser controller is disconnected")
}
impl Subscription for Calls {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async {
            self.receiver
                .recv()
                .await
                .map(Update::BrowserCall)
                .ok_or_else(disconnected)
        })
    }
}
impl Drop for Calls {
    fn drop(&mut self) {
        let mut connections = self.owner.connections.lock().unwrap();
        if connections
            .get(&self.caller)
            .is_some_and(|connection| connection.id == self.id)
            && let Some(connection) = connections.remove(&self.caller)
        {
            connection.replies.lock().unwrap().clear();
        }
    }
}

#[cfg(test)]
mod tests;
