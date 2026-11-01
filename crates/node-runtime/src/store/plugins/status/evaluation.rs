use super::*;
use sailry_link::Handler as _;
use sailry_protocol::{Command, Output, Request, plugin, plugin::ui};
use serde_json::{Value, json};

pub(super) async fn packages(ingress: &Ingress, scope: Scope) -> Result<Vec<plugin::Info>, Fault> {
    ingress
        .storage
        .get()
        .ok_or_else(unavailable)?
        .read(move |connection| {
            Box::pin(async move {
                let assistant = if let Some(session) = scope.session {
                    commands::session(connection, session)
                        .await?
                        .config
                        .assistant
                        .map(|binding| binding.package.name)
                } else {
                    None
                };
                let mut packages = Vec::new();
                // Agent capture deliberately excludes UI-only packages.
                for summary in super::super::list(connection).await? {
                    if !summary.available() {
                        continue;
                    }
                    let Some(info) = super::super::get(connection, &summary.name).await? else {
                        continue;
                    };
                    let Some(extension) = &info.extension else {
                        continue;
                    };
                    if extension.scope != plugin::Scope::Host
                        || crate::plugins::compatibility::check(&info).is_err()
                        || extension
                            .host
                            .as_ref()
                            .is_none_or(|host| host.status.is_none())
                        || !extension
                            .ui
                            .iter()
                            .any(|entry| entry.slot == ui::Slot::Status)
                        || assistant.as_ref().is_some_and(|name| {
                            name != &summary.name
                                && !extension
                                    .desktop
                                    .as_ref()
                                    .is_some_and(|desktop| desktop.ui_shared)
                        })
                    {
                        continue;
                    }
                    packages.push(info);
                }
                Ok(packages)
            })
        })
        .await
}

pub(super) fn entries(info: &plugin::Info, states: Vec<ui::State>) -> Vec<Entry> {
    let extension = info.extension.as_ref().expect("selected Status package");
    let invoke = &extension
        .host
        .as_ref()
        .and_then(|host| host.status.as_ref())
        .expect("selected Status export")
        .invoke;
    extension
        .ui
        .iter()
        .filter(|entry| entry.slot == ui::Slot::Status)
        .map(|declaration| Entry {
            package: info.summary.reference(),
            declaration: declaration.clone(),
            state: states
                .iter()
                .find(|state| state.id == declaration.id)
                .cloned()
                .unwrap_or_else(|| ui::State::pending(declaration)),
            invoke: (declaration.kind == ui::Kind::Button)
                .then(|| invoke.clone())
                .flatten(),
        })
        .collect()
}

pub(super) fn validate(info: &plugin::Info, value: Value) -> Result<Vec<ui::State>, Fault> {
    if serde_json::to_vec(&value)
        .map_err(|_| Fault::invalid("invalid Status output"))?
        .len()
        > ui::MAX_STATE_BYTES
    {
        return Err(Fault::invalid("Status output exceeds its byte limit"));
    }
    let states: Vec<ui::State> =
        serde_json::from_value(value).map_err(|_| Fault::invalid("invalid Status states"))?;
    if states.len() > ui::MAX_ENTRIES {
        return Err(Fault::invalid("Status output exceeds its entry limit"));
    }
    let declared = &info.extension.as_ref().expect("selected Status package").ui;
    for (index, state) in states.iter().enumerate() {
        if states[..index].iter().any(|other| other.id == state.id)
            || !declared.iter().any(|entry| {
                entry.id == state.id && sailry_protocol::plugin::ui::status::valid(state, entry)
            })
        {
            return Err(Fault::invalid(
                "Status output does not match its declarations",
            ));
        }
    }
    Ok(states)
}

pub(super) async fn read(
    ingress: Ingress,
    scope: Scope,
    info: plugin::Info,
    permits: Arc<Semaphore>,
    stop: CancellationToken,
) -> Result<Vec<Entry>, Fault> {
    let _permit = tokio::select! {
        biased;
        _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "Status evaluation stopped")),
        permit = permits.acquire_owned() => permit.map_err(|_| unavailable())?,
    };
    let status = info
        .extension
        .as_ref()
        .and_then(|extension| extension.host.as_ref())
        .and_then(|host| host.status.as_ref())
        .expect("selected Status export");
    let request = Request::new(
        ingress.node,
        Command::ReadPluginCallback {
            handler: status.read.clone(),
            input: json!({}),
        },
    )
    .with_plugin(plugin::Context {
        invocation: None,
        turn: None,
        surface: plugin::desktop::Surface::Composer,
        package: info.summary.reference(),
        worktree: scope.worktree,
        session: scope.session,
    });
    let evaluation = async {
        let admitted = ingress.dispatch(ingress.node, request).await?;
        admitted.completion.await.map_err(|_| unavailable())?
    };
    let result = tokio::select! {
        biased;
        _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "Status evaluation stopped")),
        result = evaluation => result,
    }?;
    let Output::PluginResult(value) = result else {
        return Err(Fault::invalid("invalid Status callback output"));
    };
    validate(&info, value).map(|states| entries(&info, states))
}

pub(super) fn public(error: Fault) -> Fault {
    Fault::new(error.code, error.message)
}
