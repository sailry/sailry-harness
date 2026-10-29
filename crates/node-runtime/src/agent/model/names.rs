//! Package aliases remain canonical; only the captured model boundary exposes names.
//! Adapted from the reviewed Sailry tool exposure WIP 8305566a.
use crate::{
    agent::{catalog::Registration, plugins},
    store::agent::Invocation,
};
use goose_provider_types::{
    conversation::message::{Message, MessageContentBlock},
    errors::ProviderError,
};
use rmcp::model::Tool;
use sailry_protocol::{Authentication, Fault, plugin::Info};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, OnceLock},
};

#[derive(Clone, Default)]
pub(in crate::agent) struct Registry(Arc<OnceLock<Arc<Exposure>>>);

impl Registry {
    pub(in crate::agent) fn capture(
        &self,
        invocation: &Invocation,
        tools: &[Registration],
        historical: &BTreeSet<String>,
    ) -> Result<Arc<Exposure>, Fault> {
        let claude = invocation
            .provider
            .as_ref()
            .is_some_and(|provider| provider.authentication == Authentication::ClaudeOAuth);
        let exposure = Arc::new(Exposure::capture(
            &invocation.plugins,
            tools,
            historical,
            claude,
        ));
        self.0
            .set(exposure.clone())
            .map_err(|_| Fault::invalid("tool exposure was already captured"))?;
        Ok(exposure)
    }

    pub(super) fn get(&self, purpose: super::Purpose) -> Result<Arc<Exposure>, ProviderError> {
        if matches!(purpose, super::Purpose::Compaction) {
            return Ok(Arc::new(Exposure::default()));
        }
        self.0.get().cloned().ok_or_else(|| {
            ProviderError::InvalidValue("tool exposure has not been captured".into())
        })
    }
}

#[derive(Default)]
pub(in crate::agent) struct Exposure {
    forward: BTreeMap<String, String>,
    reverse: BTreeMap<String, String>,
}

impl Exposure {
    fn capture(
        plugins: &[Info],
        tools: &[Registration],
        historical: &BTreeSet<String>,
        claude: bool,
    ) -> Self {
        let declared = tools.iter().filter_map(|entry| {
            let owner = entry.plugin.as_deref()?;
            let declaration = plugins
                .iter()
                .find(|package| package.summary.name == owner)?
                .extension
                .as_ref()?
                .tools
                .iter()
                .find(|tool| {
                    tool.valid()
                        && tool.server.is_none()
                        && crate::plugins::tools::alias(owner, &tool.name)
                            == entry.tool.name.as_ref()
                })?;
            Some((entry.tool.name.to_string(), declaration.name.clone()))
        });
        Self::new(
            declared,
            tools
                .iter()
                .map(|entry| entry.tool.name.to_string())
                .chain(historical.iter().cloned())
                .collect(),
            claude,
        )
    }

    pub(super) fn new(
        declared: impl IntoIterator<Item = (String, String)>,
        registered: BTreeSet<String>,
        claude: bool,
    ) -> Self {
        let wire = |name: &str| {
            if claude {
                super::authorization::canonical(name).to_owned()
            } else {
                name.to_owned()
            }
        };
        let declared: BTreeMap<_, _> = declared.into_iter().collect();
        let mut counts = BTreeMap::new();
        for name in declared.values() {
            *counts.entry(wire(name)).or_insert(0) += 1;
        }
        // Reserve current and historical identities, including deferred tools and
        // aliases which happen to resemble another package's declared name.
        let occupied: BTreeSet<_> = registered.iter().map(|name| wire(name)).collect();
        let forward: BTreeMap<_, _> = declared
            .into_iter()
            .filter(|(_, name)| {
                counts[&wire(name)] == 1
                    && !occupied.contains(&wire(name))
                    && !crate::plugins::tools::is_alias(name)
                    && !crate::agent::mcp::is_alias(name)
            })
            .map(|(alias, name)| (alias, wire(&name)))
            .collect();
        let reverse = forward
            .iter()
            .map(|(alias, name)| (name.clone(), alias.clone()))
            .collect();
        Self { forward, reverse }
    }

    pub(in crate::agent) fn name<'a>(&'a self, name: &'a str) -> &'a str {
        self.forward.get(name).map_or(name, String::as_str)
    }

    pub(super) fn tools<'a>(&self, tools: &'a [Tool]) -> Cow<'a, [Tool]> {
        if self.forward.is_empty() {
            return Cow::Borrowed(tools);
        }
        Cow::Owned(
            tools
                .iter()
                .cloned()
                .map(|mut tool| {
                    tool.name = self.name(&tool.name).to_owned().into();
                    tool
                })
                .collect(),
        )
    }

    pub(super) fn messages<'a>(&self, messages: &'a [Message]) -> Cow<'a, [Message]> {
        if self.forward.is_empty() {
            return Cow::Borrowed(messages);
        }
        let mut messages = messages.to_vec();
        let mut calls = BTreeMap::new();
        for content in messages.iter_mut().flat_map(|message| &mut message.content) {
            match content {
                MessageContentBlock::ToolRequest(request) => {
                    if let Ok(call) = &mut request.tool_call {
                        calls.insert(request.id.clone(), call.name.to_string());
                        call.name = self.name(&call.name).to_owned().into();
                    }
                }
                MessageContentBlock::ToolResponse(response)
                    if calls.get(&response.id).map(String::as_str) == Some("tool_search") =>
                {
                    if let Ok(result) = &mut response.tool_result {
                        plugins::search::project(result, |name| self.name(name).to_owned());
                    }
                }
                MessageContentBlock::ToolConfirmationRequest(request) => {
                    request.tool_name = self.name(&request.tool_name).to_owned();
                }
                _ => {}
            }
        }
        Cow::Owned(messages)
    }

    pub(super) fn restore(&self, message: &mut Message) {
        for content in &mut message.content {
            match content {
                MessageContentBlock::ToolRequest(request) => {
                    if let Ok(call) = &mut request.tool_call
                        && let Some(alias) = self.reverse.get(call.name.as_ref())
                    {
                        call.name = alias.clone().into();
                    }
                }
                MessageContentBlock::ToolConfirmationRequest(request) => {
                    if let Some(alias) = self.reverse.get(&request.tool_name) {
                        request.tool_name = alias.clone();
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;
