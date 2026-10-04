use super::super::host::{
    Host,
    sdk::values::{decode, encode},
};
use super::*;
use gpui_shell::{HostError, HostModule, HostValue};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    resource: Resource,
    title: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    title: Option<String>,
    busy: Option<bool>,
}

fn emit(owner: &WeakEntity<Panel>, host: &Host, action: Action) -> Result<HostValue, HostError> {
    host.check()?;
    gpui_shell::with_current_app(|cx| {
        owner
            .update(cx, |panel, cx| {
                if !panel.connected || panel.selected.as_ref() != Some(&host.context().package) {
                    return Err(HostError::new("plugin view is unavailable"));
                }
                match &action {
                    Action::Open { resource, .. } if !panel.permits_pane(*resource, cx) => {
                        return Ok(HostValue::Bool(false));
                    }
                    Action::Update { .. } | Action::Close if panel.pane.is_none() => {
                        return Err(HostError::new("plugin view has no workspace pane"));
                    }
                    Action::Titles(titles)
                        if titles
                            .iter()
                            .any(|title| !panel.permits_title(title.resource, cx)) =>
                    {
                        return Err(HostError::new("resource title is unavailable in this view"));
                    }
                    _ => {}
                }
                let opening = matches!(action, Action::Open { .. });
                cx.emit(Event {
                    package: host.context().package.clone(),
                    action,
                });
                Ok(if opening {
                    HostValue::Bool(true)
                } else {
                    HostValue::Null
                })
            })
            .map_err(|_| HostError::new("plugin view is closed"))?
    })
    .ok_or_else(|| HostError::new("pane navigation requires an active view"))?
}

pub(in crate::plugins) fn module(
    module: HostModule,
    owner: WeakEntity<Panel>,
    host: Arc<Host>,
    scope: Scope,
) -> HostModule {
    let resource = scope.resource;
    let receiver = scope.events.receiver;
    let open_owner = owner.clone();
    let open_host = host.clone();
    let update_owner = owner.clone();
    let update_host = host.clone();
    let close_host = host.clone();
    let state_host = host.clone();
    let title_host = host.clone();
    let title_owner = owner.clone();
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        include_str!("api.d.ts")
    );
    module
        .function("pane", move |_| {
            state_host.check()?;
            encode(serde_json::json!(resource.map(|resource| serde_json::json!({"resource": resource}))))
        })
        .function("openPane", move |args| {
            let request: Open = serde_json::from_value(decode(args.value(0)?)?).map_err(|error| HostError::new(error.to_string()))?;
            emit(&open_owner, &open_host, Action::Open { resource: request.resource, title: request.title })
        })
        .function("updatePane", move |args| {
            let update: Update = serde_json::from_value(decode(args.value(0)?)?).map_err(|error| HostError::new(error.to_string()))?;
            emit(&update_owner, &update_host, Action::Update { title: update.title, busy: update.busy })
        })
        .function("closePane", move |_| emit(&owner, &close_host, Action::Close))
        .function("publishResourceTitles", move |args| {
            let titles: Vec<ResourceTitle> = serde_json::from_value(decode(args.value(0)?)?)
                .map_err(|error| HostError::new(error.to_string()))?;
            emit(&title_owner, &title_host, Action::Titles(titles))
        })
        .async_function("nextPaneEvent", move |_| {
            host.check()?;
            if resource.is_none() { return Err(HostError::new("plugin view has no workspace pane")); }
            let receiver = receiver.clone();
            let stop = host.stop_token();
            Ok(async move {
                let kind = tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                    event = async {receiver.lock().await.recv().await} => event.ok_or_else(|| HostError::new("pane is closed"))?,
                };
                encode(serde_json::json!({"kind":kind}))
            })
        })
        .declarations(declarations)
}
