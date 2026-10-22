use gpui_kit::*;
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{Info, Reference, Summary};
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
    reads: BTreeMap<String, Read>,
    stop: CancellationToken,
}

struct Read {
    expected: Summary,
    stop: CancellationToken,
    pending: bool,
    _task: Option<Task<()>>,
}

impl Drop for Read {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Drop for Metadata {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Metadata {
    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        self.stop.cancel();
        self.reads.clear();
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
            reads: BTreeMap::new(),
            stop: CancellationToken::new(),
        }
    }

    pub(crate) fn accept(&mut self, expected: &[Summary], cx: &mut Context<Self>) {
        if self.reconcile(expected) {
            self.reload(cx);
        }
    }

    fn reconcile(&mut self, expected: &[Summary]) -> bool {
        if self.expected == expected {
            return false;
        }
        self.reads
            .retain(|_, read| expected.iter().any(|summary| *summary == read.expected));
        self.errors.retain(|name, _| {
            self.expected.iter().find(|summary| summary.name == *name)
                == expected.iter().find(|summary| summary.name == *name)
        });
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
        true
    }

    pub(crate) fn ready(&self, package: &Reference) -> bool {
        self.expected
            .iter()
            .find(|summary| summary.reference() == *package)
            .is_some_and(|expected| {
                self.entries
                    .get(&package.name)
                    .is_some_and(|info| info.summary == *expected)
            })
    }

    pub(crate) fn settled(&self) -> bool {
        self.reads.values().all(|read| !read.pending)
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.retry(None, cx);
    }

    pub(super) fn retry(&mut self, name: Option<&str>, cx: &mut Context<Self>) {
        let retry: Vec<_> = self
            .errors
            .keys()
            .filter(|failed| name.is_none_or(|name| name == failed.as_str()))
            .cloned()
            .collect();
        if retry.is_empty() {
            return;
        }
        for name in retry {
            self.reads.remove(&name);
            self.errors.remove(&name);
            self.diagnostics.remove(&name);
        }
        self.reload(cx);
    }

    fn apply(&mut self, batch: loading::Batch, stop: &CancellationToken) -> bool {
        if stop.is_cancelled() {
            return false;
        }
        for (name, result) in batch.results {
            if let Some(read) = self.reads.get_mut(&name) {
                read.pending = batch.retrying;
            }
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
        let missing: Vec<_> = self
            .expected
            .iter()
            .filter(|summary| {
                !self.reads.contains_key(&summary.name)
                    && !self.errors.contains_key(&summary.name)
                    && self
                        .entries
                        .get(&summary.name)
                        .is_none_or(|info| info.summary != **summary)
            })
            .cloned()
            .collect();
        for expected in missing {
            let client = self.client.clone();
            let stop = self.stop.child_token();
            let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
            let job = self.runtime.spawn(loading::load(
                client,
                vec![expected.clone()],
                stop.clone(),
                sender,
            ));
            let cancelled = stop.clone();
            let name = expected.name.clone();
            let task = cx.spawn(async move |metadata, cx| {
                while let Some(batch) = receiver.recv().await {
                    if cancelled.is_cancelled() {
                        return;
                    }
                    if metadata
                        .update(cx, |metadata, cx| {
                            if metadata.apply(batch, &cancelled) {
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
                        if cancelled.is_cancelled() {
                            return;
                        }
                        if let Some(read) = metadata.reads.get_mut(&name) {
                            read.pending = false;
                        }
                        metadata.errors.insert(name, "plugins_read_failed");
                        cx.notify();
                    });
                }
            });
            self.reads.insert(
                expected.name.clone(),
                Read {
                    expected,
                    stop,
                    pending: true,
                    _task: Some(task),
                },
            );
        }
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
