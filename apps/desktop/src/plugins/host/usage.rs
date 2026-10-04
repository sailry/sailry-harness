//! Scoped access to the shared Client's canonical usage observer and aggregation.
use super::*;
use sailry_protocol::{Output, plugin::desktop::Surface, usage::Query};
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};

#[derive(Clone)]
pub(crate) struct Source {
    pub client: Arc<Client>,
    pub label: String,
}

#[derive(Clone)]
pub(crate) struct Sources(watch::Sender<Vec<Source>>);
impl Default for Sources {
    fn default() -> Self {
        Self(watch::channel(vec![]).0)
    }
}
impl Sources {
    pub(crate) fn get(&self) -> Vec<Source> {
        self.0.borrow().clone()
    }
    pub(crate) fn set(&self, sources: Vec<Source>) {
        if source_values(&self.get()) != source_values(&sources) {
            self.0.send_replace(sources);
        }
    }
}
fn source_values(sources: &[Source]) -> Value {
    json!(
        sources
            .iter()
            .map(|source| json!({"node":node_key(source.client.target()),"label":source.label}))
            .collect::<Vec<_>>()
    )
}

fn node_key(node: sailry_protocol::NodeId) -> String {
    node.0.iter().map(|byte| format!("{byte:02x}")).collect()
}
pub(crate) fn decode_node(value: &str) -> Result<sailry_protocol::NodeId, HostError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(HostError::new("invalid Node ID"));
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| HostError::new("invalid Node ID"))?;
    }
    Ok(sailry_protocol::NodeId(bytes))
}

struct Observer {
    stop: CancellationToken,
    refresh: mpsc::Sender<()>,
    changes: watch::Receiver<Value>,
}
impl Drop for Observer {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Host {
    pub(super) fn usage_module(
        self: &Arc<Self>,
        module: HostModule,
        sources: Sources,
    ) -> HostModule {
        let observer = Arc::new(Mutex::new(None::<Observer>));
        let watch_host = self.clone();
        let watch_sources = sources.clone();
        let watch_state = observer.clone();
        let next_host = self.clone();
        let next_state = observer.clone();
        let refresh_host = self.clone();
        let refresh_state = observer;
        let inventory_host = self.clone();
        let inventory_sources = sources.clone();
        let choices_host = self.clone();
        let choices_sources = sources.clone();
        let source_host = self.clone();
        let source_changes = sources.clone();
        let source_change_host = self.clone();
        module
            .function("usageSources", move |_| {
                source_host.check_usage()?;
                sdk::values::encode(source_values(&sources.get()))
            })
            .async_function("nextUsageSources", move |args| {
                source_change_host.check_usage()?;
                let seen = sdk::values::decode(args.value(0)?)?;
                let mut receiver = source_changes.0.subscribe();
                let stop = source_change_host.stop.clone();
                Ok(async move { loop {
                    let value = source_values(&receiver.borrow_and_update());
                    if value != seen { return sdk::values::encode(value); }
                    tokio::select! {biased; _=stop.cancelled()=>return Err(HostError::new("plugin view is closed")),result=receiver.changed()=>result.map_err(|_|HostError::new("usage sources are closed"))?}
                } })
            })
            .async_function("readUsageInventory", move |args| {
                inventory_host.check_usage()?;
                let client = inventory_host.usage_client(&inventory_sources, args.get(0))?;
                let permit = inventory_host.capacity.clone().try_acquire_owned().map_err(|_|HostError::new("plugin request capacity exhausted"))?;
                let host = inventory_host.clone();
                Ok(async move {
                    let runtime = host.runtime.clone();
                    runtime.spawn(async move {
                        let _permit = permit;
                        tokio::select! {biased; _=host.stop.cancelled()=>Err(HostError::new("plugin view is closed")),result=async {
                        // Authenticate the package before exposing a narrow inventory projection.
                        let _ = client.read_usage(empty_query()).await.map_err(sdk::values::fault)?;
                        let Output::Snapshot(snapshot) = client.execute(client.prepare(Command::Snapshot)).await.map_err(sdk::values::fault)? else { return Err(HostError::new("Node snapshot expected")); };
                        sdk::values::encode(json!({
                            "node":node_key(snapshot.node),
                            "projects":snapshot.projects.iter().map(|project|json!({"id":project.id,"name":project.name})).collect::<Vec<_>>(),
                            "providers":snapshot.providers.iter().map(|provider|json!({"id":provider.id,"name":provider.name,"models":provider.models.iter().map(|model|&model.id).collect::<Vec<_>>()})).collect::<Vec<_>>()
                        }))
                        }=>result}
                    }).await.map_err(|_|HostError::new("usage worker failed"))?
                })
            })
            .async_function("readUsageChoices", move |args| {
                choices_host.check_usage()?;
                let client = choices_host.usage_client(&choices_sources, args.get(1))?;
                let permit = choices_host.capacity.clone().try_acquire_owned().map_err(|_|HostError::new("plugin request capacity exhausted"))?;
                let query = query(sdk::values::decode(args.value(0)?)?)?;
                let host = choices_host.clone();
                Ok(async move {
                    let runtime=host.runtime.clone();
                    runtime.spawn(async move {
                        let _permit = permit;
                        tokio::select! {biased; _=host.stop.cancelled()=>Err(HostError::new("plugin view is closed")),result=async {
                        let report=client.read_usage(query).await.map_err(sdk::values::fault)?;
                        sdk::values::encode(exact(serde_json::to_value(report).map_err(|error|HostError::new(error.to_string()))?))
                        }=>result}
                    }).await.map_err(|_|HostError::new("usage worker failed"))?
                })
            })
            .function("watchUsage", move |args| {
                watch_host.check_usage()?;
                let query=query(sdk::values::decode(args.value(0)?)?)?;
                let all=args.get(1).and_then(HostValue::as_bool).unwrap_or(false);
                let client=watch_host.usage_client(&watch_sources,args.get(2))?;
                if all && !(query.projects.is_empty() && query.worktrees.is_empty() && query.providers.is_empty() && query.models.is_empty()) {
                    return Err(HostError::new("cross-Node usage requires an unfiltered resource scope"));
                }
                let stop=watch_host.stop.child_token();
                let (refresh,requests)=mpsc::channel(1);
                let (changes,receiver)=watch::channel(json!({"cursor":"0","all":all,"view":null}));
                let host=watch_host.clone();
                let sources=Arc::new(watch_sources.get());
                let cancelled=stop.clone();
                host.runtime.clone().spawn(async move {
                    observe(host,client,sources,query,all,Feed { changes, stop: cancelled, requests }).await;
                });
                *watch_state.lock().map_err(lock_error)?=Some(Observer {stop,refresh,changes:receiver});
                Ok(HostValue::Null)
            })
            .async_function("nextUsageChange", move |args| {
                next_host.check_usage()?;
                let seen=args.string(0)?.to_owned();
                let (mut changes,stop)={
                    let state=next_state.lock().map_err(lock_error)?;
                    let state=state.as_ref().ok_or_else(||HostError::new("usage observer is unavailable"))?;
                    (state.changes.clone(),state.stop.clone())
                };
                Ok(async move {loop {
                    let value=changes.borrow_and_update().clone();
                    if value["cursor"].as_str()!=Some(&seen) {return sdk::values::encode(value);}
                    tokio::select! {biased; _=stop.cancelled()=>return Err(HostError::new("usage observer is closed")), result=changes.changed()=>result.map_err(|_|HostError::new("usage observer is closed"))?}
                }})
            })
            .function("refreshUsage", move |_| {
                refresh_host.check_usage()?;
                let state=refresh_state.lock().map_err(lock_error)?;
                if let Some(state)=state.as_ref(){let _=state.refresh.try_send(());}
                Ok(HostValue::Null)
            })
    }

    fn check_usage(&self) -> Result<(), HostError> {
        self.check()?;
        if !matches!(self.context.surface, Surface::Workspace | Surface::Settings)
            || self.context.turn.is_some()
            || self.context.invocation.is_some()
        {
            return Err(HostError::new("usage requires a page scope"));
        }
        Ok(())
    }

    fn usage_client(
        &self,
        sources: &Sources,
        node: Option<&HostValue>,
    ) -> Result<Arc<Client>, HostError> {
        let node = node
            .filter(|value| !matches!(value, HostValue::Null))
            .map(|value| {
                decode_node(
                    value
                        .as_str()
                        .ok_or_else(|| HostError::new("invalid Node ID"))?,
                )
            })
            .transpose()?
            .unwrap_or(self.client.target());
        if node == self.client.target() {
            return Ok(Arc::new(self.client.with_usage_scope(self.context.clone())));
        }
        let source = sources
            .get()
            .into_iter()
            .find(|source| source.client.target() == node)
            .ok_or_else(|| HostError::new("usage source is unavailable"))?;
        Ok(Arc::new(
            source
                .client
                .with_matching_usage_scope(self.context.clone()),
        ))
    }
}

struct Feed {
    changes: watch::Sender<Value>,
    stop: CancellationToken,
    requests: mpsc::Receiver<()>,
}

async fn observe(
    host: Arc<Host>,
    client: Arc<Client>,
    sources: Arc<Vec<Source>>,
    query: Query,
    all: bool,
    feed: Feed,
) {
    let Feed {
        changes,
        stop,
        requests,
    } = feed;
    let mut clients = if all {
        vec![Arc::new(host.client.with_usage_scope(host.context.clone()))]
    } else {
        vec![client]
    };
    if all {
        for source in sources
            .iter()
            .filter(|source| source.client.target() != host.client.target())
        {
            clients.push(Arc::new(
                source
                    .client
                    .with_matching_usage_scope(host.context.clone()),
            ));
        }
    }
    let (updates, mut receiver) = watch::channel(sailry_client::usage::Overview::default());
    let worker =
        sailry_client::usage::watch_overview(clients, query, updates, stop.clone(), requests);
    tokio::pin!(worker);
    let mut cursor = 0u64;
    let mut failures = Vec::new();
    loop {
        let finished = tokio::select! {biased; _=stop.cancelled()=>return, _=changes.closed()=>return, result=&mut worker=>{let _=result;true}, result=receiver.changed()=>result.is_err()};
        cursor += 1;
        let view = receiver.borrow_and_update().clone();
        let current = view
            .sources
            .iter()
            .filter_map(|source| source.view.error.clone())
            .chain(view.error.clone())
            .collect::<Vec<_>>();
        for error in &current {
            if !failures.contains(error) {
                // Report only the canonical fault, never query values or usage records.
                eprintln!("Usage read failed: {error}");
            }
        }
        failures = current;
        let value = if all {
            serde_json::to_value(&view)
        } else {
            serde_json::to_value(
                view.sources
                    .first()
                    .map(|source| source.view.clone())
                    .unwrap_or_default(),
            )
        };
        if let Ok(value) = value {
            changes
                .send_replace(json!({"cursor":cursor.to_string(),"all":all,"view":exact(value)}));
        }
        if finished {
            return;
        }
    }
}

fn empty_query() -> Query {
    Query {
        start_ms: 0,
        end_ms: 1,
        dimension: sailry_protocol::usage::Dimension::Model,
        projects: vec![],
        worktrees: vec![],
        providers: vec![],
        models: vec![],
        before: None,
    }
}

fn query(mut value: Value) -> Result<Query, HostError> {
    if let Some(node) = value.pointer_mut("/before/node")
        && let Some(text) = node.as_str()
    {
        *node = serde_json::to_value(decode_node(text)?)
            .map_err(|error| HostError::new(error.to_string()))?;
    }
    if let Some(sequence) = value.pointer_mut("/before/sequence")
        && let Some(text) = sequence.as_str()
    {
        *sequence = Value::from(
            text.parse::<u64>()
                .map_err(|_| HostError::new("invalid usage cursor"))?,
        );
    }
    for path in ["/start_ms", "/end_ms", "/before/timestamp_ms"] {
        if let Some(field) = value.pointer_mut(path)
            && let Some(text) = field.as_str()
        {
            *field = Value::from(
                text.parse::<i64>()
                    .map_err(|_| HostError::new("invalid usage timestamp"))?,
            );
        }
    }
    serde_json::from_value(value).map_err(|error| HostError::new(error.to_string()))
}

pub(crate) fn exact(value: Value) -> Value {
    match value {
        Value::Number(number) if number.is_u64() || number.is_i64() => {
            Value::String(number.to_string())
        }
        Value::Array(values) => Value::Array(values.into_iter().map(exact).collect()),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let value = if key == "node" {
                        serde_json::from_value(value.clone())
                            .map(|node| Value::String(node_key(node)))
                            .unwrap_or_else(|_| exact(value))
                    } else {
                        exact(value)
                    };
                    (key, value)
                })
                .collect(),
        ),
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{ErrorCode, plugin::Context};

    #[test]
    fn selects_only_known_scoped_sources() {
        let fixture = crate::plugins::fixture::Fixture::new(true);
        let client = Arc::new(Client::new(fixture.controller.local()));
        let Output::Plugin(package) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ReadPlugin {
                name: "statistics".into(),
            })))
            .unwrap()
        else {
            panic!("plugin expected");
        };
        let host = Host::new(
            client,
            Context {
                invocation: None,
                turn: None,
                surface: Surface::Workspace,
                package: package.summary.reference(),
                worktree: None,
                session: None,
            },
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        let sources = Sources::default();
        sources.set(vec![Source {
            client: fixture.binding.client.clone(),
            label: "Remote fixture".into(),
        }]);
        for selected in [
            None,
            Some(HostValue::Null),
            Some(node_key(host.target()).into()),
        ] {
            let scoped = host.usage_client(&sources, selected.as_ref()).unwrap();
            assert_eq!(scoped.target(), host.target());
            assert_eq!(
                fixture
                    .runtime
                    .block_on(scoped.read_usage(empty_query()))
                    .unwrap()
                    .node,
                fixture.controller.id()
            );
        }
        let selected = HostValue::from(node_key(fixture.node.id()));
        let scoped = host.usage_client(&sources, Some(&selected)).unwrap();
        assert_eq!(scoped.target(), fixture.node.id());
        assert_eq!(
            fixture
                .runtime
                .block_on(scoped.read_usage(empty_query()))
                .unwrap()
                .node,
            fixture.node.id()
        );
        for selected in [
            HostValue::from("invalid"),
            HostValue::Bool(true),
            HostValue::from(node_key(sailry_protocol::NodeId([7; 32]))),
        ] {
            assert!(host.usage_client(&sources, Some(&selected)).is_err());
        }
        let Output::Plugin(package) = fixture.execute(Command::ReadPlugin {
            name: "statistics".into(),
        }) else {
            panic!("plugin expected");
        };
        fixture.execute(Command::SetPluginEnabled {
            name: "statistics".into(),
            expected_revision: package.summary.revision,
            enabled: false,
        });
        assert_eq!(
            fixture
                .runtime
                .block_on(scoped.read_usage(empty_query()))
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert!(
            fixture
                .runtime
                .block_on(
                    host.usage_client(&sources, None)
                        .unwrap()
                        .read_usage(empty_query())
                )
                .is_ok()
        );
        sources.set(vec![]);
        assert!(host.usage_client(&sources, Some(&selected)).is_err());
        host.close();
        drop(host);
        drop(scoped);
        fixture.close();
    }

    #[test]
    fn preserves_exact_cursors_and_counters() {
        let mut query = serde_json::to_value(empty_query()).unwrap();
        query["before"] =
            json!({"node":sailry_protocol::NodeId([1;32]),"timestamp_ms":1,"sequence":u64::MAX});
        let cursor = super::query(exact(query)).unwrap().before.unwrap();
        assert_eq!(cursor.sequence, u64::MAX);
        assert_eq!(cursor.node, sailry_protocol::NodeId([1; 32]));
        assert_eq!(cursor.timestamp_ms, 1);
        assert_eq!(
            exact(json!({"tokens":u64::MAX}))["tokens"],
            u64::MAX.to_string()
        );
    }
}
