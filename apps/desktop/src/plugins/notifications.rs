//! Embedded script notifications reuse Kit's Root, delivery, and action controls.
//! Kit b79f4ce's script toast requires ShellRoot and cannot carry an action.
use super::{
    Panel,
    host::{
        Host,
        sdk::values::{decode, encode},
    },
};
use gpui_kit::{
    component::{
        WindowExt,
        button::{Button, ButtonVariants},
        notification::{Notification, NotificationType},
    },
    *,
};
use gpui_shell::{HostError, HostModule, HostValue};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Toast {
    id: String,
    message: String,
    kind: Kind,
    action: Option<Action>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Info,
    Error,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    id: String,
    label: String,
}

#[derive(Serialize)]
struct Click {
    id: String,
    action: String,
}

enum Change {
    Show(Toast),
    Dismiss(String),
}

struct Event {
    host: Arc<Host>,
    generation: sailry_protocol::RequestId,
    sender: tokio::sync::mpsc::Sender<Click>,
    ids: Arc<Mutex<BTreeSet<String>>>,
    cleanup: Arc<AtomicBool>,
    change: Change,
}
impl EventEmitter<Event> for Panel {}

pub(super) fn module(module: HostModule, owner: WeakEntity<Panel>, host: Arc<Host>) -> HostModule {
    let generation = sailry_protocol::RequestId::new();
    let (sender, receiver) = tokio::sync::mpsc::channel(16);
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    let ids = Arc::new(Mutex::new(BTreeSet::new()));
    let cleanup = Arc::new(AtomicBool::new(false));
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        include_str!("notifications.d.ts")
    );
    let emit = {
        let host = host.clone();
        let ids = ids.clone();
        move |change| {
            host.check()?;
            gpui_shell::with_current_app(|cx| {
                let owner = owner.clone();
                let event = Event {
                    host: host.clone(),
                    generation,
                    sender: sender.clone(),
                    ids: ids.clone(),
                    cleanup: cleanup.clone(),
                    change,
                };
                // Module initialization already holds the Panel's update guard.
                cx.defer(move |cx| {
                    let _ = owner.update(cx, |_, cx| cx.emit(event));
                });
            })
            .ok_or_else(|| HostError::new("notification requires an active view"))?;
            Ok(HostValue::Null)
        }
    };
    let show = emit.clone();
    let show_ids = ids.clone();
    module.function("toast", move |args| {
        let toast: Toast = serde_json::from_value(decode(args.value(0)?)?)
            .map_err(|error| HostError::new(error.to_string()))?;
        let identifier = sailry_protocol::plugin::ui::identifier;
        if !identifier(&toast.id) || toast.message.trim().is_empty() || toast.message.len() > 4096
            || toast.action.as_ref().is_some_and(|action| !identifier(&action.id) || action.label.trim().is_empty() || action.label.len() > 512) {
            return Err(HostError::new("invalid notification"));
        }
        let mut ids = show_ids.lock().map_err(|_| HostError::new("notifications are unavailable"))?;
        if !ids.contains(&toast.id) && ids.len() >= 16 { return Err(HostError::new("notification capacity exhausted")); }
        ids.insert(toast.id.clone());
        show(Change::Show(toast))
    }).function("dismissToast", move |args| {
        let id = args.string(0)?.to_owned();
        ids.lock().map_err(|_| HostError::new("notifications are unavailable"))?.remove(&id);
        emit(Change::Dismiss(id))
    }).async_function("nextToastEvent", move |_| {
        host.check()?;
        let stop = host.stop_token();
        let receiver = receiver.clone();
        Ok(async move {
            let click = tokio::select! {
                biased;
                _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                click = async {receiver.lock().await.recv().await} => click.ok_or_else(|| HostError::new("notifications are closed"))?,
            };
            encode(serde_json::to_value(click).map_err(|error| HostError::new(error.to_string()))?)
        })
    }).declarations(declarations)
}

impl Panel {
    pub(crate) fn observe_notifications<T: 'static>(
        panel: &Entity<Panel>,
        window: &Window,
        cx: &mut Context<T>,
    ) {
        cx.subscribe_in(panel, window, |_, panel, event: &Event, window, cx| {
            if event.host.check().is_err()
                || panel.read(cx).selected.as_ref() != Some(&event.host.context().package)
            {
                return;
            }
            let id = match &event.change {
                Change::Show(toast) => &toast.id,
                Change::Dismiss(id) => id,
            };
            let key = SharedString::from(format!("plugin-toast-{}-{id}", event.generation));
            let Change::Show(toast) = &event.change else {
                window.remove_notification1::<Panel>(key, cx);
                return;
            };
            let kind = match toast.kind {
                Kind::Info => NotificationType::Info,
                Kind::Error => NotificationType::Error,
            };
            let ids = event.ids.clone();
            let toast_id = toast.id.clone();
            let mut note = Notification::new()
                .with_type(kind)
                .message(toast.message.clone())
                .id1::<Panel>(key.clone())
                .on_close(move |_, _| {
                    if let Ok(mut ids) = ids.lock() {
                        ids.remove(&toast_id);
                    }
                });
            if let Some(action) = &toast.action {
                let sender = event.sender.clone();
                let host = event.host.clone();
                let id = toast.id.clone();
                let action = action.clone();
                let selector = format!(
                    "plugin-toast-action-{}-{}-{}",
                    host.context().package.name,
                    id,
                    action.id
                );
                note = note.action(move |_, _, _| {
                    let sender = sender.clone();
                    let host = host.clone();
                    let id = id.clone();
                    let action = action.clone();
                    Button::new(SharedString::from(selector.clone()))
                        .ghost()
                        .label(action.label)
                        .debug_selector({
                            let selector = selector.clone();
                            move || selector.clone()
                        })
                        .on_click(move |_, _, _| {
                            if host.check().is_ok() {
                                let _ = sender.try_send(Click {
                                    id: id.clone(),
                                    action: action.id.clone(),
                                });
                            }
                        })
                });
            }
            crate::feedback::status(window, toast.message.clone().into(), kind, note, cx);
            if !event.cleanup.swap(true, Ordering::SeqCst) {
                let stop = event.host.stop_token();
                let ids = event.ids.clone();
                let generation = event.generation;
                cx.spawn_in(window, async move |_, cx| {
                    stop.cancelled().await;
                    let keys: Vec<_> = ids
                        .lock()
                        .map(|ids| {
                            ids.iter()
                                .map(|id| {
                                    SharedString::from(format!("plugin-toast-{generation}-{id}"))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let _ = cx.update(|window, cx| {
                        for key in keys {
                            window.remove_notification1::<Panel>(key, cx);
                        }
                    });
                })
                .detach();
            }
        })
        .detach();
    }
}
