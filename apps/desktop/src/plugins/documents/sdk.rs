use super::*;
use crate::plugins::host::{
    Host,
    sdk::values::{decode, encode},
};
use gpui_shell::{HostError, HostModule};
use std::sync::Arc;

pub(in crate::plugins) struct Scope {
    pub controller: Option<Entity<Controller>>,
    pub read: bool,
    pub write: bool,
}

pub(in crate::plugins) fn module(scope: Scope, host: Arc<Host>, cx: &mut App) -> HostModule {
    let controller = scope
        .controller
        .filter(|_| scope.read && host.context().surface == plugin::desktop::Surface::Workspace);
    let changes = controller.as_ref().map(|controller| {
        let controller = controller.read(cx);
        controller.changes.send_replace(controller.snapshot(cx));
        controller.changes.subscribe()
    });
    let surface = controller.clone();
    let surfaces = std::rc::Rc::new(RefCell::new(BTreeMap::<
        RequestId,
        Entity<super::surface::Surface>,
    >::new()));
    let surface_host = host.clone();
    let read = controller.clone();
    let read_host = host.clone();
    let refresh = controller.clone();
    let refresh_host = host.clone();
    let changes_host = host.clone();
    let open = controller.clone();
    let open_host = host.clone();
    let action = controller.clone();
    let action_host = host.clone();
    let save = controller.clone();
    let save_host = host.clone();
    let close = controller;
    let close_host = host;
    HostModule::new("sailry/documents")
        .component("DocumentSurface", move |args, _, cx| {
            let Some(controller) = surface.as_ref().filter(|_| surface_host.check().is_ok()) else { return div().into_any_element(); };
            let Some(id) = args.props().get("document").and_then(|value| value.as_str()).and_then(|id| id.parse::<RequestId>().ok()) else { return div().into_any_element(); };
            if !controller.read(cx).documents.contains_key(&id) { return div().into_any_element(); }
            surfaces.borrow_mut().entry(id).or_insert_with(||cx.new(|cx|super::surface::Surface::new(controller.clone(),id,args.id().to_owned(),surface_host.clone(),scope.write,cx))).clone().into_any_element()
        })
        .function("readDocuments", move |_| {
            read_host.check()?;
            let controller = read.as_ref().ok_or_else(missing)?;
            gpui_shell::with_current_app(|cx| encode(capabilities(controller.read(cx).snapshot(cx), scope.write))).ok_or_else(missing)?
        })
        .function("refreshDocuments", move |_| {
            refresh_host.check()?;
            let controller = refresh.as_ref().ok_or_else(missing)?;
            gpui_shell::with_current_app(|cx| {
                controller.update(cx, |controller, cx| controller.refresh("", refresh_host.context().clone(), cx));
            }).ok_or_else(missing)?;
            Ok(gpui_shell::HostValue::Null)
        })
        .async_function("nextDocumentChange", move |args| {
            changes_host.check()?;
            let seen = args.string(0)?.to_owned();
            let mut changes = changes.clone().ok_or_else(missing)?;
            let stop = changes_host.stop_token();
            Ok(async move {
                loop {
                    let value = changes.borrow_and_update().clone();
                    if value["cursor"].as_str() != Some(&seen) { return encode(capabilities(value, scope.write)); }
                    tokio::select! { biased; _ = stop.cancelled() => return Err(missing()), changed = changes.changed() => changed.map_err(|_| missing())? }
                }
            })
        })
        .async_function("openDocument", move |args| {
            let path = args.string(0)?.to_owned();
            let options = args.value(1).ok().map(decode).transpose()?.unwrap_or(Value::Null);
            let line = options.get("line").map(|value| value.as_u64().and_then(|value| usize::try_from(value).ok()).ok_or_else(|| HostError::new("invalid document line"))).transpose()?;
            let receive = dispatch(&open, &open_host, scope.write, Operation::Open { path, line })?;
            Ok(complete(receive, scope.write))
        })
        .async_function("documentAction", move |args| {
            let id = args.string(0)?.parse().map_err(HostError::new)?;
            let action_value = serde_json::from_value::<Action>(decode(args.value(1)?)?).map_err(|error| HostError::new(error.to_string()))?;
            let receive = dispatch(&action, &action_host, scope.write, Operation::Action { id, action: action_value })?;
            Ok(complete(receive, scope.write))
        })
        .async_function("saveDocument", move |args| {
            let id = args.string(0)?.parse().map_err(HostError::new)?;
            let receive = dispatch(&save, &save_host, scope.write, Operation::Save { id })?;
            Ok(complete(receive, scope.write))
        })
        .async_function("closeDocument", move |args| {
            let id = args.string(0)?.parse().map_err(HostError::new)?;
            let options = args.value(1).ok().map(decode).transpose()?.unwrap_or(Value::Null);
            let discard = options.get("discard").and_then(Value::as_bool).unwrap_or(false);
            let confirm = options.get("confirm").and_then(Value::as_bool).unwrap_or(false);
            let receive = dispatch(&close, &close_host, scope.write, Operation::Close { id, discard, confirm })?;
            Ok(complete(receive, scope.write))
        })
        .declarations(include_str!("api.d.ts"))
}

fn capabilities(mut snapshot: Value, write: bool) -> Value {
    if !write {
        fn readonly(value: &mut Value) {
            if value.is_object() {
                value["can_edit"] = false.into();
                value["can_cut_paste"] = false.into();
                value["can_save"] = false.into();
                value["readonly"] = true.into();
            }
        }
        if let Some(documents) = snapshot.get_mut("documents").and_then(Value::as_array_mut) {
            for document in documents {
                readonly(document);
            }
        } else if snapshot.get("id").is_some() {
            readonly(&mut snapshot);
        }
    }
    snapshot
}

pub(super) fn dispatch(
    controller: &Option<Entity<Controller>>,
    host: &Host,
    write: bool,
    operation: Operation,
) -> Result<tokio::sync::oneshot::Receiver<Result<Value, Fault>>, HostError> {
    host.check()?;
    let controller = controller.clone().ok_or_else(missing)?;
    let (reply, receive) = tokio::sync::oneshot::channel();
    let request = Request {
        context: host.context().clone(),
        write,
        stop: host.stop_token(),
        operation,
        reply: RefCell::new(Some(reply)),
    };
    gpui_shell::with_current_app(|cx| {
        cx.defer(move |cx| controller.update(cx, |_, cx| cx.emit(request)))
    })
    .ok_or_else(missing)?;
    Ok(receive)
}

pub(in crate::plugins) async fn complete(
    receive: tokio::sync::oneshot::Receiver<Result<Value, Fault>>,
    write: bool,
) -> Result<gpui_shell::HostValue, HostError> {
    let value = receive
        .await
        .map_err(|_| missing())?
        .map_err(|fault| HostError::new(serde_json::to_string(&fault).unwrap_or(fault.message)))?;
    encode(capabilities(value, write))
}
fn missing() -> HostError {
    HostError::new("document scope is unavailable")
}

pub(in crate::plugins) fn entry(
    controller: &Option<Entity<Controller>>,
    host: &Host,
    id: RequestId,
    write: bool,
) -> Result<Option<tokio::sync::oneshot::Receiver<Result<Value, Fault>>>, HostError> {
    host.check()?;
    let owns = gpui_shell::with_current_app(|cx| {
        controller.as_ref().is_some_and(|controller| {
            controller
                .read(cx)
                .owns_entry(id, &host.context().package.name)
        })
    })
    .unwrap_or(false);
    owns.then(|| dispatch(controller, host, write, Operation::Entry { id }))
        .transpose()
}
