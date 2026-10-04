use gpui_kit::*;
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, Output,
    plugin::{Info, Summary},
};
use std::{collections::BTreeMap, sync::Arc};

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
    pub(crate) fn new(client: Arc<Client>, runtime: Arc<tokio::runtime::Runtime>) -> Self {
        Self {
            client,
            runtime,
            expected: Vec::new(),
            entries: BTreeMap::new(),
            errors: BTreeMap::new(),
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

    #[cfg(test)]
    pub(crate) fn skills(&self) -> Vec<String> {
        self.entries
            .values()
            .filter(|info| info.summary.enabled)
            .flat_map(|info| {
                info.skills
                    .iter()
                    .map(|skill| format!("{}:{}", info.summary.name, skill.name))
            })
            .collect()
    }

    pub(crate) fn settled(&self) -> bool {
        !self.loading && self.errors.is_empty()
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.loading && !self.errors.is_empty() {
            self.reload(cx);
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.stop.cancel();
        self.stop = CancellationToken::new();
        self.errors.clear();
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
        let job = self.runtime.spawn(async move {
            let mut results = Vec::new();
            for expected in missing {
                let result = tokio::select! {
                    _ = stop.cancelled() => return None,
                    result = client.execute(client.prepare(Command::ReadPlugin { name: expected.name.clone() })) => result,
                };
                let result = match result {
                    Ok(Output::Plugin(info)) if info.summary == expected => Ok(info),
                    Ok(Output::Plugin(_)) => Err("plugins_conflict"),
                    _ => Err("plugins_read_failed"),
                };
                results.push((expected.name, result));
            }
            Some(results)
        });
        let stop = self.stop.clone();
        self.task = Some(cx.spawn(async move |metadata, cx| {
            let result = job.await;
            let _ = metadata.update(cx, |metadata, cx| {
                if stop.is_cancelled() {
                    return;
                }
                metadata.loading = false;
                match result {
                    Ok(Some(results)) => {
                        for (name, result) in results {
                            match result {
                                Ok(info) => {
                                    metadata.entries.insert(name, info);
                                }
                                Err(error) => {
                                    metadata.errors.insert(name, error);
                                }
                            }
                        }
                    }
                    _ => {
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
                    }
                }
                cx.notify();
            });
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
