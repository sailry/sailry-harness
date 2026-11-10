use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

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
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => None,
            result = future => Some(result),
        };
        state.complete(operation, result);
        done.cancel();
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
