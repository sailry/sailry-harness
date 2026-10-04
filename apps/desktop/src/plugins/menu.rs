//! Kit b79f4ce's JS DropdownMenu only supports enabled items. This adapter keeps
//! the original Kit controls while exposing disabled/checked items, form triggers,
//! and captured selection IDs.
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    component::{
        Disableable, Icon, Sizable,
        button::{Button, ButtonVariants, DropdownButton},
        menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    },
    *,
};
use gpui_shell::{HostError, HostModule};
use sailry_link::CancellationToken;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum TriggerSize {
    Small,
    Medium,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Props {
    label: String,
    icon: Option<String>,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    form: bool,
    #[serde(default)]
    small: bool,
    size: Option<TriggerSize>,
    items: Vec<Item>,
    primary: Option<Item>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    #[serde(default)]
    id: String,
    #[serde(default)]
    label: String,
    icon: Option<String>,
    #[serde(default)]
    separator: bool,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    checked: bool,
    #[serde(default)]
    children: Vec<Item>,
    value: Option<serde_json::Value>,
}

pub(super) fn module(module: HostModule, stop: CancellationToken) -> HostModule {
    let (sender, receiver) = tokio::sync::mpsc::channel::<serde_json::Value>(16);
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    let closed = stop.clone();
    let declarations = format!(
        "{}\nexport type MenuItem = {{id?:string;label?:string;icon?:string;separator?:boolean;enabled?:boolean;checked?:boolean;children?:MenuItem[];value?:unknown}};\nexport const Menu: {{new(id:string,props:{{label:string;icon?:string;disabled?:boolean;form?:boolean;small?:boolean;size?:'small'|'medium';items:MenuItem[];primary?:MenuItem}}):import(\"gpui-kit\").Element}};\nexport function nextMenuEvent():Promise<{{menu:string;id:string;value?:unknown}}>;",
        module.declared().unwrap_or_default()
    );
    module
        .component("Menu", move |args, _, _| {
            let props = super::host::sdk::values::decode(args.props())
                .ok()
                .and_then(|value| serde_json::from_value::<Props>(value).ok());
            let Some(props) = props.filter(|props| {
                props.items.len() <= 256
                    && !props.label.trim().is_empty()
                    && valid(&props.items)
            }) else {
                return div().into_any_element();
            };
            let sender = sender.clone();
            let id = args.id().to_owned();
            let small = props.small || props.size == Some(TriggerSize::Small);
            let medium = !props.small && props.size == Some(TriggerSize::Medium);
            let build = {
                let sender=sender.clone(); let id=id.clone(); let items=props.items;
                move |menu,window:&mut Window,cx:&mut Context<PopupMenu>| entries(menu,&items,&id,&sender,window,cx)
            };
            if let Some(primary)=props.primary {
                let menu_id=id.clone();let event_id=primary.id.clone();let value=primary.value;
                let button=Button::new(primary.id.clone()).flex_1().label(primary.label)
                    .when_some(primary.icon.as_deref(),|button,icon|button.icon(menu_icon(icon)))
                    .disabled(!primary.enabled||closed.is_cancelled())
                    .debug_selector(move||event_id.clone())
                    .on_click(move|_,_,_|{let _=sender.try_send(serde_json::json!({"menu":menu_id,"id":primary.id,"value":value}));});
                return div().when(props.form,|region|region.w_full()).debug_selector(move||id.clone()).child(DropdownButton::new(args.id().to_owned())
                    .when(small,|button|button.small()).when(props.form,|button|button.secondary().w_full())
                    .button(button).dropdown_menu(build)).into_any_element();
            }
            let button = Button::new(id.clone());
            let button = if props.form {
                button.w_full().label(props.label).dropdown_caret(true)
            } else if let Some(icon) = &props.icon {
                button.ghost().when(!medium,|button|button.small()).icon(menu_icon(icon)).tooltip(props.label.clone()).accessibility_label(props.label)
            } else {
                button.ghost().label(props.label)
            };
            button.when(small, |button| button.small())
                .disabled(props.disabled || closed.is_cancelled())
                .debug_selector(move || id.clone()).dropdown_menu(build).into_any_element()
        })
        .async_function("nextMenuEvent", move |_| {
            let receiver = receiver.clone();
            let stop = stop.clone();
            Ok(async move {
                let event = tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                    event = async {receiver.lock().await.recv().await} => event.ok_or_else(|| HostError::new("plugin menu is closed"))?,
                };
                super::host::sdk::values::encode(event)
            })
        })
        .declarations(declarations)
}

fn menu_icon(value: &str) -> Icon {
    Icon::empty().path(if value.contains(':') || value.contains('/') {
        value.to_owned()
    } else {
        format!("icons/{value}.svg")
    })
}

fn valid(items: &[Item]) -> bool {
    items.iter().all(|item| {
        item.separator
            || (!item.label.trim().is_empty()
                && ((!item.children.is_empty() && valid(&item.children))
                    || (!item.id.is_empty() && item.id.len() <= 256)))
    })
}

fn entries(
    mut menu: PopupMenu,
    items: &[Item],
    id: &str,
    sender: &tokio::sync::mpsc::Sender<serde_json::Value>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    for item in items {
        if item.separator {
            menu = menu.separator();
            continue;
        }
        if !item.children.is_empty() {
            let children = item.children.clone();
            let sender = sender.clone();
            let id = id.to_owned();
            menu = menu.submenu_with_icon(
                item.icon.as_deref().map(menu_icon),
                item.label.clone(),
                window,
                cx,
                move |menu, window, cx| entries(menu, &children, &id, &sender, window, cx),
            );
        } else {
            let sender = sender.clone();
            let id = id.to_owned();
            let event = item.id.clone();
            let value = item.value.clone();
            menu = menu.item(
                PopupMenuItem::new(item.label.clone())
                    .when_some(item.icon.as_deref(), |item, icon| {
                        item.icon(menu_icon(icon))
                    })
                    .disabled(!item.enabled)
                    .checked(item.checked)
                    .on_click(move |_, _, _| {
                        let _ = sender
                            .try_send(serde_json::json!({"menu":id,"id":event,"value":value}));
                    }),
            );
        }
    }
    menu
}
