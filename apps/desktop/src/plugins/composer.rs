//! Captured composer options reuse the conversation's draft and configuration owner.
use super::host::{
    Host,
    sdk::values::{decode, encode},
};
use crate::conversation::live::View;
use gpui_kit::*;
use gpui_shell::{HostError, HostModule};
use serde_json::Value;
use std::{cell::RefCell, sync::Arc};

pub(super) struct Scope {
    pub source: Option<WeakEntity<View>>,
    pub read: bool,
    pub control: bool,
}

pub(super) struct State {
    source: WeakEntity<View>,
    node: sailry_protocol::NodeId,
    changes: tokio::sync::watch::Sender<Value>,
    cursor: u64,
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
        next["cursor"] = self.cursor.to_string().into();
        if *self.changes.borrow() != next {
            self.cursor += 1;
            next["cursor"] = self.cursor.to_string().into();
            self.changes.send_replace(next);
        }
    }

    fn matches(&self, host: &Host, cx: &App) -> bool {
        self.source.upgrade().is_some_and(|source| {
            let source = source.read(cx);
            let binding = source.binding();
            self.node == binding.client.target()
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
                State {
                    node: source.read(cx).binding().client.target(),
                    source: source.downgrade(),
                    changes: tokio::sync::watch::channel(snapshot).0,
                    cursor: 0,
                    _observer: observer,
                }
            })
        });
    let read = state.clone().filter(|_| scope.read);
    let change = read.clone();
    let write = state.clone().filter(|_| scope.control);
    let read_host = host.clone();
    let change_host = host.clone();
    let declarations = format!(
        "{}\nexport interface ComposerState {{cursor:string;command:{{name:string;arguments:string;package:import('sailry/sdk').Json}}|null;mode:'plan'|'code';session:string|null;connected:boolean;busy:boolean;readonly:boolean;statistics:Record<string,unknown>|null;context_limit:number|null;compacting:boolean;has_messages:boolean;sidebar:boolean}}\nexport function readComposer():ComposerState;\nexport function nextComposerChange(cursor:string):Promise<ComposerState>;\nexport function setComposerOption(cursor:string,key:'command'|'mode',value:unknown):Promise<ComposerState>;",
        module.declared().unwrap_or_default()
    );
    let module=module.function("readComposer",move |_| {
        read_host.check()?;
        let state=read.as_ref().ok_or_else(||HostError::new("composer read is unavailable"))?;
        gpui_shell::with_current_app(|cx|state.update(cx,|state,cx| {
            if !state.matches(&read_host,cx) {return Err(HostError::new("composer scope is unavailable"));}
            state.refresh(cx); encode(state.changes.borrow().clone())
        })).ok_or_else(||HostError::new("composer requires an active view"))?
    }).async_function("nextComposerChange",move |args| {
        change_host.check()?;let seen=args.string(0)?.to_owned();
        let state=change.as_ref().ok_or_else(||HostError::new("composer read is unavailable"))?;
        let mut changes=gpui_shell::with_current_app(|cx|state.read(cx).changes.subscribe()).ok_or_else(||HostError::new("composer requires an active view"))?;
        let stop=change_host.stop_token();
        Ok(async move { loop {
            let state=changes.borrow_and_update().clone();
            if state["cursor"].as_str()!=Some(&seen) {return encode(state);}
            tokio::select! {biased; _=stop.cancelled()=>return Err(HostError::new("plugin view is closed")), changed=changes.changed()=>changed.map_err(|_|HostError::new("composer is closed"))?}
        }})
    }).async_function("setComposerOption",move |args| {
        host.check()?;let state=write.clone().ok_or_else(||HostError::new("composer control is unavailable"))?;
        let cursor=args.string(0)?.to_owned();let key=args.string(1)?.to_owned();let value=decode(args.value(2)?)?;
        let (reply,receive)=tokio::sync::oneshot::channel();let host=host.clone();
        gpui_shell::with_current_app(|cx|cx.defer(move |cx|state.update(cx,|_,cx|cx.emit(Request {cursor,key,value,host,reply:RefCell::new(Some(reply))}))))
            .ok_or_else(||HostError::new("composer requires an active view"))?;
        Ok(async move {receive.await.map_err(|_|HostError::new("composer is closed"))?.map_err(HostError::new).and_then(encode)})
    }).declarations(declarations);
    (module, state)
}
