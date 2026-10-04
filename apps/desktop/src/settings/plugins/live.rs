use super::*;
use crate::settings::providers::Binding;
use sailry_protocol::{Command, ErrorCode, Fault, Output, Request, Snapshot, plugin::Summary};

#[derive(Default)]
pub(in crate::settings) struct Catalog {
    pub packages: Vec<Summary>,
    pub(super) market: Option<Entity<super::market::Market>>,
    pub metadata: Option<Entity<Metadata>>,
    pub(super) updates: super::updates::State,
    action: Option<Action>,
    pub(in crate::settings) error: Option<&'static str>,
}

struct Action {
    binding: Binding,
    request: Request,
    pending: bool,
}

impl Catalog {
    pub(in crate::settings) fn busy(&self) -> bool {
        self.action.is_some() || self.updates.busy()
    }
    pub(in crate::settings) fn retryable(&self) -> bool {
        self.action.as_ref().is_some_and(|action| !action.pending) || self.updates.retryable()
    }
    pub(in crate::settings) fn accept(&mut self, snapshot: &Snapshot, cx: &mut App) {
        self.packages = snapshot.plugins.clone();
        self.updates.accept(&snapshot.plugins);
        if let Some(metadata) = &self.metadata {
            metadata.update(cx, |metadata, cx| metadata.accept(&snapshot.plugins, cx));
        }
    }
}

impl Workspace {
    pub(super) fn install_bundled(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(live) = &self.provider_link else {
            return;
        };
        if !live.connected || self.plugin_catalog.busy() {
            return;
        }
        self.plugin_catalog.action = Some(Action {
            binding: live.binding.clone(),
            request: live.binding.client.prepare(Command::InstallBundledPlugin {
                name: name.into(),
                expected_revision: self
                    .plugin_catalog
                    .packages
                    .iter()
                    .find(|plugin| plugin.name == name)
                    .map_or(0, |plugin| plugin.revision),
            }),
            pending: false,
        });
        self.send_plugin_action(cx);
    }

    pub(in crate::settings) fn toggle_plugin(
        &mut self,
        plugin: &Summary,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.provider_link else {
            return;
        };
        if !live.connected || self.plugin_catalog.action.is_some() {
            return;
        }
        self.plugin_catalog.action = Some(Action {
            binding: live.binding.clone(),
            request: live.binding.client.prepare(Command::SetPluginEnabled {
                name: plugin.name.clone(),
                expected_revision: plugin.revision,
                enabled,
            }),
            pending: false,
        });
        self.send_plugin_action(cx);
    }

    pub(in crate::settings) fn send_plugin_action(&mut self, cx: &mut Context<Self>) {
        if self.plugin_catalog.updates.retryable() {
            return;
        }
        let Some(action) = &mut self.plugin_catalog.action else {
            return;
        };
        if action.pending {
            return;
        }
        action.pending = true;
        self.plugin_catalog.error = None;
        let binding = action.binding.clone();
        let request = action.request.clone();
        let id = request.id;
        let job = binding
            .runtime
            .spawn(async move { binding.client.execute(request).await });
        cx.spawn(async move |owner, cx| {
            let result = job.await;
            let _ = owner.update(cx, |owner, cx| {
                let catalog = &mut owner.plugin_catalog;
                let Some(action) = &mut catalog.action else {
                    return;
                };
                if action.request.id != id {
                    return;
                }
                action.pending = false;
                match result {
                    Ok(Ok(Output::Plugin(info))) => {
                        if matches!(
                            &action.request.command,
                            Command::InstallBundledPlugin { .. }
                        ) {
                            crate::preferences::plugins::forget(
                                action.binding.client.target(),
                                &info.summary.name,
                                info.summary.revision,
                                cx,
                            );
                        }
                        catalog.action = None;
                        catalog.error = None;
                    }
                    Ok(Err(error)) => {
                        if !uncertain(&error) {
                            catalog.action = None;
                        }
                        catalog.error = Some(error_key(&error));
                    }
                    _ => catalog.error = Some("plugins_unknown"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

pub(in crate::settings) fn uncertain(error: &Fault) -> bool {
    matches!(
        error.code,
        ErrorCode::OutcomeUnknown | ErrorCode::Unavailable
    )
}

pub(in crate::settings) fn error_key(error: &Fault) -> &'static str {
    match error.code {
        ErrorCode::RevisionConflict | ErrorCode::Conflict => "plugins_conflict",
        ErrorCode::NotFound => "plugins_not_found",
        ErrorCode::InvalidRequest if error.message == "plugin contains invalid skills" => {
            "plugins_issue_skill"
        }
        ErrorCode::InvalidRequest => "plugins_invalid",
        ErrorCode::PermissionDenied => "plugins_path_denied",
        ErrorCode::OutcomeUnknown | ErrorCode::Unavailable => "plugins_unknown",
        ErrorCode::Busy => "plugins_busy",
        _ => "plugins_failed",
    }
}
