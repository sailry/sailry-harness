//! The pinned script host has no application-header slot. This small adapter
//! exposes summaries and icon actions while Kit owns controls and focus.
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        *,
    },
    *,
};
use gpui_shell::{HostError, HostModule, HostValue};
use serde::Deserialize;
use std::sync::Arc;
mod filters;

#[derive(Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Content {
    title: Option<String>,
    tabs: Option<super::controls::tabs::Bar>,
    #[serde(default)]
    min_content_width: f32,
    #[serde(default)]
    min_content_height: f32,
    #[serde(default)]
    items: Vec<Item>,
    #[serde(default)]
    actions: Vec<Action>,
    #[serde(default)]
    filters: Vec<filters::Filter>,
    #[serde(default)]
    filters_label: String,
}
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Item {
    label: String,
    value: String,
    #[serde(default)]
    tone: Tone,
}
#[derive(Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Tone {
    #[default]
    Default,
    Success,
    Danger,
}
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Action {
    id: String,
    label: String,
    icon: String,
    #[serde(default)]
    disabled: bool,
}

pub(crate) struct Header {
    content: Content,
    filters: filters::Controls,
    tabs: super::controls::tabs::Controls,
    actions: tokio::sync::mpsc::Sender<String>,
}

pub(super) fn create(
    tabs: super::controls::tabs::Controls,
    cx: &mut App,
) -> (Entity<Header>, impl FnOnce(HostModule) -> HostModule) {
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    let header = cx.new(|_| Header {
        content: Content::default(),
        filters: Default::default(),
        tabs,
        actions: sender,
    });
    let owner = header.downgrade();
    let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
    (header, move |module: HostModule| {
        module
            .component("Header", move |args, window, cx| {
                if let Some(source) = args.props().get("content").and_then(|value| value.as_str())
                    && let Ok(content) = serde_json::from_str::<Content>(source)
                    && content.items.len() <= 4
                    && content.actions.len() <= 3
                    && content.filters.len() <= 8
                    && content.filters.iter().all(filters::Filter::valid)
                    && content.min_content_width.is_finite()
                    && content.min_content_width >= 0.
                    && content.min_content_height.is_finite()
                    && content.min_content_height >= 0.
                {
                    let _ = owner.update(cx, |header, cx| {
                        if header.content != content {
                            header.content = content;
                            header.sync_filters(window, cx);
                            cx.notify();
                        }
                    });
                }
                div().into_any_element()
            })
            .async_function("header_action", move |_| {
                let receiver = receiver.clone();
                Ok(async move {
                    receiver
                        .lock()
                        .await
                        .recv()
                        .await
                        .map(HostValue::from)
                        .ok_or_else(|| HostError::new("header closed"))
                })
            })
    })
}

impl Header {
    pub(crate) fn has_filters(&self) -> bool {
        !self.content.filters.is_empty()
    }

    pub(crate) fn title(&self, cx: &App) -> Option<AnyElement> {
        if let Some(tabs) = &self.content.tabs
            && !tabs.content.is_empty()
        {
            return Some(self.tabs.render(&tabs.id, &tabs.content, cx));
        }
        self.content
            .title
            .as_ref()
            .map(|title| div().truncate().child(title.clone()).into_any_element())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.content.items.is_empty()
            && self.content.actions.is_empty()
            && self.content.filters.is_empty()
    }
    pub(super) fn min_content_width(&self) -> Pixels {
        px(self.content.min_content_width)
    }

    pub(super) fn min_content_height(&self) -> Pixels {
        px(self.content.min_content_height)
    }

    #[cfg(test)]
    pub(super) fn value(&self, index: usize) -> Option<&str> {
        self.content
            .items
            .get(index)
            .map(|item| item.value.as_str())
    }
}

impl Render for Header {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("plugin-header")
            .gap_3()
            .items_center()
            .text_sm()
            .when(!self.content.filters.is_empty(), |row| {
                row.flex_1().min_w_0().child(self.controls(cx))
            })
            .children(self.content.items.iter().map(|item| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(item.label.clone()),
                    )
                    .child(
                        div()
                            .font_semibold()
                            .text_color(match item.tone {
                                Tone::Default => cx.theme().foreground,
                                Tone::Success => cx.theme().success,
                                Tone::Danger => cx.theme().danger,
                            })
                            .child(item.value.clone()),
                    )
            }))
            .children(self.content.actions.iter().map(|action| {
                let sender = self.actions.clone();
                let id = action.id.clone();
                Button::new(SharedString::from(action.id.clone()))
                    .debug_selector({
                        let id = action.id.clone();
                        move || id.clone()
                    })
                    .disabled(action.disabled)
                    .ghost()
                    .small()
                    .icon(Icon::empty().path(action.icon.clone()))
                    .tooltip(action.label.clone())
                    .accessibility_label(action.label.clone())
                    .on_click(move |_, _, _| {
                        let _ = sender.try_send(id.clone());
                    })
            }))
    }
}
