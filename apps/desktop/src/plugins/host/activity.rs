//! Activity reads authenticate the package before exposing shared controller data.
use super::*;
use sailry_protocol::{Output, TerminalId, activity::Catalog, plugin::desktop::Surface};
use serde_json::{Value, json};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct Source {
    pub node: sailry_protocol::NodeId,
    pub label: String,
    pub unread_terminals: Vec<TerminalId>,
}

#[derive(Clone)]
pub(crate) struct Sources(watch::Sender<ViewState>);

#[derive(Clone, Default)]
struct ViewState {
    sources: Vec<Source>,
    visible: bool,
}

impl Default for Sources {
    fn default() -> Self {
        Self(watch::channel(ViewState::default()).0)
    }
}

impl Sources {
    pub(crate) fn get(&self) -> Vec<Source> {
        self.0.borrow().sources.clone()
    }

    pub(crate) fn set(&self, sources: Vec<Source>) {
        if source_values(&self.get()) != source_values(&sources) {
            self.0.send_modify(|state| state.sources = sources);
        }
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        if self.0.borrow().visible != visible {
            self.0.send_modify(|state| state.visible = visible);
        }
    }

    pub(crate) fn visible(&self) -> bool {
        self.0.borrow().visible
    }
}

fn source_values(sources: &[Source]) -> Value {
    json!(
        sources
            .iter()
            .map(|source| json!({
                "node": crate::live::node_key(source.node),
                "label": source.label,
                "unread_terminals": source.unread_terminals,
            }))
            .collect::<Vec<_>>()
    )
}

fn host_values(state: &ViewState, node: sailry_protocol::NodeId) -> Value {
    json!({
        "hosts": state.sources.iter().map(|source| json!({
            "node": crate::live::node_key(source.node), "label": source.label,
        })).collect::<Vec<_>>(),
        "unread_terminals": state.sources.iter().find(|source| source.node == node)
            .map(|source| source.unread_terminals.as_slice()).unwrap_or_default(),
        "visible": state.visible,
    })
}

impl Host {
    pub(in crate::plugins) fn activity_module(
        self: &Arc<Self>,
        module: HostModule,
        sources: Sources,
    ) -> HostModule {
        let declarations = format!(
            "{}\n{}",
            module.declared().unwrap_or_default(),
            include_str!("activity.d.ts")
        );
        let read = self.clone();
        let read_sources = sources.clone();
        let previews = self.clone();
        let preview_sources = sources.clone();
        let next = self.clone();
        module
            .async_function("readActivityCatalog", move |_| {
                read.check_activity()?;
                let permit = read.capacity.clone().try_acquire_owned()
                    .map_err(|_| HostError::new("plugin request capacity exhausted"))?;
                let host = read.clone();
                let sources = read_sources.clone();
                Ok(async move {
                    let runtime = host.runtime.clone();
                    runtime.spawn(async move {
                        let _permit = permit;
                        let catalog = host.activity_catalog().await?;
                        let mut value = serde_json::to_value(&catalog)
                            .map_err(|error| HostError::new(error.to_string()))?;
                        value["node"] = crate::live::node_key(catalog.node).into();
                        value["cursor"] = catalog.cursor.to_string().into();
                        value["session_lanes"] = json!(catalog.sessions.iter().map(|session| (
                            session.id.to_string(), lane(sailry_client::activity::lane(session))
                        )).collect::<BTreeMap<_, _>>());
                        value["terminal_lanes"] = json!(catalog.terminals.iter().filter_map(|terminal| {
                            sailry_client::activity::terminal_activity(terminal)
                                .map(|state| (terminal.id.to_string(), lane(state)))
                        }).collect::<BTreeMap<_, _>>());
                        let hosts = host_values(&sources.0.borrow(), catalog.node);
                        value["hosts"] = hosts["hosts"].clone();
                        value["unread_terminals"] = hosts["unread_terminals"].clone();
                        sdk::values::encode(value)
                    }).await.map_err(|_| HostError::new("activity worker failed"))?
                })
            })
            .async_function("readActivityPreviews", move |args| {
                previews.check_activity()?;
                let sessions: Vec<sailry_protocol::SessionId> =
                    serde_json::from_value(sdk::values::decode(args.value(0)?)?)
                        .map_err(|error| HostError::new(error.to_string()))?;
                if sessions.len() > 8 {
                    return Err(HostError::new("activity preview batch exceeds the limit"));
                }
                let mut state = preview_sources.0.subscribe();
                if !state.borrow().visible {
                    return Err(HostError::new("activity page is hidden"));
                }
                let permit = previews.capacity.clone().try_acquire_owned()
                    .map_err(|_| HostError::new("plugin request capacity exhausted"))?;
                let host = previews.clone();
                let request = host.client.prepare(Command::ReadActivity { sessions })
                    .with_plugin(host.context.clone());
                Ok(async move {
                    let runtime = host.runtime.clone();
                    runtime.spawn(async move {
                        let _permit = permit;
                        let read = host.client.execute(request);
                        tokio::pin!(read);
                        loop {
                            tokio::select! {
                                biased;
                                _ = host.stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                                change = state.changed() => {
                                    change.map_err(|_| HostError::new("activity page is closed"))?;
                                    if !state.borrow_and_update().visible {
                                        return Err(HostError::new("activity page is hidden"));
                                    }
                                }
                                result = &mut read => {
                                    let Output::Activity(values) = result.map_err(sdk::values::fault)? else {
                                        return Err(HostError::new("activity previews expected"));
                                    };
                                    return sdk::values::encode(serde_json::to_value(values)
                                        .map_err(|error| HostError::new(error.to_string()))?);
                                }
                            }
                        }
                    }).await.map_err(|_| HostError::new("activity preview worker failed"))?
                })
            })
            .async_function("nextActivityHosts", move |args| {
                next.check_activity()?;
                let seen = sdk::values::decode(args.value(0)?)?;
                let mut changes = sources.0.subscribe();
                let host = next.clone();
                Ok(async move {
                    let runtime = host.runtime.clone();
                    runtime.spawn(async move {
                        loop {
                            let value = host_values(&changes.borrow_and_update(), host.client.target());
                            if value != seen {
                                // Scope validation precedes cached unread IDs as well as Node data.
                                host.activity_catalog().await?;
                                return sdk::values::encode(value);
                            }
                            tokio::select! {
                                biased;
                                _ = host.stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                                result = changes.changed() => result.map_err(|_| HostError::new("activity sources are closed"))?,
                            }
                        }
                    }).await.map_err(|_| HostError::new("activity sources worker failed"))?
                })
            })
            .declarations(declarations)
    }

    fn check_activity(&self) -> Result<(), HostError> {
        self.check()?;
        if self.context.surface != Surface::Workspace
            || self.context.worktree.is_some()
            || self.context.session.is_some()
            || self.context.turn.is_some()
            || self.context.invocation.is_some()
        {
            return Err(HostError::new(
                "activity requires an unscoped workspace view",
            ));
        }
        Ok(())
    }

    async fn activity_catalog(&self) -> Result<Catalog, HostError> {
        self.check_activity()?;
        let request = self
            .client
            .prepare(Command::ReadActivityCatalog)
            .with_plugin(self.context.clone());
        let result = tokio::select! {
            biased;
            _ = self.stop.cancelled() => return Err(HostError::new("plugin view is closed")),
            result = self.client.execute(request) => result.map_err(sdk::values::fault)?,
        };
        let Output::ActivityCatalog(catalog) = result else {
            return Err(HostError::new("activity catalog expected"));
        };
        Ok(catalog)
    }
}

fn lane(value: sailry_client::activity::Lane) -> &'static str {
    use sailry_client::activity::Lane;
    match value {
        Lane::Idle => "idle",
        Lane::Running => "running",
        Lane::Waiting => "waiting",
        Lane::Completed => "completed",
        Lane::Failed => "failed",
    }
}
