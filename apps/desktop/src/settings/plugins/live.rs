use super::*;
use crate::settings::providers::Binding;
use sailry_protocol::{Command, ErrorCode, Fault, Output, Request, Snapshot, plugin::Summary};

#[derive(Default)]
pub(in crate::settings) struct Catalog {
    pub(in crate::settings) python: sailry_client::python::View,
    pub packages: Vec<Summary>,
    pub(in crate::settings) market: Option<Entity<super::market::Market>>,
    pub metadata: Option<Entity<Metadata>>,
    pub(in crate::settings) updates: Option<Entity<super::updates::State>>,
    pub(super) inventory: Option<super::inventory::Inventory>,
    action: Option<Action>,
    pub(in crate::settings) error: Option<&'static str>,
}

struct Action {
    binding: Binding,
    request: Request,
    pending: bool,
}

impl Catalog {
    pub(in crate::settings) fn busy(&self, cx: &App) -> bool {
        self.action.is_some()
            || self
                .updates
                .as_ref()
                .is_some_and(|state| state.read(cx).busy() || state.read(cx).selecting())
    }
    pub(in crate::settings) fn retryable(&self, cx: &App) -> bool {
        self.action.as_ref().is_some_and(|action| !action.pending)
            || self
                .updates
                .as_ref()
                .is_some_and(|state| state.read(cx).retryable())
    }
    pub(in crate::settings) fn accept(&mut self, snapshot: &Snapshot, cx: &mut App) {
        self.packages = snapshot.plugins.clone();
        if let Some(state) = &self.updates {
            state.update(cx, |state, cx| {
                state.accept(&snapshot.plugins);
                cx.notify();
            });
        }
        if let Some(metadata) = &self.metadata {
            metadata.update(cx, |metadata, cx| metadata.accept(&snapshot.plugins, cx));
        }
    }
}

impl Workspace {
    pub(in crate::settings) fn install_bundled(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.provider_link else {
            return;
        };
        if !live.connected || self.plugin_catalog.busy(cx) {
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
        self.send_plugin_action(window, cx);
    }

    pub(in crate::settings) fn toggle_plugin(
        &mut self,
        plugin: &Summary,
        enabled: bool,
        window: &mut Window,
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
        self.send_plugin_action(window, cx);
    }

    pub(in crate::settings) fn send_plugin_action(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .plugin_catalog
            .updates
            .as_ref()
            .is_some_and(|state| state.read(cx).retryable())
        {
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
        dispatch(cx.entity(), binding, request, window, cx);
        cx.notify();
    }
}

fn dispatch(
    owner: Entity<Workspace>,
    binding: Binding,
    request: Request,
    window: &mut Window,
    cx: &mut App,
) {
    let id = request.id;
    let node = binding.client.target();
    let bundled = matches!(&request.command, Command::InstallBundledPlugin { .. });
    let retry = owner.clone();
    let retry_binding = binding.clone();
    let captured = request.clone();
    let ticket = crate::installations::track(
        binding.client.clone(),
        binding.runtime.clone(),
        &request,
        false,
        move |attempt, window, cx| {
            let request = attempt.request(&retry_binding.client, &captured);
            let selected = retry
                .read(cx)
                .provider_link
                .as_ref()
                .is_some_and(|live| live.binding.client.target() == node);
            if !selected {
                // Recovery remains bound to its captured Node even if the
                // workspace now presents another host. Do not replace that
                // host's catalog or reconstruct the old request there.
                dispatch(retry.clone(), retry_binding.clone(), request, window, cx);
                return;
            }
            retry.update(cx, |owner, cx| {
                if owner
                    .plugin_catalog
                    .action
                    .as_ref()
                    .is_some_and(|action| action.request.id != id)
                {
                    return;
                }
                if owner
                    .plugin_catalog
                    .action
                    .as_ref()
                    .is_none_or(|action| !action.pending)
                {
                    owner.plugin_catalog.action = Some(Action {
                        binding: retry_binding.clone(),
                        request,
                        pending: false,
                    });
                }
                owner.send_plugin_action(window, cx);
            })
        },
        window,
        cx,
    );
    let job = binding
        .runtime
        .spawn(async move { binding.client.execute(request).await });
    let origin = window.window_handle();
    cx.spawn(async move |cx| {
        let result = job.await;
        if let Some(ticket) = &ticket {
            cx.update(|cx| ticket.record(crate::installations::result(&result), cx));
        }
        let reduced = cx.update(|cx| {
            if let Ok(Ok(Output::Plugin(info))) = &result
                && bundled
            {
                crate::preferences::plugins::forget(
                    node,
                    &info.summary.name,
                    info.summary.revision,
                    cx,
                );
            }
            owner.update(cx, |owner, cx| {
                let catalog = &mut owner.plugin_catalog;
                let Some(action) = &mut catalog.action else {
                    return false;
                };
                if action.request.id != id || action.binding.client.target() != node {
                    return false;
                }
                action.pending = false;
                match &result {
                    Ok(Ok(Output::Plugin(_))) => {
                        catalog.action = None;
                        catalog.error = None;
                    }
                    Ok(Err(error)) => {
                        if !uncertain(error) {
                            catalog.action = None;
                        }
                        catalog.error = Some(error_key(error));
                    }
                    _ => catalog.error = Some("plugins_unknown"),
                }
                cx.notify();
                true
            })
        });
        if let Some(ticket) = &ticket
            && let Some(target) = cx.update(|cx| crate::installations::target(origin, cx))
        {
            let _ = target.update(cx, |_, window, cx| ticket.close_empty(window, cx));
        }
        if !reduced
            && let Ok(Err(error)) = &result
            && let Some(target) = cx.update(|cx| crate::installations::target(origin, cx))
        {
            let key = error_key(error);
            let _ = target.update(cx, |_, window, cx| {
                crate::feedback::toast(
                    window,
                    tr(key),
                    gpui_kit::component::notification::Notification::error(tr(key)),
                    cx,
                )
            });
        }
    })
    .detach();
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
