//! Core navigation supplies captured targets; the declared renderer decides how to display them.
use super::{Panel, host::Host};
use gpui_kit::*;
use gpui_shell::{HostError, HostModule};
use sailry_protocol::{NodeId, WorktreeId, plugin::desktop::ResourceKind};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) struct State {
    pub(crate) kind: ResourceKind,
    pub(crate) scope: (NodeId, WorktreeId),
    sequence: u64,
    changes: tokio::sync::watch::Sender<Value>,
}
impl State {
    pub(crate) fn new(kind: ResourceKind, scope: (NodeId, WorktreeId)) -> Self {
        Self {
            kind,
            scope,
            sequence: 0,
            changes: tokio::sync::watch::channel(json!({"cursor":"0","value":null})).0,
        }
    }
    pub(crate) fn reveal(&mut self, value: Value, cx: &mut Context<Self>) {
        self.sequence += 1;
        self.changes
            .send_replace(json!({"cursor":self.sequence.to_string(),"value":value}));
        cx.notify();
    }
}
impl Panel {
    pub(crate) fn resource_page(
        source: Entity<crate::conversation::live::View>,
        binding: crate::conversation::live::Binding,
        kind: ResourceKind,
        cx: &mut Context<Self>,
    ) -> Self {
        let scope = (
            binding.client.target(),
            binding.worktree.expect("resource checkout"),
        );
        let mut panel = Self::standalone(binding, cx);
        panel.resource = Some(cx.new(|_| State::new(kind, scope)));
        panel.source = Some(source);
        panel.attach_workspace(cx.new(super::workspace::State::embedded), cx);
        panel
    }
    pub(crate) fn resource_scope(&self, cx: &App) -> Option<(NodeId, WorktreeId)> {
        self.resource
            .as_ref()
            .map(|state| state.read(cx).scope)
            .or_else(|| self.document_scope(cx))
    }
}

pub(super) fn module(
    module: HostModule,
    state: Option<Entity<State>>,
    host: Arc<Host>,
) -> HostModule {
    let read = state.clone();
    let read_host = host.clone();
    let declarations = format!(
        "{}\nexport function readResourceTarget():{{cursor:string;value:unknown}}|null;\nexport function nextResourceTarget(cursor:string):Promise<{{cursor:string;value:unknown}}>;",
        module.declared().unwrap_or_default()
    );
    module.function("readResourceTarget",move|_|{
        read_host.check()?;
        let value=gpui_shell::with_current_app(|cx|read.as_ref().map(|state|state.read(cx).changes.borrow().clone())).flatten().unwrap_or(Value::Null);
        super::host::sdk::values::encode(value)
    }).async_function("nextResourceTarget",move|args|{
        host.check()?;let seen=args.string(0)?.to_owned();let stop=host.stop_token();
        let mut changes=gpui_shell::with_current_app(|cx|state.as_ref().map(|state|state.read(cx).changes.subscribe())).flatten();
        Ok(async move {
            let Some(changes)=changes.as_mut() else{stop.cancelled().await;return Err(HostError::new("resource view is closed"));};
            loop {let value=changes.borrow_and_update().clone();if value["cursor"].as_str()!=Some(&seen){return super::host::sdk::values::encode(value);}
                tokio::select!{biased;_=stop.cancelled()=>return Err(HostError::new("resource view is closed")),result=changes.changed()=>result.map_err(|_|HostError::new("resource view is closed"))?}
            }
        })
    }).declarations(declarations)
}
