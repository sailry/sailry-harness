use super::{codec::Codec, resolve::Plan};
use futures::{SinkExt, StreamExt};
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, code::Status};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    sync::{Mutex, mpsc, oneshot, watch},
};
use tokio_util::codec::{FramedRead, FramedWrite};

pub(super) struct State {
    pub status: Status,
    pub progress: Option<String>,
    pub diagnostics: BTreeMap<String, Diagnostics>,
    pub generation: u64,
    pub diagnostics_revision: u64,
    pub stderr: Vec<u8>,
}

#[derive(Clone, PartialEq)]
pub(super) struct Diagnostics {
    pub version: Option<i32>,
    pub items: Vec<lsp_types::Diagnostic>,
}

enum Send {
    Request {
        id: u64,
        method: String,
        params: Value,
        reply: oneshot::Sender<Result<Value, Fault>>,
    },
    Notify {
        method: String,
        params: Value,
    },
    Cancel(u64),
}

pub(super) struct Rpc {
    sender: mpsc::Sender<Send>,
    next: AtomicU64,
    pub state: Arc<Mutex<State>>,
    pub changed: watch::Receiver<u64>,
    stop: CancellationToken,
    done: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Rpc {
    pub fn spawn(
        plan: &Plan,
        environment: &crate::process::Environment,
        closed: CancellationToken,
    ) -> Result<Self, Fault> {
        if plan.dependency.canonicalize().map_err(io)? != plan.dependency {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "language dependency changed after startup approval",
            ));
        }
        let sandbox = crate::process::sandbox::Sandbox::prepare(
            &crate::process::Launch {
                root: plan.root.clone(),
                profile: plan.profile.clone(),
                command: plan.startup.executable.clone(),
                access: sailry_protocol::process::Access::Workspace,
                cwd: plan.startup.root.clone(),
                timeout_ms: plan.timeout.as_millis() as u64,
                inputs: None,
                scratch: None,
            },
            environment,
            Some(&plan.dependency),
        )?;
        let mut command = tokio::process::Command::new(&plan.executable);
        environment.apply(&mut command);
        command
            .args(&plan.args)
            .envs(&plan.environment)
            .current_dir(&plan.directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(sandbox) = &sandbox {
            command = sandbox.command(command)?;
            // Language framing needs stdin after the shared wrapper's shell defaults.
            command.stdin(Stdio::piped());
        }
        let mut command = CommandWrap::from(command);
        command.wrap(KillOnDrop);
        #[cfg(unix)]
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(process_wrap::tokio::JobObject);
        let mut child = Child {
            inner: command.spawn().map_err(io)?,
            armed: true,
            _sandbox: sandbox,
        };
        let input = child
            .inner
            .stdin()
            .take()
            .ok_or_else(|| io("language stdin is unavailable"))?;
        let output = child
            .inner
            .stdout()
            .take()
            .ok_or_else(|| io("language stdout is unavailable"))?;
        let mut errors = child
            .inner
            .stderr()
            .take()
            .ok_or_else(|| io("language stderr is unavailable"))?;
        let (sender, receiver) = mpsc::channel(64);
        let (changed, updates) = watch::channel(0);
        let state = Arc::new(Mutex::new(State {
            status: Status::Starting,
            progress: None,
            diagnostics: BTreeMap::new(),
            generation: 0,
            diagnostics_revision: 0,
            stderr: Vec::new(),
        }));
        let stop = CancellationToken::new();
        let diagnostics = state.clone();
        let stderr = tokio::spawn(async move {
            let mut buffer = [0; 4096];
            while let Ok(count) = errors.read(&mut buffer).await {
                if count == 0 {
                    break;
                }
                let mut state = diagnostics.lock().await;
                state.stderr.extend_from_slice(&buffer[..count]);
                if state.stderr.len() > 64 * 1024 {
                    let excess = state.stderr.len() - 64 * 1024;
                    state.stderr.drain(..excess);
                }
            }
        });
        let done = tokio::spawn(run(
            child,
            FramedWrite::new(input, Codec::default()),
            FramedRead::new(output, Codec::default()),
            receiver,
            Control {
                state: state.clone(),
                changed,
                config: plan.config.clone(),
                stop: stop.clone(),
                closed,
                stderr,
            },
        ));
        Ok(Self {
            sender,
            next: AtomicU64::new(1),
            state,
            changed: updates,
            stop,
            done: Mutex::new(Some(done)),
        })
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<Value, Fault> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (reply, receive) = oneshot::channel();
        let _cancel = Cancel {
            sender: self.sender.clone(),
            id,
        };
        let work = async {
            self.sender
                .send(Send::Request {
                    id,
                    method: method.into(),
                    params,
                    reply,
                })
                .await
                .map_err(|_| io("language server has stopped"))?;
            receive
                .await
                .map_err(|_| io("language server has stopped"))?
        };
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "code query was cancelled")),
            _ = self.stop.cancelled() => Err(io("language server has stopped")),
            result = tokio::time::timeout(timeout, work) => result.map_err(|_| Fault::new(ErrorCode::Unavailable, "language query timed out"))?,
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), Fault> {
        self.sender
            .send(Send::Notify {
                method: method.into(),
                params,
            })
            .await
            .map_err(|_| io("language server has stopped"))
    }

    pub async fn shutdown(&self) {
        let _ = self
            .request(
                "shutdown",
                Value::Null,
                Duration::from_secs(1),
                &CancellationToken::new(),
            )
            .await;
        let _ = self.notify("exit", Value::Null).await;
        self.stop.cancel();
        if let Some(mut done) = self.done.lock().await.take()
            && tokio::time::timeout(Duration::from_secs(2), &mut done)
                .await
                .is_err()
        {
            done.abort();
            let _ = done.await;
        }
    }
}

impl Drop for Rpc {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

struct Cancel {
    sender: mpsc::Sender<Send>,
    id: u64,
}
impl Drop for Cancel {
    fn drop(&mut self) {
        let _ = self.sender.try_send(Send::Cancel(self.id));
    }
}

struct Child {
    inner: Box<dyn ChildWrapper>,
    armed: bool,
    _sandbox: Option<crate::process::sandbox::Sandbox>,
}
impl Child {
    async fn wait_root(&mut self) -> std::io::Result<std::process::ExitStatus> {
        // SAFETY: ProcessGroup/JobObject exclusively owns the native child; only
        // Tokio's root wait is used so descendants cannot keep cleanup waiting.
        unsafe { self.inner.try_inner_child_mut() }
            .expect("language child is native")
            .wait()
            .await
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.inner.start_kill();
        }
    }
}

struct Control {
    state: Arc<Mutex<State>>,
    changed: watch::Sender<u64>,
    config: Value,
    stop: CancellationToken,
    closed: CancellationToken,
    stderr: tokio::task::JoinHandle<()>,
}

async fn run(
    mut child: Child,
    mut input: FramedWrite<tokio::process::ChildStdin, Codec>,
    mut output: FramedRead<tokio::process::ChildStdout, Codec>,
    mut receive: mpsc::Receiver<Send>,
    control: Control,
) {
    let Control {
        state,
        changed,
        config,
        stop,
        closed,
        stderr,
    } = control;
    let mut pending = BTreeMap::<u64, oneshot::Sender<Result<Value, Fault>>>::new();
    let mut progress = BTreeSet::new();
    let failure = loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => break None,
            _ = closed.cancelled() => break None,
            result = child.wait_root() => break Some(format!("language server exited: {}", result.map(|status| status.to_string()).unwrap_or_else(|error| error.to_string()))),
            message = receive.recv() => {
                let message = match message {
                    Some(Send::Request { id, method, params, reply }) => { if pending.len()>=128 {let _=reply.send(Err(Fault::new(ErrorCode::Busy,"language request capacity is exhausted")));continue;} pending.insert(id, reply); json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}) },
                    Some(Send::Notify { method, params }) => json!({"jsonrpc":"2.0","method":method,"params":params}),
                    Some(Send::Cancel(id)) => { if pending.remove(&id).is_none() { continue; } json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":id}}) },
                    None => break None,
                };
                if let Err(error) = emit(&mut input,message,&stop,&closed).await { break Some(error.to_string()); }
            }
            message = output.next() => {
                let message = match message { Some(Ok(message)) => message, Some(Err(error)) => break Some(error.to_string()), None => break Some("language server closed its output".into()) };
                if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") { break Some("invalid language JSON-RPC version".into()); }
                if let Some(method) = message.get("method").and_then(Value::as_str) {
                    let params = message.get("params").unwrap_or(&Value::Null);
                    if let Some(id) = message.get("id") {
                        let response = match method {
                            "workspace/configuration" => json!({"jsonrpc":"2.0","id":id,"result":params.get("items").and_then(Value::as_array).map(|items| items.iter().map(|item| configuration(&config, item.get("section").and_then(Value::as_str))).collect::<Vec<_>>()).unwrap_or_default()}),
                            "workspace/applyEdit" => json!({"jsonrpc":"2.0","id":id,"result":{"applied":false,"failureReason":"language services cannot modify project files"}}),
                            "window/workDoneProgress/create" | "client/registerCapability" | "client/unregisterCapability" | "window/showMessageRequest" => json!({"jsonrpc":"2.0","id":id,"result":null}),
                            _ => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"unsupported language server request"}}),
                        };
                        if let Err(error) = emit(&mut input,response,&stop,&closed).await { break Some(error.to_string()); }
                    } else {
                        let mut state = state.lock().await;
                        if method == "textDocument/publishDiagnostics" {
                            if let (Some(uri), Some(items)) = (params.get("uri").and_then(Value::as_str), params.get("diagnostics"))
                                && let Ok(items) = serde_json::from_value::<Vec<lsp_types::Diagnostic>>(items.clone())
                            {
                                    // Publication may contain arbitrary URIs; reads apply confinement before exposing them.
                                    if state.diagnostics.len() < 50 || state.diagnostics.contains_key(uri) {
                                        let published=Diagnostics { version: params.get("version").and_then(Value::as_i64).and_then(|value| i32::try_from(value).ok()), items: items.into_iter().take(101).map(bounded_diagnostic).collect() };
                                        if state.diagnostics.get(uri)!=Some(&published) {state.diagnostics_revision=state.diagnostics_revision.wrapping_add(1);state.diagnostics.insert(uri.into(),published);}
                                    }
                            }
                        } else if method == "$/progress" {
                            let token = params.get("token").map(Value::to_string).unwrap_or_default();
                            match params.pointer("/value/kind").and_then(Value::as_str) {
                                Some("begin") => { if progress.len() < 64 { progress.insert(token); } },
                                Some("end") => { progress.remove(&token); },
                                _ => {}
                            }
                            state.progress = params.pointer("/value/message").or_else(|| params.pointer("/value/title")).and_then(Value::as_str).map(|value| value.chars().take(256).collect());
                            state.status = if progress.is_empty() { Status::Ready } else { Status::Indexing };
                        }
                        state.generation = state.generation.wrapping_add(1);
                        let _ = changed.send(state.generation);
                    }
                } else if let Some(id) = message.get("id").and_then(Value::as_u64) && let Some(reply) = pending.remove(&id) {
                    let result = if message.get("error").is_some() { Err(io("language server rejected the query")) } else { Ok(message.get("result").cloned().unwrap_or(Value::Null)) };
                    let _ = reply.send(result);
                }
            }
        }
    };
    for (_, reply) in pending {
        let _ = reply.send(Err(io("language server has stopped")));
    }
    let _ = child.inner.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(1), child.wait_root()).await;
    child.armed = false;
    stderr.abort();
    let _ = stderr.await;
    let mut state = state.lock().await;
    state.status = if failure.is_some() {
        Status::Failed
    } else {
        Status::Stopped
    };
    state.progress = None;
    state.generation = state.generation.wrapping_add(1);
    let _ = changed.send(state.generation);
}

async fn emit(
    input: &mut FramedWrite<tokio::process::ChildStdin, Codec>,
    message: Value,
    stop: &CancellationToken,
    closed: &CancellationToken,
) -> Result<(), std::io::Error> {
    tokio::select! {biased;_ = stop.cancelled()=>Err(std::io::Error::new(std::io::ErrorKind::Interrupted,"language server was stopped")),_ = closed.cancelled()=>Err(std::io::Error::new(std::io::ErrorKind::Interrupted,"Node was stopped")),result=input.send(message)=>result}
}

pub(super) fn bounded_diagnostic(mut item: lsp_types::Diagnostic) -> lsp_types::Diagnostic {
    let mut end = item.message.len().min(8192);
    while !item.message.is_char_boundary(end) {
        end -= 1;
    }
    item.message.truncate(end);
    item.related_information = None;
    item.data = None;
    item.code_description = None;
    item
}

fn configuration(config: &Value, section: Option<&str>) -> Value {
    let Some(section) = section.filter(|value| !value.is_empty()) else {
        return config.clone();
    };
    if let Some(value) = config.get(section) {
        return value.clone();
    }
    section
        .split('.')
        .try_fold(config, |value, key| value.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}

fn io(error: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Internal, error.to_string())
}

#[cfg(test)]
mod tests;
