//! Package-scoped provider contexts for a settings contribution host.
use super::host::{Host, sdk::values::encode};
use gpui_kit::*;
use gpui_shell::{HostError, HostModule};
use sailry_protocol::{
    conversation::Provider,
    plugin::{Reference, model_auth::Resource, ui::ProviderScope},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) struct Source {
    connected: bool,
    providers: Vec<Provider>,
    credentials: Vec<sailry_protocol::Credential>,
    packages: BTreeMap<String, (Reference, Vec<Resource>)>,
}

impl Source {
    pub(crate) fn new() -> Self {
        Self {
            connected: false,
            providers: vec![],
            credentials: vec![],
            packages: BTreeMap::new(),
        }
    }

    pub(crate) fn accept(
        &mut self,
        providers: &[Provider],
        credentials: &[sailry_protocol::Credential],
        metadata: &[sailry_protocol::plugin::Info],
        connected: bool,
        cx: &mut Context<Self>,
    ) {
        let packages = metadata
            .iter()
            .filter(|info| info.summary.enabled)
            .filter_map(|info| {
                Some((
                    info.summary.name.clone(),
                    (
                        info.summary.reference(),
                        info.extension.as_ref()?.model_resources.clone(),
                    ),
                ))
            })
            .collect();
        if self.connected != connected
            || self.providers != providers
            || self.credentials != credentials
            || self.packages != packages
        {
            self.connected = connected;
            self.providers = providers.to_vec();
            self.credentials = credentials.to_vec();
            self.packages = packages;
            cx.notify();
        }
    }

    pub(crate) fn invalidate(&mut self, cx: &mut Context<Self>) {
        self.credentials.clear();
        cx.notify();
    }

    pub(crate) fn scope(&self, provider: &Provider) -> Option<ProviderScope> {
        let reference = provider.credential.as_ref()?;
        let credential = self.credentials.iter().find(|credential| {
            credential.id == reference.id
                && credential.provider == provider.id
                && !credential.revoked
        })?;
        Some(ProviderScope {
            provider: provider.id,
            revision: format!("{}:{}", provider.revision, credential.revision),
            credential: reference.clone(),
        })
    }

    pub(crate) fn read(&self, package: &Reference) -> Value {
        let providers = self
            .packages
            .get(&package.name)
            .filter(|(current, _)| current == package)
            .filter(|_| self.connected)
            .map(|(_, resources)| {
                self.providers
                    .iter()
                    .filter_map(|provider| {
                        let scope = self.scope(provider)?;
                        let mut value =
                            context(provider, &provider.default_model, &package.name, resources)?;
                        value["scope"] = serde_json::to_value(scope)
                            .expect("serializable public provider identity");
                        Some(value)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        json!({"connected":self.connected,"providers":providers})
    }
}

pub(super) struct Watch {
    changes: tokio::sync::watch::Sender<Value>,
    _observer: Subscription,
}

pub(super) fn extend(
    module: HostModule,
    source: Option<Entity<Source>>,
    host: Arc<Host>,
    cx: &mut App,
) -> (HostModule, Option<Entity<Watch>>) {
    let state = source.map(|source| {
        cx.new(|cx| {
            let package = host.context().package.clone();
            let mut value = source.read(cx).read(&package);
            value["cursor"] = "0".into();
            let observer = cx.observe(&source, move |state: &mut Watch, source, cx| {
                let mut value = source.read(cx).read(&package);
                let current = state.changes.borrow().clone();
                value["cursor"] = current["cursor"].clone();
                if value != current {
                    let cursor = current["cursor"]
                        .as_str()
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or(0)
                        + 1;
                    value["cursor"] = cursor.to_string().into();
                    state.changes.send_replace(value);
                }
            });
            Watch {
                changes: tokio::sync::watch::channel(value).0,
                _observer: observer,
            }
        })
    });
    let read = state.clone();
    let read_host = host.clone();
    let change = state.clone();
    let declarations = format!(
        "{}\nexport interface ProviderContext extends NonNullable<ModelContext['provider']> {{scope:import('sailry/sdk').ProviderScope}}\nexport interface ProviderContexts {{cursor:string;connected:boolean;providers:ProviderContext[]}}\nexport function readProviderContexts():ProviderContexts;\nexport function nextProviderContexts(cursor:string):Promise<ProviderContexts>;",
        module.declared().unwrap_or_default()
    );
    let module = module
        .function("readProviderContexts", move |_| {
            read_host.check()?;
            let read = read.as_ref().ok_or_else(|| HostError::new("provider contexts are unavailable"))?;
            gpui_shell::with_current_app(|cx| encode(read.read(cx).changes.borrow().clone()))
                .ok_or_else(|| HostError::new("provider contexts require an active view"))?
        })
        .async_function("nextProviderContexts", move |args| {
            host.check()?;
            let seen = args.string(0)?.to_owned();
            let change = change.as_ref().ok_or_else(|| HostError::new("provider contexts are unavailable"))?;
            let mut updates = gpui_shell::with_current_app(|cx| change.read(cx).changes.subscribe())
                .ok_or_else(|| HostError::new("provider contexts require an active view"))?;
            let stop = host.stop_token();
            Ok(async move {
                loop {
                    let value = updates.borrow_and_update().clone();
                    if value["cursor"].as_str() != Some(&seen) {
                        return encode(value);
                    }
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                        changed = updates.changed() => changed.map_err(|_| HostError::new("provider contexts are closed"))?,
                    }
                }
            })
        })
        .declarations(declarations);
    (module, state)
}

pub(crate) fn context(
    provider: &sailry_protocol::conversation::Provider,
    model: &str,
    package: &str,
    resources: &[sailry_protocol::plugin::model_auth::Resource],
) -> Option<Value> {
    use sailry_protocol::Authentication;
    use sailry_protocol::conversation::presets::Source;
    let owns_preset = provider
        .preset
        .as_ref()
        .is_some_and(|preset| matches!(&preset.source, Source::Package {name} if name==package));
    let declared = matches!(
        provider.authentication,
        Authentication::ChatGpt | Authentication::ChatGptOAuth | Authentication::ClaudeOAuth
    ) && resources
        .iter()
        .any(|resource| resource.authentication == provider.authentication);
    if !provider.enabled || !(owns_preset || declared) {
        return None;
    }
    Some(
        json!({"id":provider.id,"revision":provider.revision.to_string(),"model":model,
            "authentication":provider.authentication,"preset":provider.preset}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn batches_only_current_declared_credentials() {
        let provider:Provider=serde_json::from_value(json!({"id":sailry_protocol::ProviderId::new(),"revision":9007199254740993_u64,"name":"Account","api":"responses","authentication":"chat_gpt_oauth","endpoint":"https://private.example.test","enabled":true,"models":[],"default_model":"selected","credential":{"node":sailry_protocol::NodeId([1;32]),"id":sailry_protocol::CredentialId::new()}})).unwrap();
        let reference: Reference = serde_json::from_value(
            json!({"name":"status","digest":"a".repeat(64),"settings_revision":0}),
        )
        .unwrap();
        let resources:Vec<Resource>=serde_json::from_value(json!([{"authentication":"chat_gpt_oauth","requests":{"usage":{"url":"https://example.test/usage"}}}])).unwrap();
        let mut source = Source {
            connected: true,
            providers: vec![provider.clone()],
            credentials: vec![sailry_protocol::Credential {
                id: provider.credential.as_ref().unwrap().id,
                provider: provider.id,
                authentication: provider.authentication,
                account_label: None,
                revision: 7,
                expires_at_ms: None,
                revoked: false,
            }],
            packages: BTreeMap::from([("status".into(), (reference.clone(), resources))]),
        };
        let first = source.read(&reference);
        assert_eq!(
            first["providers"][0]["scope"]["credential"]["id"],
            json!(provider.credential.as_ref().unwrap().id)
        );
        assert_eq!(
            first["providers"][0]["scope"]["revision"],
            "9007199254740993:7"
        );
        assert!(!first.to_string().contains("private.example"));
        let mut other = reference.clone();
        other.name = "other".into();
        assert_eq!(source.read(&other)["providers"], json!([]));
        source.credentials[0].revision += 1;
        let renewed = source.read(&reference);
        assert_eq!(
            renewed["providers"][0]["revision"],
            first["providers"][0]["revision"]
        );
        assert_eq!(
            renewed["providers"][0]["scope"]["credential"],
            first["providers"][0]["scope"]["credential"]
        );
        assert_eq!(
            renewed["providers"][0]["scope"]["revision"],
            "9007199254740993:8"
        );
        source.credentials.clear();
        assert_eq!(source.read(&reference)["providers"], json!([]));
        source.credentials = vec![sailry_protocol::Credential {
            id: provider.credential.as_ref().unwrap().id,
            provider: provider.id,
            authentication: provider.authentication,
            account_label: None,
            revision: 8,
            expires_at_ms: None,
            revoked: false,
        }];
        source.providers[0].revision += 1;
        source.providers[0].credential.as_mut().unwrap().id = sailry_protocol::CredentialId::new();
        source.credentials[0].id = source.providers[0].credential.as_ref().unwrap().id;
        assert!(!source.read(&reference)["providers"][0]["scope"].is_null());
        assert_ne!(
            source.read(&reference)["providers"][0]["scope"],
            first["providers"][0]["scope"]
        );
        source.connected = false;
        assert_eq!(source.read(&reference)["providers"], json!([]));
    }
}
