//! Shared native document tabs for inline and Shell-header script slots.
use super::{decode, encode, icon};
use gpui_kit::base::Disableable as _;
use gpui_kit::{
    component::{
        ActiveTheme as _, IconName, Sizable as _,
        button::{Button, ButtonVariants},
    },
    *,
};
use gpui_shell::{HostError, HostModule};
use sailry_link::CancellationToken;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

#[derive(Clone, Deserialize, PartialEq)]
pub(crate) struct Bar {
    pub id: String,
    #[serde(flatten)]
    pub content: Content,
}

#[derive(Clone, Deserialize, PartialEq)]
pub(crate) struct Content {
    items: Vec<Item>,
    selected: Option<String>,
    max_width: Option<f32>,
    close_label: String,
}

#[derive(Clone, Deserialize, PartialEq)]
struct Item {
    id: String,
    label: String,
    icon: Option<String>,
    #[serde(default)]
    loading: bool,
    #[serde(default)]
    closable: bool,
    #[serde(default)]
    close_disabled: bool,
    #[serde(default)]
    dirty: bool,
    close_label: Option<String>,
}

impl Content {
    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[derive(Clone)]
pub(crate) struct Controls {
    sender: tokio::sync::mpsc::Sender<Value>,
    receiver: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Value>>>,
    scrolls: Rc<RefCell<BTreeMap<String, ScrollHandle>>>,
    stop: CancellationToken,
}

impl Controls {
    pub(crate) fn new(stop: CancellationToken) -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(32);
        Self {
            sender,
            receiver: Arc::new(tokio::sync::Mutex::new(receiver)),
            scrolls: Default::default(),
            stop,
        }
    }

    pub(super) fn extend(&self, module: HostModule) -> HostModule {
        let controls = self.clone();
        let receiver = self.receiver.clone();
        let stop = self.stop.clone();
        module
            .component("NavigationTabs", move |args, _, cx| {
                let Ok(content) = decode(args.props()).and_then(|value| {
                    serde_json::from_value::<Content>(value)
                        .map_err(|error| HostError::new(error.to_string()))
                }) else {
                    return div().into_any_element();
                };
                controls.render(args.id(), &content, cx)
            })
            .async_function("nextNavigationTabEvent", move |_| {
                let receiver = receiver.clone();
                let stop = stop.clone();
                Ok(async move {
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => Err(HostError::new("plugin view is closed")),
                        event = async { receiver.lock().await.recv().await } => event
                            .ok_or_else(|| HostError::new("tabs are closed"))
                            .and_then(encode),
                    }
                })
            })
    }

    pub(crate) fn render(&self, bar: &str, content: &Content, cx: &App) -> AnyElement {
        let scroll = self
            .scrolls
            .borrow_mut()
            .entry(bar.to_owned())
            .or_default()
            .clone();
        let selected = content
            .items
            .iter()
            .position(|item| Some(&item.id) == content.selected.as_ref());
        let ids = content
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        let width = content
            .max_width
            .filter(|width| width.is_finite() && *width > 0.)
            .unwrap_or(240.);
        let items = content
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let id = item.id.clone();
                let selected = Some(&item.id) == content.selected.as_ref();
                let select_events = self.sender.clone();
                let select_bar = bar.to_owned();
                let select_id = id.clone();
                let close = item.closable.then(|| {
                    let sender = self.sender.clone();
                    let event_bar = bar.to_owned();
                    let id = id.clone();
                    let selector = format!("{}-tab-close-{id}", bar.trim_end_matches("-tabs"));
                    let close_label = item
                        .close_label
                        .clone()
                        .unwrap_or_else(|| content.close_label.clone());
                    Button::new(SharedString::from(selector.clone()))
                        .debug_selector(move || selector.clone())
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .disabled(item.close_disabled)
                        .tooltip(close_label.clone())
                        .accessibility_label(close_label)
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let _ =
                                sender.try_send(json!({"bar":event_bar,"id":id,"kind":"close"}));
                        })
                });
                let prefix = if item.loading {
                    Some(crate::ui::loading::mini().into_any_element())
                } else if item.dirty {
                    let id = id.clone();
                    Some(
                        div()
                            .size_2()
                            .rounded_full()
                            .bg(cx.theme().success)
                            .debug_selector(move || format!("document-dirty-{id}"))
                            .into_any_element(),
                    )
                } else {
                    item.icon
                        .as_deref()
                        .map(|name| icon(name).small().into_any_element())
                };
                let label: SharedString = item.label.clone().into();
                let item = crate::navigation_tabs::item(
                    format!("{}-tab-{id}", bar.trim_end_matches("-tabs")),
                    label.clone(),
                    selected,
                    prefix,
                    close,
                    cx,
                )
                .max_w(px(width))
                .gap_1p5()
                .set_position(index + 1, content.items.len())
                .on_click(move |_, _, _| {
                    let _ = select_events
                        .try_send(json!({"bar":select_bar,"id":select_id,"kind":"select"}));
                });
                (label, item.into_any_element())
            })
            .collect();
        let sender = self.sender.clone();
        let event_bar = bar.to_owned();
        crate::navigation_tabs::strip(
            bar.to_owned(),
            &scroll,
            items,
            selected,
            move |index, _, _| {
                if let Some(id) = ids.get(*index) {
                    let _ = sender.try_send(json!({"bar":event_bar,"id":id,"kind":"select"}));
                }
            },
            cx,
        )
    }
}
