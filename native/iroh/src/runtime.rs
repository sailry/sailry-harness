use std::{
    collections::HashMap,
    future::Future,
    panic::AssertUnwindSafe,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

use futures_util::FutureExt;
use iroh::{
    Endpoint,
    endpoint::{Connection, RecvStream, SendStream},
};
use serde_json::{Value, json};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

pub type Callback = extern "C" fn(usize, usize, *const u8, usize);
pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Default)]
pub struct Owner {
    pub endpoint: u64,
    pub connection: u64,
    pub direction: u64,
}

#[derive(Clone)]
pub struct ConnectionEntry {
    pub endpoint: u64,
    pub connection: Connection,
}

#[derive(Clone)]
pub struct SendEntry {
    pub owner: Owner,
    pub stream: Arc<AsyncMutex<SendStream>>,
    pub finished: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct RecvEntry {
    pub owner: Owner,
    pub stream: Arc<AsyncMutex<RecvStream>>,
}

struct Operation {
    owner: Owner,
    stop: CancellationToken,
    done: CancellationToken,
}

#[derive(Default)]
struct Registry {
    endpoints: HashMap<u64, Endpoint>,
    connections: HashMap<u64, ConnectionEntry>,
    sends: HashMap<u64, SendEntry>,
    receives: HashMap<u64, RecvEntry>,
    operations: HashMap<usize, Operation>,
}

pub enum Output {
    Empty,
    Json(Value),
    Bytes(Vec<u8>),
    Eof,
    Endpoint(Endpoint),
    Connection(u64, Connection),
    Streams(Owner, SendStream, RecvStream),
}

pub struct State {
    runtime: tokio::runtime::Runtime,
    callback: Callback,
    next_operation: AtomicUsize,
    next_handle: AtomicU64,
    registry: Mutex<Registry>,
}

static STATE: OnceLock<State> = OnceLock::new();

pub fn init(callback: Callback) -> i32 {
    if let Some(state) = STATE.get() {
        return if state.callback as usize == callback as usize {
            0
        } else {
            1
        };
    }
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    else {
        return 1;
    };
    let state = State {
        runtime,
        callback,
        next_operation: AtomicUsize::new(1),
        next_handle: AtomicU64::new(1),
        registry: Mutex::new(Registry::default()),
    };
    if STATE.set(state).is_ok() { 0 } else { 1 }
}

pub fn state() -> &'static State {
    STATE.get().expect("iroh is initialized before use")
}

pub fn spawn(owner: Owner, future: impl Future<Output = Result<Output>> + Send + 'static) -> usize {
    let Some(state) = STATE.get() else {
        return 0;
    };
    let operation = state.next_operation.fetch_add(1, Ordering::Relaxed);
    let stop = CancellationToken::new();
    let done = CancellationToken::new();
    state.registry.lock().unwrap().operations.insert(
        operation,
        Operation {
            owner,
            stop: stop.clone(),
            done: done.clone(),
        },
    );
    state.runtime.spawn(async move {
        let _done = done.drop_guard();
        // A Tokio task panic must still settle its callback and release the
        // descendant-operation waiters used when closing its owner.
        let result = AssertUnwindSafe(async move {
            tokio::select! {
                biased;
                _ = stop.cancelled() => None,
                result = future => Some(result),
            }
        })
        .catch_unwind()
        .await
        .unwrap_or_else(|_| Some(Err("iroh operation panicked".into())));
        state.complete(operation, result);
    });
    operation
}

impl State {
    fn handle(&self) -> u64 {
        self.next_handle.fetch_add(1, Ordering::Relaxed)
    }

    pub fn endpoint(&self, handle: u64) -> Result<Endpoint> {
        self.registry
            .lock()
            .unwrap()
            .endpoints
            .get(&handle)
            .cloned()
            .ok_or_else(|| "iroh endpoint is closed".into())
    }

    pub fn connection(&self, handle: u64) -> Result<ConnectionEntry> {
        self.registry
            .lock()
            .unwrap()
            .connections
            .get(&handle)
            .cloned()
            .ok_or_else(|| "iroh connection is closed".into())
    }

    pub fn send(&self, handle: u64) -> Result<SendEntry> {
        self.registry
            .lock()
            .unwrap()
            .sends
            .get(&handle)
            .cloned()
            .ok_or_else(|| "iroh send stream is closed".into())
    }

    pub fn receive(&self, handle: u64) -> Result<RecvEntry> {
        self.registry
            .lock()
            .unwrap()
            .receives
            .get(&handle)
            .cloned()
            .ok_or_else(|| "iroh receive stream is closed".into())
    }

    pub fn connection_owner(&self, handle: u64) -> Owner {
        let endpoint = self
            .connection(handle)
            .map(|entry| entry.endpoint)
            .unwrap_or_default();
        Owner {
            endpoint,
            connection: handle,
            direction: 0,
        }
    }

    pub fn send_owner(&self, handle: u64) -> Owner {
        self.send(handle)
            .map(|entry| entry.owner)
            .unwrap_or_default()
    }

    pub fn receive_owner(&self, handle: u64) -> Owner {
        self.receive(handle)
            .map(|entry| entry.owner)
            .unwrap_or_default()
    }

    pub fn cancel(&self, operation: usize) {
        if let Some(operation) = self.registry.lock().unwrap().operations.get(&operation) {
            operation.stop.cancel();
        }
    }

    pub async fn close_endpoint(&self, handle: u64) -> Result<()> {
        let (endpoint, pending) = {
            let mut registry = self.registry.lock().unwrap();
            let Some(endpoint) = registry.endpoints.remove(&handle) else {
                return Ok(());
            };
            let pending = cancel_owned(&registry, |owner| owner.endpoint == handle);
            registry
                .connections
                .retain(|_, entry| entry.endpoint != handle);
            registry
                .sends
                .retain(|_, entry| entry.owner.endpoint != handle);
            registry
                .receives
                .retain(|_, entry| entry.owner.endpoint != handle);
            (endpoint, pending)
        };
        endpoint.close().await;
        wait(pending).await;
        Ok(())
    }

    pub async fn close_connection(&self, handle: u64, code: u64, reason: Vec<u8>) -> Result<()> {
        let code = iroh::endpoint::VarInt::from_u64(code).map_err(error)?;
        let pending = {
            let mut registry = self.registry.lock().unwrap();
            let Some(entry) = registry.connections.remove(&handle) else {
                return Ok(());
            };
            entry.connection.close(code, &reason);
            let pending = cancel_owned(&registry, |owner| owner.connection == handle);
            registry
                .sends
                .retain(|_, entry| entry.owner.connection != handle);
            registry
                .receives
                .retain(|_, entry| entry.owner.connection != handle);
            pending
        };
        wait(pending).await;
        Ok(())
    }

    pub async fn drop_streams(&self, send: u64, receive: u64) -> Result<()> {
        let (send, receive, pending) = {
            let mut registry = self.registry.lock().unwrap();
            let pending = cancel_owned(&registry, |owner| {
                owner.direction != 0 && (owner.direction == send || owner.direction == receive)
            });
            (
                registry.sends.remove(&send),
                registry.receives.remove(&receive),
                pending,
            )
        };
        wait(pending).await;
        if let Some(send) = send {
            if !send.finished.load(Ordering::Acquire) {
                let _ = send.stream.lock().await.reset(0u32.into());
            }
        }
        if let Some(receive) = receive {
            let _ = receive.stream.lock().await.stop(0u32.into());
        }
        Ok(())
    }

    pub async fn interrupt_direction(&self, handle: u64) {
        let pending = {
            let registry = self.registry.lock().unwrap();
            cancel_owned(&registry, |owner| owner.direction == handle)
        };
        wait(pending).await;
    }

    pub fn debug_count(&self, kind: usize) -> usize {
        let registry = self.registry.lock().unwrap();
        match kind {
            0 => registry.endpoints.len(),
            1 => registry.connections.len(),
            2 => registry.sends.len(),
            3 => registry.receives.len(),
            4 => registry.operations.len(),
            _ => 0,
        }
    }

    fn complete(&self, operation: usize, result: Option<Result<Output>>) {
        let mut status = 0;
        let result = {
            let mut registry = self.registry.lock().unwrap();
            match result {
                None => {
                    status = 2;
                    Ok(Vec::new())
                }
                Some(Err(message)) => Err(message),
                Some(Ok(Output::Empty)) => Ok(b"null".to_vec()),
                Some(Ok(Output::Json(value))) => serde_json::to_vec(&value).map_err(error),
                Some(Ok(Output::Bytes(bytes))) => Ok(bytes),
                Some(Ok(Output::Eof)) => {
                    status = 3;
                    Ok(Vec::new())
                }
                Some(Ok(Output::Endpoint(endpoint))) => {
                    let handle = self.handle();
                    registry.endpoints.insert(handle, endpoint);
                    Ok(json!({ "handle": handle }).to_string().into_bytes())
                }
                Some(Ok(Output::Connection(endpoint, connection))) => {
                    if registry.endpoints.contains_key(&endpoint) {
                        let handle = self.handle();
                        registry.connections.insert(
                            handle,
                            ConnectionEntry {
                                endpoint,
                                connection,
                            },
                        );
                        Ok(json!({ "handle": handle }).to_string().into_bytes())
                    } else {
                        connection.close(0u32.into(), b"endpoint closed");
                        Err("iroh endpoint is closed".into())
                    }
                }
                Some(Ok(Output::Streams(owner, send, receive))) => {
                    if registry.connections.contains_key(&owner.connection) {
                        let send_handle = self.handle();
                        let receive_handle = self.handle();
                        registry.sends.insert(
                            send_handle,
                            SendEntry {
                                owner: Owner {
                                    direction: send_handle,
                                    ..owner
                                },
                                stream: Arc::new(AsyncMutex::new(send)),
                                finished: Arc::new(AtomicBool::new(false)),
                            },
                        );
                        registry.receives.insert(
                            receive_handle,
                            RecvEntry {
                                owner: Owner {
                                    direction: receive_handle,
                                    ..owner
                                },
                                stream: Arc::new(AsyncMutex::new(receive)),
                            },
                        );
                        Ok(json!({ "send": send_handle, "recv": receive_handle })
                            .to_string()
                            .into_bytes())
                    } else {
                        Err("iroh connection is closed".into())
                    }
                }
            }
        };
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(message) => {
                status = 1;
                json!({ "message": message }).to_string().into_bytes()
            }
        };
        if bytes.is_empty() {
            (self.callback)(operation, status, std::ptr::null(), 0);
        } else {
            let length = bytes.len();
            let pointer = Box::into_raw(bytes.into_boxed_slice()) as *const u8;
            (self.callback)(operation, status, pointer, length);
        }
        self.registry.lock().unwrap().operations.remove(&operation);
    }
}

fn cancel_owned(registry: &Registry, matches: impl Fn(Owner) -> bool) -> Vec<CancellationToken> {
    registry
        .operations
        .values()
        .filter(|operation| matches(operation.owner))
        .map(|operation| {
            operation.stop.cancel();
            operation.done.clone()
        })
        .collect()
}

async fn wait(pending: Vec<CancellationToken>) {
    for done in pending {
        done.cancelled().await;
    }
}

pub fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

pub unsafe fn copy_input(pointer: *const u8, length: usize) -> Vec<u8> {
    if length == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    type Completion = (usize, usize, Vec<u8>);
    static COMPLETIONS: OnceLock<mpsc::Sender<Completion>> = OnceLock::new();

    extern "C" fn completed(operation: usize, status: usize, pointer: *const u8, length: usize) {
        let bytes = if pointer.is_null() {
            Vec::new()
        } else {
            // The callback owns the same buffer that the Go binding frees.
            unsafe {
                Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    pointer as *mut u8,
                    length,
                ))
                .into_vec()
            }
        };
        if let Some(sender) = COMPLETIONS.get() {
            let _ = sender.send((operation, status, bytes));
        }
    }

    #[test]
    fn operations_settle_once_after_success_failure_cancel_or_panic() {
        let (sender, receiver) = mpsc::channel();
        assert!(COMPLETIONS.set(sender).is_ok());
        assert_eq!(init(completed), 0);
        let state = state();

        for (kind, status) in [("success", 0), ("failure", 1), ("cancel", 2), ("panic", 1)] {
            let (release, ready) = tokio::sync::oneshot::channel();
            let operation = spawn(Owner::default(), async move {
                let _ = ready.await;
                match kind {
                    "failure" => Err("fixture failure".into()),
                    "panic" => panic!("fixture operation panic"),
                    _ => Ok(Output::Empty),
                }
            });
            let done = state
                .registry
                .lock()
                .unwrap()
                .operations
                .get(&operation)
                .expect("the pending operation is registered")
                .done
                .clone();
            if kind == "cancel" {
                state.cancel(operation);
            } else {
                release.send(()).unwrap();
            }
            let (completed_operation, completed_status, bytes) = receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("the operation delivered its callback");
            assert_eq!(completed_operation, operation, "{kind}");
            assert_eq!(completed_status, status, "{kind}");
            match kind {
                "success" => assert_eq!(bytes, b"null"),
                "failure" => {
                    assert_eq!(
                        serde_json::from_slice::<Value>(&bytes).unwrap(),
                        json!({"message": "fixture failure"})
                    );
                }
                "cancel" => assert!(bytes.is_empty()),
                "panic" => {
                    assert_eq!(
                        serde_json::from_slice::<Value>(&bytes).unwrap(),
                        json!({"message": "iroh operation panicked"})
                    );
                }
                _ => unreachable!(),
            }
            state.runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(5), wait(vec![done]))
                    .await
                    .expect("closing the owner can finish");
            });
            assert_eq!(state.debug_count(4), 0, "{kind}");
            assert!(matches!(
                receiver.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
        }
    }
}
