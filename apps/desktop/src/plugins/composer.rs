//! Captured composer options reuse the conversation's draft and configuration owner.
use super::host::{
    Host,
    sdk::values::{decode, encode},
};
use crate::conversation::live::View;
use gpui_kit::*;
use gpui_shell::{HostArguments, HostError, HostModule, HostResult};
use serde_json::Value;
use std::{cell::RefCell, sync::Arc};

pub(super) struct Scope {
    pub source: Option<WeakEntity<View>>,
    pub read: bool,
    pub model: bool,
    pub control: bool,
}

pub(super) struct State {
    source: WeakEntity<View>,
    node: sailry_protocol::NodeId,
    package: sailry_protocol::plugin::Reference,
    changes: tokio::sync::watch::Sender<Value>,
    cursor: u64,
    model_changes: tokio::sync::watch::Sender<Value>,
    model_cursor: u64,
    session: Option<sailry_protocol::SessionId>,
    worktree: Option<sailry_protocol::WorktreeId>,
    _observer: Subscription,
}

struct Request {
    cursor: String,
    key: String,
    value: Value,
    host: Arc<Host>,
    reply: RefCell<Option<tokio::sync::oneshot::Sender<Result<Value, String>>>>,
}
impl EventEmitter<Request> for State {}

impl State {
    fn refresh(&mut self, cx: &App) {
        let Some(source) = self.source.upgrade() else {
            return;
        };
        let mut next = source.read(cx).composer_options(cx);
        next["options"] = source.read(cx).package_options(&self.package, cx);
        publish(&self.changes, &mut self.cursor, next);
        let source = source.read(cx);
        let mut model = source.model_context(&self.package, cx);
        if self.node != source.binding().client.target()
            || self.session != source.session()
            || self.worktree != source.binding().worktree
        {
            model = serde_json::json!({"connected":false,"provider":null});
        }
        publish(&self.model_changes, &mut self.model_cursor, model);
    }

    fn matches(&self, host: &Host, cx: &App) -> bool {
        self.source.upgrade().is_some_and(|source| {
            let source = source.read(cx);
            let binding = source.binding();
            self.package == host.context().package
                && self.node == binding.client.target()
                && host.context().worktree == binding.worktree
                && host.context().session == source.session()
        })
    }
}

pub(super) fn extend(
    module: HostModule,
    scope: Scope,
    host: Arc<Host>,
    window: &mut Window,
    cx: &mut App,
) -> (HostModule, Option<Entity<State>>) {
    let state = scope
        .source
        .and_then(|source| source.upgrade())
        .map(|source| {
            cx.new(|cx| {
                let observer = cx.observe(&source, |state: &mut State, _, cx| state.refresh(cx));
                cx.subscribe_in(
                    &cx.entity(),
                    window,
                    |state: &mut State, _, request: &Request, window, cx| {
                        let Some(reply) = request.reply.borrow_mut().take() else {
                            return;
                        };
                        state.refresh(cx);
                        let result =
                            if request.host.check().is_err() || !state.matches(&request.host, cx) {
                                Err("composer scope is unavailable".into())
                            } else if request.cursor != state.cursor.to_string() {
                                Err("composer options changed".into())
                            } else {
                                state
                                    .source
                                    .update(cx, |source, cx| {
                                        source.set_composer_option(
                                            &request.host.context().package,
                                            &request.key,
                                            request.value.clone(),
                                            window,
                                            cx,
                                        )
                                    })
                                    .map_err(|_| "composer is closed".to_owned())
                                    .and_then(|result| result)
                            };
                        state.refresh(cx);
                        let _ = reply.send(result.map(|_| state.changes.borrow().clone()));
                    },
                )
                .detach();
                let mut snapshot = source.read(cx).composer_options(cx);
                snapshot["cursor"] = "0".into();
                snapshot["options"] = source.read(cx).package_options(&host.context().package, cx);
                let mut model = source.read(cx).model_context(&host.context().package, cx);
                model["cursor"] = "0".into();
                State {
                    model_changes: tokio::sync::watch::channel(model).0,
                    model_cursor: 0,
                    session: host.context().session,
                    worktree: host.context().worktree,
                    node: source.read(cx).binding().client.target(),
                    package: host.context().package.clone(),
                    source: source.downgrade(),
                    changes: tokio::sync::watch::channel(snapshot).0,
                    cursor: 0,
                    _observer: observer,
                }
            })
        });
    let model_read = state.clone().filter(|_| scope.model);
    let model_change = model_read.clone();
    let read = state.clone().filter(|_| scope.read);
    let change = read.clone();
    let reveal = read.clone();
    let reveal_host = host.clone();
    let write = state.clone().filter(|_| scope.control);
    let read_host = host.clone();
    let change_host = host.clone();
    let model_host = host.clone();
    let model_change_host = host.clone();
    let declarations = format!(
        "{}\nexport interface ComposerState {{cursor:string;command:{{name:string;arguments:string;package:import('sailry/sdk').Json}}|null;mode:'plan'|'code';session:string|null;connected:boolean;busy:boolean;configuring:boolean;model_tools:boolean;operations:string[];options:Record<string,unknown>;readonly:boolean;statistics:Record<string,unknown>|null;context_limit:number|null;compacting:boolean;has_messages:boolean;sidebar:boolean}}\nexport function revealComposerEntry(target:{{turn:string;turn_sequence:string;entry:string}}):void;\nexport function readComposer():ComposerState;\nexport function nextComposerChange(cursor:string):Promise<ComposerState>;\nexport function setComposerOption(cursor:string,key:'command'|'mode'|'plugin',value:unknown):Promise<ComposerState>;",
        module.declared().unwrap_or_default()
    );
    let declarations = format!(
        "{declarations}\nexport interface ModelContext {{cursor:string;connected:boolean;provider:{{id:string;revision:string;model:string;authentication:'api_key'|'chat_gpt'|'chat_gpt_oauth'|'claude_oauth'|'copilot'|'plugin';preset:{{source:import('sailry/sdk').Json;id:string;route:import('sailry/sdk').Json|null}}|null}}|null}}\nexport function readModelContext():ModelContext;\nexport function nextModelContext(cursor:string):Promise<ModelContext>;"
    );
    let module = module
        .function("readModelContext", move |_| {
            read_state(&model_host, &model_read, Projection::Model)
        })
        .async_function("nextModelContext", move |args| {
            changes(args, &model_change_host, &model_change, Projection::Model)
        })
        .function("revealComposerEntry", move |args| {
            reveal_entry(args, &reveal_host, &reveal)
        })
        .function("readComposer", move |_| {
            read_state(&read_host, &read, Projection::Composer)
        })
        .async_function("nextComposerChange", move |args| {
            changes(args, &change_host, &change, Projection::Composer)
        })
        .async_function("setComposerOption", move |args| {
            set_option(args, &host, &write)
        })
        .declarations(declarations);
    (module, state)
}

#[derive(Clone, Copy)]
enum Projection {
    Composer,
    Model,
}

impl Projection {
    fn changes(self, state: &State) -> &tokio::sync::watch::Sender<Value> {
        match self {
            Self::Composer => &state.changes,
            Self::Model => &state.model_changes,
        }
    }
}

fn publish(changes: &tokio::sync::watch::Sender<Value>, cursor: &mut u64, mut next: Value) {
    next["cursor"] = cursor.to_string().into();
    if *changes.borrow() != next {
        *cursor += 1;
        next["cursor"] = cursor.to_string().into();
        changes.send_replace(next);
    }
}

fn read_state(
    read_host: &Arc<Host>,
    read: &Option<Entity<State>>,
    projection: Projection,
) -> HostResult {
    read_host.check()?;
    let state = read
        .as_ref()
        .ok_or_else(|| HostError::new("composer read is unavailable"))?;
    gpui_shell::with_current_app(|cx| {
        state.update(cx, |state, cx| {
            if !state.matches(read_host, cx) {
                return Err(HostError::new("composer scope is unavailable"));
            }
            state.refresh(cx);
            encode(projection.changes(state).borrow().clone())
        })
    })
    .ok_or_else(|| HostError::new("composer requires an active view"))?
}

fn reveal_entry(
    args: &HostArguments,
    host: &Arc<Host>,
    read: &Option<Entity<State>>,
) -> HostResult {
    host.check()?;
    let target = crate::conversation::live::search::Target::decode(decode(args.value(0)?)?)
        .map_err(HostError::new)?;
    let state = read
        .as_ref()
        .ok_or_else(|| HostError::new("composer read is unavailable"))?;
    gpui_shell::with_current_app(|cx| {
        state.update(cx, |state, cx| {
            if !state.matches(host, cx) || state.session.is_none() {
                return Err(HostError::new("composer scope is unavailable"));
            }
            state
                .source
                .update(cx, |source, cx| {
                    if !source.connected() {
                        return Err(HostError::new("composer is disconnected"));
                    }
                    source.reveal_entry(target, cx);
                    encode(Value::Null)
                })
                .map_err(|_| HostError::new("composer is closed"))?
        })
    })
    .ok_or_else(|| HostError::new("composer requires an active view"))?
}

fn changes(
    args: &HostArguments,
    change_host: &Arc<Host>,
    change: &Option<Entity<State>>,
    projection: Projection,
) -> Result<impl std::future::Future<Output = HostResult> + Send + 'static + use<>, HostError> {
    change_host.check()?;
    let seen = args.string(0)?.to_owned();
    let state = change
        .as_ref()
        .ok_or_else(|| HostError::new("composer read is unavailable"))?;
    let mut changes =
        gpui_shell::with_current_app(|cx| projection.changes(state.read(cx)).subscribe())
            .ok_or_else(|| HostError::new("composer requires an active view"))?;
    let stop = change_host.stop_token();
    Ok(async move {
        loop {
            let state = changes.borrow_and_update().clone();
            if state["cursor"].as_str() != Some(&seen) {
                return encode(state);
            }
            tokio::select! {
                biased;
                _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                changed = changes.changed() => changed.map_err(|_| HostError::new("composer is closed"))?,
            }
        }
    })
}

fn set_option(
    args: &HostArguments,
    host: &Arc<Host>,
    write: &Option<Entity<State>>,
) -> Result<impl std::future::Future<Output = HostResult> + Send + 'static + use<>, HostError> {
    host.check()?;
    let state = write
        .clone()
        .ok_or_else(|| HostError::new("composer control is unavailable"))?;
    let cursor = args.string(0)?.to_owned();
    let key = args.string(1)?.to_owned();
    let value = decode(args.value(2)?)?;
    let (reply, receive) = tokio::sync::oneshot::channel();
    let host = host.clone();
    gpui_shell::with_current_app(|cx| {
        cx.defer(move |cx| {
            state.update(cx, |_, cx| {
                cx.emit(Request {
                    cursor,
                    key,
                    value,
                    host,
                    reply: RefCell::new(Some(reply)),
                })
            })
        })
    })
    .ok_or_else(|| HostError::new("composer requires an active view"))?;
    Ok(async move {
        receive
            .await
            .map_err(|_| HostError::new("composer is closed"))?
            .map_err(HostError::new)
            .and_then(encode)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use serde_json::json;

    #[test]
    fn model_changes_ignore_unrelated_composer_updates() {
        let initial = json!({"cursor":"0","connected":true,"provider":null});
        let model = tokio::sync::watch::channel(initial.clone()).0;
        let composer = tokio::sync::watch::channel(json!({"cursor":"0","busy":false})).0;
        let (mut model_cursor, mut composer_cursor) = (0, 0);
        publish(&composer, &mut composer_cursor, json!({"busy":true}));
        publish(
            &model,
            &mut model_cursor,
            json!({"connected":true,"provider":null}),
        );
        assert_eq!(model_cursor, 0);
        assert_eq!(*model.borrow(), initial);
        publish(
            &model,
            &mut model_cursor,
            json!({"connected":true,"provider":{"id":"channel","revision":"2","model":"selected","authentication":"chat_gpt_oauth","preset":null}}),
        );
        assert_eq!(model_cursor, 1);
        assert!(model.borrow().get("busy").is_none());
        publish(
            &model,
            &mut model_cursor,
            json!({"connected":false,"provider":null}),
        );
        assert_eq!(model_cursor, 2);
        assert!(model.borrow()["provider"].is_null());
        assert_eq!(composer_cursor, 1);
    }
}
