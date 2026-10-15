//! Keep segmented-tab state and events inside the native Kit control.
use crate::plugins::host::sdk::values::decode;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::tab::{Tab, TabBar},
    *,
};
use gpui_shell::{HostError, HostModule};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc::Sender;

#[derive(Deserialize)]
struct Item {
    id: String,
    label: String,
}

#[derive(Deserialize)]
struct Props {
    items: Vec<Item>,
    selected: Option<String>,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    fill: bool,
}

fn render(id: String, props: Props, events: Sender<Value>) -> AnyElement {
    let ids = props
        .items
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    TabBar::new(SharedString::from(id.clone()))
        .segmented()
        .w_full()
        .menu(false)
        .selected_index(
            ids.iter()
                .position(|value| Some(value) == props.selected.as_ref())
                .unwrap_or(usize::MAX),
        )
        .children(props.items.into_iter().map(|item| {
            let selector = format!("{id}-item-{}", item.id);
            Tab::new()
                .when(props.fill, |tab| tab.flex_1().min_w_0())
                .label(item.label)
                .disabled(props.disabled)
                .debug_selector(move || selector.clone())
        }))
        .on_click(move |index, _, _| {
            if !props.disabled
                && let Some(value) = ids.get(*index)
            {
                let _ = events.try_send(json!({"id":id,"value":value}));
            }
        })
        .into_any_element()
}

pub(super) fn extend(module: HostModule, events: Sender<Value>) -> HostModule {
    module.component("SegmentedTabs", move |args, _, _| {
        let Ok(props) = decode(args.props()).and_then(|value| {
            serde_json::from_value::<Props>(value)
                .map_err(|error| HostError::new(error.to_string()))
        }) else {
            return div().into_any_element();
        };
        render(args.id().to_owned(), props, events.clone())
    })
}
