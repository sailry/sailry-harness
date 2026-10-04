//! A view's request drafts use the existing Client and Node admission ledger.
pub(crate) mod activity;
mod changes;
pub(super) mod sdk;
mod settings;
pub(crate) mod usage;
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, Request, RequestId, plugin::Context};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const MAX_DRAFTS: usize = 16;
const MAX_COMMAND_BYTES: usize = 2 * 1024 * 1024;

pub(super) struct Host {
    changes: changes::Changes,
    conversation: sdk::conversation::Subscription,
    contributions: Option<Arc<super::contributions::bridge::Bridge>>,
    files: super::files::Files,
    client: Arc<Client>,
    context: Context,
    runtime: tokio::runtime::Handle,
    stop: CancellationToken,
    requests: Mutex<BTreeMap<RequestId, Request>>,
    settings: Mutex<settings::Settings>,
    capacity: Arc<tokio::sync::Semaphore>,
}

impl Host {
    pub(super) fn prepared(&self, id: RequestId) -> Result<Request, HostError> {
        self.check()?;
        self.requests
            .lock()
            .map_err(lock_error)?
            .get(&id)
            .cloned()
            .ok_or_else(|| HostError::new("plugin request draft is unavailable"))
    }

    pub(super) fn context(&self) -> Context {
        let mut context = self.context.clone();
        if let Ok(settings) = self.settings.lock() {
            context.package = settings.reference();
        }
        context
    }

    pub(super) fn target(&self) -> sailry_protocol::NodeId {
        self.client.target()
    }

    pub(super) fn new(
        client: Arc<Client>,
        context: Context,
        runtime: tokio::runtime::Handle,
        read_files: bool,
        contributions: Option<Arc<super::contributions::bridge::Bridge>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            changes: Default::default(),
            conversation: Default::default(),
            contributions,
            files: super::files::Files::new(read_files),
            client,
            settings: Mutex::new(settings::Settings::new(&context)),
            context,
            runtime,
            stop: CancellationToken::new(),
            requests: Mutex::new(BTreeMap::new()),
            capacity: Arc::new(tokio::sync::Semaphore::new(4)),
        })
    }

    pub(super) fn stop_token(&self) -> CancellationToken {
        self.stop.clone()
    }

    pub(super) fn close(&self) {
        self.stop.cancel();
    }

    pub(super) fn module(self: &Arc<Self>) -> HostModule {
        let scope = self.clone();
        let prepare = self.clone();
        let execute = self.clone();
        let outcome = self.clone();
        let forget = self.clone();
        let files = self.clone();
        HostModule::new("sailry")
            .function("context", move |_| {
                scope.check()?;
                let context = scope.context();
                let mut value = serde_json::json!({
                    "surface": context.surface,
                    "api_version": sailry_protocol::plugin::API_VERSION,
                    "node": scope.client.target(),
                    "package": context.package,
                    "worktree": context.worktree,
                    "session": context.session,
                    "locale": rust_i18n::locale().to_string(),
                });
                value["package"]["settings_revision"] = serde_json::json!(context.package.settings_revision.to_string());
                Ok(HostValue::from(value.to_string()))
            })
            .function("prepare", move |arguments| {
                prepare.check()?;
                let value = arguments.string(0)?;
                if value.len() > MAX_COMMAND_BYTES {
                    return Err(HostError::new("plugin command exceeds the size limit"));
                }
                let command = serde_json::from_str(value)
                    .map_err(|_| HostError::new("invalid plugin command"))?;
                prepare.prepare_public(sdk::values::command(command)?)
            })
            .async_function("execute", move |arguments| {
                execute.check()?;
                let id: RequestId = arguments.string(0)?.parse().map_err(HostError::new)?;
                let request = execute.requests.lock().map_err(lock_error)?
                    .get(&id).cloned().ok_or_else(|| HostError::new("plugin request draft is unavailable"))?;
                let permit = execute.capacity.clone().try_acquire_owned()
                    .map_err(|_| HostError::new("plugin request capacity exhausted"))?;
                let owner = execute.clone();
                Ok(async move {
                    let runtime = owner.runtime.clone();
                    let task = runtime.spawn(async move {
                        let _permit = permit;
                        let result = tokio::select! {
                            biased;
                            _ = owner.stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                            result = owner.client.execute(request) => result,
                        };
                        owner.finish_settings(id, &result).await?;
                        let value = match result {
                            Ok(output) => serde_json::json!({"Ok":sdk::public_output(output)?}),
                            Err(fault) => serde_json::json!({"Err":fault}),
                        };
                        Ok(HostValue::from(value.to_string()))
                    });
                    task.await.map_err(|_| HostError::new("plugin request worker failed"))?
                })
            })
            .function("forget", move |arguments| {
                let id: RequestId = arguments.string(0)?.parse().map_err(HostError::new)?;
                forget.settings_forgettable(id)?;
                forget.requests.lock().map_err(lock_error)?.remove(&id);
                Ok(HostValue::Null)
            })
            .async_function("outcome", move |arguments| {
                outcome.check()?;
                let id: RequestId = arguments.string(0)?.parse().map_err(HostError::new)?;
                let request = outcome.requests.lock().map_err(lock_error)?.get(&id).cloned()
                    .ok_or_else(|| HostError::new("plugin request draft is unavailable"))?;
                let permit = outcome.capacity.clone().try_acquire_owned()
                    .map_err(|_| HostError::new("plugin request capacity exhausted"))?;
                let owner = outcome.clone();
                Ok(async move {
                    let runtime = owner.runtime.clone();
                    runtime.spawn(async move {
                        let _permit = permit;
                        let result = tokio::select! {
                            biased;
                            _ = owner.stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                            result = owner.client.outcome(&request) => result,
                        };
                        if let Ok(sailry_protocol::RequestOutcome::Completed(completed)) = &result {
                            owner.finish_settings(id, completed).await?;
                        }
                        let value = match result {
                            Ok(sailry_protocol::RequestOutcome::Completed(completed)) => {
                                let output = match *completed {
                                    Ok(output) => serde_json::json!({"Ok":sdk::public_output(output)?}),
                                    Err(fault) => serde_json::json!({"Err":fault}),
                                };
                                serde_json::json!({"Ok":{"kind":"completed","data":output}})
                            },
                            result => serde_json::to_value(result).map_err(|_| HostError::new("plugin outcome could not be encoded"))?,
                        };
                        Ok(HostValue::from(value.to_string()))
                    }).await.map_err(|_| HostError::new("plugin outcome worker failed"))?
                })
            })
            .async_function("next_change", move |arguments| {
                files.check()?;
                files.files.next(arguments.string(0)?.into(), files.client.clone(), files.context.worktree
                    .ok_or_else(|| HostError::new("plugin has no worktree scope"))?,
                    files.runtime.clone(), files.stop.clone())
            })
            .declarations(r#"
export function theme(): { is_dark: boolean; colors: Record<string, string> };
                export const Header: { new(id: string, props: { content: string }): import("gpui-kit").Element };
                export function header_action(): Promise<string>;
                export const Image: { new(id: string, props: { path: string, circular?: boolean }): import("gpui-kit").Element };
                export const Terminal: { new(id: string, props: { terminal: string | null }): import("gpui-kit").Element };
                export const BrowserSurface: { new(id: string): import("gpui-kit").Element };
                export const Conversation: { new(id: string, props?: { assistant: string, resource?: { kind: "database" | "ssh", id: string }, heading?: string }): import("gpui-kit").Element };
                export function nextAssistantTools(cursor: string): Promise<{cursor: string; conversations: Record<string, {connected: boolean; session?: string; revision?: string; latest?: string; calls: {key: string; name: string; arguments: unknown; state: string; timestamp_ms: number; result: unknown; sequence: string | null}[]}>}>;
                export function context(): string;
                export function prepare(command: string): string;
                export function execute(request: string): Promise<string>;
                export function forget(request: string): void;
                export function outcome(request: string): Promise<string>;
                export function next_change(seen: string): Promise<string>;
            "#)
    }

    pub(super) fn check(&self) -> Result<(), HostError> {
        if self.stop.is_cancelled() {
            Err(HostError::new("plugin view is closed"))
        } else {
            Ok(())
        }
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> HostError {
    HostError::new("plugin request drafts are unavailable")
}
