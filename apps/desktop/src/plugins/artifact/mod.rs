//! Packages compose version controls around a captured session document surface.
use super::{
    Panel,
    host::Host,
    host::sdk::values::{decode, encode},
};
use crate::conversation::live::{Binding, View as Chat};
use gpui_kit::*;
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_protocol::{
    ArtifactId,
    artifact::{Content, Summary},
};
use serde::Deserialize;
use std::sync::Arc;
mod export;
#[cfg(all(target_os = "macos", feature = "workload-tests"))]
pub(super) mod native_check;
#[cfg(all(target_os = "macos", feature = "workload-tests"))]
pub(in crate::plugins) mod native_flow;
mod page;
mod surface;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct Resource {
    pub binding: Binding,
    pub session: sailry_protocol::SessionId,
    pub artifact: Summary,
    pub source: WeakEntity<Chat>,
}

impl Panel {
    pub(crate) fn export_document(
        source: Entity<Chat>,
        artifact: Summary,
        image: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(session) = source.read(cx).session() else {
            crate::feedback::error("", &crate::tr("artifact_export_failed"), window, cx);
            return;
        };
        let resource = Resource {
            binding: source.read(cx).binding(),
            session,
            artifact,
            source: source.downgrade(),
        };
        let (reply, _) = tokio::sync::oneshot::channel();
        export::choose_card(
            resource,
            if image {
                export::Format::Png
            } else {
                export::Format::Html
            },
            reply,
            window,
            cx,
        );
    }

    pub(crate) fn document(
        source: Entity<Chat>,
        artifact: Summary,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::new(source.clone(), cx);
        panel.document = Some(Resource {
            binding: source.read(cx).binding(),
            session: source
                .read(cx)
                .session()
                .expect("document requires a session"),
            artifact,
            source: source.downgrade(),
        });
        panel
    }

    pub(super) fn sync_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(resource) = &self.document else {
            return;
        };
        if !self.connected || self.loading || !self.metadata.read(cx).settled() {
            return;
        }
        let package = self
            .metadata
            .read(cx)
            .entries
            .get(&resource.artifact.package.name)
            .filter(|info| {
                info.summary.enabled
                    && info.extension.as_ref().is_some_and(|extension| {
                        extension.artifact_view.is_some()
                            && extension
                                .desktop
                                .as_ref()
                                .is_some_and(|desktop| desktop.entry.is_some())
                    })
            })
            .map(|info| info.summary.reference());
        if let Some(package) = package {
            if self.selected.as_ref() != Some(&package) {
                self.open(package, window, cx);
            }
        } else if self.snapshot.borrow().as_ref().is_some_and(|snapshot| {
            !snapshot
                .plugins
                .iter()
                .any(|summary| summary.name == resource.artifact.package.name)
        }) {
            if self.error != Some("plugins_uninstalled") {
                self.error = Some("plugins_uninstalled");
                // Reconciliation runs during render; notify observers after drawing.
                let owner = cx.weak_entity();
                cx.defer(move |cx| {
                    let _ = owner.update(cx, |_, cx| cx.notify());
                });
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Props {
    artifact: ArtifactId,
    revision: Option<u32>,
    #[serde(default)]
    labels: std::collections::BTreeMap<String, String>,
}

fn captured(
    resource: &Option<Resource>,
    id: ArtifactId,
    host: &Host,
) -> Result<Resource, HostError> {
    host.check()?;
    resource
        .as_ref()
        .filter(|resource| {
            resource.artifact.id == id
                && Some(resource.session) == host.context().session
                && resource.binding.client.target() == host.target()
        })
        .cloned()
        .ok_or_else(|| HostError::new("artifact is outside the captured surface"))
}

pub(super) fn module(resource: Option<Resource>, host: Arc<Host>, window: &Window) -> HostModule {
    let scope = resource.clone();
    let reader = resource.clone();
    let read_host = host.clone();
    let versions = resource.clone();
    let version_host = host.clone();
    let observer = resource.clone();
    let observe_host = host.clone();
    let renderer = resource.clone();
    let render_host = host.clone();
    let export_host = host.clone();
    let (events, receiver) = tokio::sync::mpsc::channel::<serde_json::Value>(32);
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    let stop = host.stop_token();
    let handle = window.window_handle();
    let live = std::rc::Rc::new(std::cell::RefCell::new(None::<WeakEntity<surface::View>>));
    let mounted = live.clone();
    HostModule::new("sailry/artifacts")
        .function("artifactResource", move |_| encode(scope.as_ref().map_or(serde_json::Value::Null,
            |resource| {
                let mut value=serde_json::json!({"id":resource.artifact.id,"session":resource.session});
                if resource.session!=resource.artifact.session {value["revision"]=serde_json::json!(resource.artifact.revision);}
                value
            })))
        .async_function("readArtifact", move |args| {
            let id = args.string(0)?.parse().map_err(HostError::new)?;
            let resource = captured(&reader, id, &read_host)?;
            let revision = args.get(1).filter(|value| !matches!(value, HostValue::Null)).map(|_| args.integer(1)).transpose()?
                .map(u32::try_from).transpose().map_err(|_| HostError::new("invalid artifact revision"))?;
            let result = read_host.read_artifact(id, revision, resource.artifact.session)?;
            Ok(async move { encode(serde_json::to_value(result.await?).map_err(|error| HostError::new(error.to_string()))?) })
        })
        .async_function("listArtifactRevisions", move |args| {
            let id = args.string(0)?.parse().map_err(HostError::new)?;
            captured(&versions, id, &version_host)?;
            let before = args.get(1).filter(|value| !matches!(value, HostValue::Null)).map(|_| args.integer(1)).transpose()?
                .map(u32::try_from).transpose().map_err(|_| HostError::new("invalid artifact revision"))?;
            version_host.artifact_revisions(id, before, 64)
        })
        .async_function("nextArtifactChange", move |args| {
            let id = observer.as_ref().ok_or_else(|| HostError::new("no artifact surface"))?.artifact.id;
            captured(&observer, id, &observe_host)?;
            let permission = observe_host.artifact_revisions(id, None, 1)?;
            let mut updates = observe_host.artifact_updates()?;
            let seen = args.get(0).and_then(HostValue::as_str).unwrap_or_default().to_owned();
            let stop = observe_host.stop_token();
            Ok(async move {
                permission.await?;
                loop {
                    let view = updates.borrow_and_update().clone();
                    let cursor = format!("{}:{}", view.artifact_cursor, view.connected);
                    if cursor != seen { return encode(serde_json::json!({"cursor":cursor,"connected":view.connected})); }
                    tokio::select! {biased; _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                        changed = updates.changed() => changed.map_err(|_| HostError::new("artifact subscription ended"))?,}
                }
            })
        })
        .component("ArtifactSurface", move |args, window, cx| {
            let Ok(props) = decode(args.props()).and_then(|value| serde_json::from_value::<Props>(value).map_err(|error| HostError::new(error.to_string()))) else { return div().into_any_element() };
            let Ok(resource) = captured(&renderer, props.artifact, &render_host) else { return div().into_any_element() };
            let key: SharedString = format!("document-surface-{}-{}", args.id(), props.artifact).into();
            let host = render_host.clone();
            let revision = props.revision;
            let view = window.use_keyed_state(key, cx, |window, cx| surface::View::new(resource, props, host, events.clone(), window, cx));
            view.update(cx, |view, cx| view.select(revision, window, cx));
            *mounted.borrow_mut() = Some(view.downgrade());
            div().size_full().child(view).into_any_element()
        })
        .async_function("nextArtifactEvent", move |_| {
            let receiver = receiver.clone(); let stop = stop.clone();
            Ok(async move {let event = tokio::select! {biased; _=stop.cancelled()=>return Err(HostError::new("plugin view is closed")), event=async{receiver.lock().await.recv().await}=>event};
                encode(event.ok_or_else(||HostError::new("artifact surface is closed"))?)})
        })
        .async_function("exportArtifact", move |args| {
            let options: export::Options = serde_json::from_value(decode(args.value(0)?)?).map_err(|error| HostError::new(error.to_string()))?;
            let resource = captured(&resource, options.artifact, &export_host)?;
            let host = export_host.clone();
            let live = live.borrow().clone();
            let (sender, receiver) = tokio::sync::oneshot::channel();
            gpui_shell::with_current_app(move |cx| cx.defer(move |cx| {
                if host.check().is_err() {
                    return;
                }
                // The script task already borrows this window until its scope returns.
                let _ = cx.update_window(handle, move |_, window, cx| export::choose(resource, host, options, live, sender, window, cx));
            })).ok_or_else(|| HostError::new("export requires an active view"))?;
            Ok(async move { Ok(HostValue::Bool(receiver.await.unwrap_or(false))) })
        })
        .declarations(include_str!("api.d.ts"))
}
