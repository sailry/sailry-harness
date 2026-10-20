use gpui_kit::*;
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{Info, Summary};
use std::{collections::BTreeMap, sync::Arc};

mod loading;
#[cfg(test)]
mod tests;

pub(crate) fn title(info: &Info) -> String {
    if let Some(display) = info
        .extension
        .as_ref()
        .and_then(|extension| extension.display.as_ref())
    {
        return display.label(&rust_i18n::locale()).into();
    }
    if info.skill.is_some()
        && let Some(skill) = info.skills.first()
    {
        return skill_title(skill);
    }
    if info.mcp_source.is_some() {
        return info
            .summary
            .name
            .strip_prefix("mcp-")
            .unwrap_or(&info.summary.name)
            .to_owned();
    }
    if let Some(navigation) = info
        .extension
        .as_ref()
        .and_then(|extension| extension.desktop.as_ref())
        .and_then(|desktop| desktop.navigation.as_ref().or(desktop.panel.as_ref()))
        .or_else(|| {
            info.extension
                .as_ref()?
                .settings_page
                .as_ref()
                .map(|page| &page.navigation)
        })
    {
        return navigation.label(&rust_i18n::locale()).into();
    }
    info.summary.name.clone()
}

pub(crate) fn description(info: &Info) -> Option<String> {
    if let Some(description) = info
        .extension
        .as_ref()
        .and_then(|extension| extension.description.as_ref())
    {
        return Some(description.label(&rust_i18n::locale()).into());
    }
    info.settings
        .as_ref()
        .and_then(|schema| schema.locales.get::<str>(&rust_i18n::locale()))
        .and_then(|labels| labels.get("description"))
        .cloned()
        .or_else(|| info.summary.description.clone())
}

/// Read-only presentation cache keyed by the authoritative Node summary.
pub(crate) struct Metadata {
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    expected: Vec<Summary>,
    pub(crate) entries: BTreeMap<String, Info>,
    pub(crate) errors: BTreeMap<String, &'static str>,
    pub(crate) diagnostics: BTreeMap<String, sailry_protocol::Fault>,
    loading: bool,
    stop: CancellationToken,
    task: Option<Task<()>>,
}

impl Drop for Metadata {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Metadata {
    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        self.stop.cancel();
        self.task = None;
        self.loading = false;
        self.errors.clear();
        self.diagnostics.clear();
        cx.notify();
    }

    pub(crate) fn new(client: Arc<Client>, runtime: Arc<tokio::runtime::Runtime>) -> Self {
        Self {
            client,
            runtime,
            expected: Vec::new(),
            entries: BTreeMap::new(),
            errors: BTreeMap::new(),
            diagnostics: BTreeMap::new(),
            loading: false,
            stop: CancellationToken::new(),
            task: None,
        }
    }

    pub(crate) fn accept(&mut self, expected: &[Summary], cx: &mut Context<Self>) {
        if self.expected == expected {
            return;
        }
        self.expected = expected.to_vec();
        self.diagnostics
            .retain(|name, _| expected.iter().any(|summary| summary.name == *name));
        self.entries.retain(|_, info| {
            let Some(summary) = expected
                .iter()
                .find(|summary| summary.name == info.summary.name)
            else {
                return false;
            };
            let mut previous = info.summary.clone();
            if previous.settings_revision != summary.settings_revision {
                previous.settings_revision = summary.settings_revision;
                previous.revision = summary.revision;
            }
            if previous != *summary {
                return false;
            }
            // Keep immutable declarations visible, but reload inventory metadata
            // at the current revision instead of relabeling an old origin.
            true
        });
        self.reload(cx);
    }

    pub(crate) fn settled(&self) -> bool {
        !self.loading && self.errors.is_empty()
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.loading && !self.errors.is_empty() {
            self.reload(cx);
        }
    }

    fn apply(&mut self, batch: loading::Batch, stop: &CancellationToken) -> bool {
        if stop.is_cancelled() {
            return false;
        }
        self.loading = batch.retrying;
        for (name, result) in batch.results {
            self.diagnostics.remove(&name);
            if let Some(error) = batch.diagnostics.get(&name) {
                self.diagnostics.insert(name.clone(), error.clone());
            }
            match result {
                Ok(info) => {
                    self.errors.remove(&name);
                    self.entries.insert(name, info);
                }
                Err(error) => {
                    self.errors.insert(name, error);
                }
            }
        }
        true
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.stop.cancel();
        self.stop = CancellationToken::new();
        self.errors.clear();
        self.diagnostics.clear();
        let missing: Vec<_> = self
            .expected
            .iter()
            .filter(|summary| {
                self.entries
                    .get(&summary.name)
                    .is_none_or(|info| info.summary != **summary)
            })
            .cloned()
            .collect();
        self.loading = !missing.is_empty();
        if missing.is_empty() {
            self.task = None;
            cx.notify();
            return;
        }
        let client = self.client.clone();
        let stop = self.stop.clone();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let job = self
            .runtime
            .spawn(loading::load(client, missing, stop, sender));
        let stop = self.stop.clone();
        self.task = Some(cx.spawn(async move |metadata, cx| {
            while let Some(batch) = receiver.recv().await {
                if stop.is_cancelled() {
                    return;
                }
                if metadata
                    .update(cx, |metadata, cx| {
                        if metadata.apply(batch, &stop) {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    return;
                }
            }
            if job.await.is_err() {
                let _ = metadata.update(cx, |metadata, cx| {
                    if stop.is_cancelled() {
                        return;
                    }
                    metadata.loading = false;
                    for expected in &metadata.expected {
                        if metadata
                            .entries
                            .get(&expected.name)
                            .is_none_or(|info| info.summary != *expected)
                        {
                            metadata
                                .errors
                                .insert(expected.name.clone(), "plugins_read_failed");
                        }
                    }
                    cx.notify();
                });
            }
        }));
        cx.notify();
    }
}

/// Human-readable skill names are independent of their execution keys.
pub(crate) fn skill_title(skill: &sailry_protocol::plugin::Skill) -> String {
    skill.display_name.clone().unwrap_or_else(|| {
        match skill.name.as_str() {
            "pdf" => return "PDF".into(),
            "powerpoint" => return "PowerPoint".into(),
            _ => {}
        }
        skill
            .name
            .split('-')
            .map(|word| {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
}
pub(crate) fn has_connector(info: &sailry_protocol::plugin::Info, server: &str) -> bool {
    use sailry_protocol::plugin::{connector::Auth, mcp::Definition};
    info.extension.as_ref().is_some_and(|extension| {
        extension.connectors.iter().any(|declaration| {
            declaration
                .auth
                .iter()
                .any(|auth| matches!(auth,Auth::McpOauth { server: name,.. } if name == server))
        })
    }) || (server == "server"
        && matches!(
            info.mcp_source,
            Some(Definition::StreamableHttp { .. } | Definition::Sse { .. })
        ))
}
