//! Coalesce the same Node fault across retained views without dropping diagnostics.
use super::*;
use sailry_protocol::{ErrorCode, Fault, NodeId};
use std::collections::HashMap;

#[derive(Default)]
struct Owners(HashMap<(WindowId, EntityId), (NodeId, Fault)>);
impl Global for Owners {}

fn key(node: NodeId, fault: &Fault) -> SharedString {
    format!(
        "{}:{:?}:{}",
        crate::live::node_key(node),
        fault.code,
        fault.message
    )
    .into()
}

fn replace(
    owner: (WindowId, EntityId),
    value: Option<(NodeId, Fault)>,
    cx: &mut App,
) -> (Option<SharedString>, Option<(NodeId, Fault)>) {
    if !cx.has_global::<Owners>() {
        cx.set_global(Owners::default());
    }
    cx.update_global(|owners: &mut Owners, _| {
        let before = owners.0.get(&owner).cloned();
        if before == value {
            return (None, None);
        }
        owners.0.remove(&owner);
        let removed = before
            .filter(|previous| {
                !owners
                    .0
                    .iter()
                    .any(|((window, _), current)| *window == owner.0 && current == previous)
            })
            .map(|(node, fault)| key(node, &fault));
        let added = value
            .as_ref()
            .filter(|value| {
                !owners
                    .0
                    .iter()
                    .any(|((window, _), current)| *window == owner.0 && current == *value)
            })
            .cloned();
        if let Some(value) = value {
            owners.0.insert(owner, value);
        }
        (removed, added)
    })
}

pub(crate) fn observe<T: 'static>(
    window: &Window,
    cx: &mut Context<T>,
    errors: impl Fn(&T, &App) -> Option<(NodeId, Fault)> + 'static,
) {
    let window = window.window_handle();
    let owner = (window.window_id(), cx.entity_id());
    cx.observe_self(move |view, cx| {
        let (removed, added) = replace(owner, errors(view, cx), cx);
        if removed.is_none() && added.is_none() {
            return;
        }
        _ = window.update(cx, |_, window, cx| {
            if let Some(key) = removed {
                window.remove_notification1::<Owners>(key, cx);
            }
            if let Some((node, error)) = added {
                let title = tr(match error.code {
                    ErrorCode::Busy => "live_read_busy",
                    ErrorCode::Unavailable => "chat_reconnecting",
                    _ => "live_sync_failed",
                });
                toast(
                    window,
                    title.clone(),
                    diagnostic(error.to_string().into())
                        .title(title)
                        .id1::<Owners>(key(node, &error)),
                    cx,
                );
            }
        });
    })
    .detach();
    cx.on_release(move |_, cx| {
        let (removed, _) = replace(owner, None, cx);
        if let Some(key) = removed {
            _ = window.update(cx, |_, window, cx| {
                window.remove_notification1::<Owners>(key, cx)
            });
        }
    })
    .detach();
    let entity = cx.weak_entity();
    cx.defer(move |cx| {
        _ = entity.update(cx, |_, cx| cx.notify());
    });
}
