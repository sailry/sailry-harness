//! The pinned script menu trigger is left-click only. Reuse the OS menu for right-click regions.
use super::host::sdk::values::{decode, encode};
use gpui_kit::{component::native_menu::NativeMenu, *};
use gpui_shell::{HostError, HostModule};
use sailry_link::CancellationToken;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Item {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub separator: bool,
    #[serde(default)]
    pub value: Option<Value>,
}
fn enabled() -> bool {
    true
}

pub(super) struct Events {
    pub sender: tokio::sync::mpsc::Sender<Value>,
    stop: CancellationToken,
}
impl Events {
    pub fn new(sender: tokio::sync::mpsc::Sender<Value>, stop: CancellationToken) -> Self {
        Self { sender, stop }
    }
    pub fn send(&self, value: Value) {
        if !self.stop.is_cancelled() {
            let _ = self.sender.try_send(value);
        }
    }
}
#[derive(Clone, PartialEq, gpui_kit::Action)]
#[action(namespace = plugin_native_context, no_json)]
struct Dispatch {
    events: Entity<Events>,
    value: Value,
}

fn dispatch(events: Entity<Events>, mut value: Value, field: &str, item: &Item) -> Dispatch {
    value[field] = item.id.clone().into();
    if let Some(target) = &item.value {
        value["value"] = target.clone();
    }
    Dispatch { events, value }
}

#[cfg(test)]
#[derive(Clone)]
struct Captured {
    items: Vec<Item>,
    events: WeakEntity<Events>,
    value: Value,
    field: String,
}

#[cfg(test)]
#[derive(Default)]
struct Captures(std::collections::HashMap<WindowId, Captured>);
#[cfg(test)]
impl Global for Captures {}

#[cfg(test)]
pub(in crate::plugins) fn choose(menu: &str, label: &str, window: &mut Window, cx: &mut App) {
    let captured = cx
        .default_global::<Captures>()
        .0
        .remove(&window.window_handle().window_id())
        .expect("native menu was not opened");
    assert_eq!(
        captured
            .value
            .get("menu")
            .or_else(|| captured.value.get("tree"))
            .and_then(Value::as_str),
        Some(menu),
        "captured menu target changed"
    );
    let item = captured
        .items
        .iter()
        .find(|item| !item.separator && item.label == label)
        .expect("native menu item is absent");
    assert!(item.enabled, "native menu item is disabled");
    let events = captured
        .events
        .upgrade()
        .expect("native menu owner was released");
    window.dispatch_action(
        Box::new(dispatch(
            events,
            captured.value.clone(),
            &captured.field,
            item,
        )),
        cx,
    );
}

pub(super) fn init(cx: &mut App) {
    cx.on_action(|action: &Dispatch, cx| action.events.read(cx).send(action.value.clone()));
}

pub(super) fn show(
    items: &[Item],
    events: Entity<Events>,
    value: Value,
    field: &str,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    if !cfg!(any(target_os = "macos", target_os = "windows")) {
        crate::feedback::error(
            &crate::tr("workspace_native_menu_unavailable"),
            &crate::tr("workspace_native_menu_platform"),
            window,
            cx,
        );
        return;
    }
    #[cfg(test)]
    cx.default_global::<Captures>().0.insert(
        window.window_handle().window_id(),
        Captured {
            items: items.to_vec(),
            events: events.downgrade(),
            value: value.clone(),
            field: field.to_owned(),
        },
    );
    let mut menu = NativeMenu::new();
    for item in items {
        if item.separator {
            menu = menu.separator();
            continue;
        }
        menu = menu.menu_with_disabled(
            item.label.clone(),
            !item.enabled,
            Box::new(dispatch(events.clone(), value.clone(), field, item)),
        );
    }
    menu.show(position, window, cx);
}

pub(super) fn module(module: HostModule, stop: CancellationToken, cx: &mut App) -> HostModule {
    let (sender, receiver) = tokio::sync::mpsc::channel(64);
    let events = cx.new(|_| Events::new(sender, stop.clone()));
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    let declarations = format!(
        "{}\nexport type NativeMenuItem = {{id?: string; label?: string; enabled?: boolean; separator?: boolean; value?: unknown}};\nexport const NativeContextMenu: {{new(id: string, props: {{items: NativeMenuItem[]}}): import(\"gpui-kit\").Element}};\nexport function nextContextMenuEvent(): Promise<{{menu: string; id: string; value?: unknown}}>;",
        module.declared().unwrap_or_default()
    );
    module.component("NativeContextMenu", move |mut args, _, _| {
        let items = args.props().get("items").and_then(|value| decode(value).ok()).and_then(|value| serde_json::from_value::<Vec<Item>>(value).ok()).unwrap_or_default();
        let id = args.id().to_owned();
        let events = events.clone();
        div().id(SharedString::from(id.clone())).debug_selector({let id=id.clone(); move ||id.clone()})
            .flex().flex_col().flex_1().min_w_0().min_h_0().children(args.take_children())
            .on_mouse_down(MouseButton::Right, move |event,window,cx| {
                show(&items,events.clone(),json!({"menu":id}),"id",event.position,window,cx);
                cx.stop_propagation();
            }).into_any_element()
    }).async_function("nextContextMenuEvent", move |_| {
        let stop = stop.clone(); let receiver = receiver.clone();
        Ok(async move {
            let value = tokio::select! { biased; _=stop.cancelled()=>return Err(HostError::new("plugin view is closed")), value=async {receiver.lock().await.recv().await}=>value.ok_or_else(||HostError::new("context menu is closed"))? };
            encode(value)
        })
    }).declarations(declarations)
}
