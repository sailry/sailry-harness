use crate::{plugins::resources::mcp::Launch, process::Environment};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, OnceLock, Weak},
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::Mutex};

const DEADLINE: Duration = Duration::from_secs(10);
const MAX_BYTES: u64 = 64 * 1024;
const SESSION: &str = "AGENT_DESKTOP_SESSION";

#[derive(Default)]
pub(crate) struct Overlay {
    state: Mutex<State>,
    ingress: OnceLock<Weak<crate::store::Ingress>>,
}

#[derive(Default)]
struct State {
    homes: BTreeMap<PathBuf, Home>,
    allowed: BTreeMap<String, bool>,
    closed: bool,
}

struct Home {
    package: String,
    invocation: Invocation,
    session: Option<String>,
    collected: bool,
}

struct Invocation {
    command: PathBuf,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    environment: Environment,
}

#[derive(Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default)]
    data: Value,
    error: Option<Failure>,
}

#[derive(Deserialize)]
struct Failure {
    code: String,
}

impl Overlay {
    pub(crate) fn bind(&self, ingress: &Arc<crate::store::Ingress>) {
        let _ = self.ingress.set(Arc::downgrade(ingress));
    }

    pub(crate) async fn attach(
        &self,
        package: &str,
        enabled: bool,
        launch: &mut Launch,
        environment: &Environment,
    ) {
        let Launch::Stdio {
            command,
            cwd,
            env,
            computer: true,
            ..
        } = launch
        else {
            return;
        };
        if env.contains_key(SESSION) {
            return;
        }
        let Some(home) = env.get("AGENT_DESKTOP_HOME").map(PathBuf::from) else {
            return;
        };
        let invocation = Invocation {
            command: command.clone(),
            cwd: cwd.clone(),
            env: env.clone(),
            environment: environment.clone(),
        };
        if let Some(session) = self.ensure(package, enabled, home, invocation).await {
            env.entry(SESSION.into()).or_insert(session);
        }
    }

    async fn ensure(
        &self,
        package: &str,
        enabled: bool,
        path: PathBuf,
        invocation: Invocation,
    ) -> Option<String> {
        let operation = async {
            let mut state = self.state.lock().await;
            if state.closed {
                return None;
            }
            let current = match self.ingress.get() {
                Some(ingress) => match ingress.upgrade() {
                    Some(ingress) => Some(
                        ingress
                            .mcp_resources(Some(package.into()))
                            .await
                            .map(|resources| resources.allows_overlay(package))
                            .unwrap_or(false),
                    ),
                    None => Some(false),
                },
                None => None,
            };
            if !current.unwrap_or(enabled && state.allowed.get(package) != Some(&false)) {
                if let Some(home) = state.homes.get_mut(&path) {
                    home.end().await;
                }
                return None;
            }
            state.allowed.entry(package.into()).or_insert(true);
            let home = match state.homes.entry(path.clone()) {
                std::collections::btree_map::Entry::Occupied(entry) => {
                    let home = entry.into_mut();
                    home.invocation = invocation;
                    home
                }
                std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Home {
                    package: package.into(),
                    invocation,
                    session: None,
                    collected: false,
                }),
            };
            if let Some(session) = &home.session
                && active(&path, session).await
            {
                return Some(session.clone());
            }
            home.session = None;
            if !home.collected {
                home.collected = true;
                if let Err(code) = home.invocation.run(&["session", "gc", "--ended"]).await {
                    report("gc", Some(&code));
                }
            }
            match home
                .invocation
                .run(&[
                    "session",
                    "start",
                    "--cursor",
                    "--no-trace",
                    "--name",
                    "sailry",
                ])
                .await
                .and_then(session_id)
            {
                Ok(session) => {
                    home.session = Some(session.clone());
                    report("start", None);
                    Some(session)
                }
                Err(code) => {
                    report("start", Some(&code));
                    None
                }
            }
        };
        match tokio::time::timeout(DEADLINE, operation).await {
            Ok(session) => session,
            Err(_) => {
                report("start", Some("TIMEOUT"));
                None
            }
        }
    }

    pub(crate) async fn retain(&self, package: Option<&str>, allowed: &[String]) {
        let mut state = self.state.lock().await;
        if let Some(package) = package {
            state
                .allowed
                .insert(package.into(), allowed.iter().any(|name| name == package));
        } else {
            for enabled in state.allowed.values_mut() {
                *enabled = false;
            }
            for name in allowed {
                state.allowed.insert(name.clone(), true);
            }
        }
        for home in state.homes.values_mut() {
            if package.is_none_or(|name| name == home.package) && !allowed.contains(&home.package) {
                home.end().await;
            }
        }
    }

    pub(crate) async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        state.closed = true;
        for home in state.homes.values_mut() {
            home.end().await;
        }
    }
}

impl Home {
    async fn end(&mut self) {
        if let Some(session) = &self.session {
            match self.invocation.run(&["session", "end", session]).await {
                Ok(_) => {
                    self.session = None;
                    report("end", None);
                }
                Err(code) => report("end", Some(&code)),
            }
        }
    }
}

impl Invocation {
    async fn run(&self, args: &[&str]) -> Result<Value, String> {
        let mut command = tokio::process::Command::new(&self.command);
        self.environment.apply(&mut command);
        command
            .args(args)
            .envs(&self.env)
            .env_remove(SESSION)
            .current_dir(&self.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(target_os = "macos")]
        super::helper::disclaim(&mut command);
        let mut child = command.spawn().map_err(|_| "SPAWN".to_owned())?;
        let output = async {
            let mut bytes = Vec::new();
            child
                .stdout
                .take()
                .ok_or("OUTPUT")?
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "OUTPUT")?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err("OUTPUT_LIMIT".into());
            }
            let status = child.wait().await.map_err(|_| "EXIT")?;
            let envelope: Envelope =
                serde_json::from_slice(&bytes).map_err(|_| "INVALID_RESPONSE")?;
            if !status.success() || !envelope.ok {
                return Err(envelope
                    .error
                    .map(|error| error.code)
                    .filter(|code| {
                        !code.is_empty()
                            && code.len() <= 64
                            && code.bytes().all(|byte| {
                                byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                            })
                    })
                    .unwrap_or_else(|| "FAILED".into()));
            }
            Ok(envelope.data)
        };
        match tokio::time::timeout(DEADLINE, output).await {
            Ok(result) => result,
            Err(_) => Err("TIMEOUT".into()),
        }
    }
}

fn session_id(data: Value) -> Result<String, String> {
    data.get("session_id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .map(str::to_owned)
        .ok_or_else(|| "INVALID_SESSION".into())
}

async fn active(home: &Path, session: &str) -> bool {
    let Ok(file) =
        tokio::fs::File::open(home.join("sessions").join(session).join("session.json")).await
    else {
        return false;
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .is_err()
        || bytes.len() as u64 > MAX_BYTES
    {
        return false;
    }
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .is_some_and(|manifest| {
            manifest.get("id").and_then(Value::as_str) == Some(session)
                && manifest.get("ended_at").is_none_or(Value::is_null)
        })
}

pub(crate) fn enabled(values: &BTreeMap<String, Value>) -> bool {
    values
        .get("cursor_overlay")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn report(operation: &'static str, code: Option<&str>) {
    let mut entry = crate::logging::Entry::new(
        crate::logging::Area::Plugin,
        if code.is_some() {
            crate::logging::Phase::Failed
        } else {
            crate::logging::Phase::Completed
        },
    );
    entry.stage = Some("computer_overlay");
    entry.operation = Some(operation);
    entry.detail = code.map(str::to_owned);
    if code.is_some() {
        entry.emit_warn();
    } else {
        entry.emit();
    }
}

#[cfg(test)]
mod tests;
