mod catalog;
mod data;
mod directory;
mod editor;
mod families;
mod list;
mod live;
mod login;
mod model;
mod presets;
mod vendors;
use gpui_kit::prelude::FluentBuilder as _;
pub(super) use live::Binding;
pub(super) use live::Live;
pub(super) use live::local as bind_local;
pub(crate) use live::{channel, logo};

use super::{
    Workspace,
    group::{Group, Row},
};
use crate::tr;
pub(crate) use data::Category as ModelCategory;
use data::Preset;
pub(super) use data::Store;
pub(crate) use data::{Channel, Model};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    switch::Switch,
    *,
};
use gpui_kit::*;

impl Workspace {
    pub(crate) fn provider_brands(
        entries: Option<&[sailry_protocol::conversation::presets::Entry]>,
        icon: Rems,
    ) -> Vec<(String, SharedString, AnyElement)> {
        editor::Editor::brands(entries, icon)
    }

    pub(crate) fn add_provider(
        owner: Entity<Self>,
        brand: Option<String>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let editor = editor::open(owner, None, window, cx);
        if let Some(brand) = brand {
            editor.update(cx, |editor, cx| editor.choose_brand(brand, window, cx));
        }
    }

    pub(super) fn providers(&self, cx: &mut Context<Self>) -> AnyElement {
        let category = &self.providers.category;
        let busy = self
            .provider_link
            .as_ref()
            .is_some_and(|live| live.pending || !live.connected);
        let owner = cx.entity();
        let query = self.provider_query.read(cx).value().to_string();
        let matching = self
            .providers
            .channels
            .iter()
            .filter(|channel| {
                let provider = self
                    .provider_link
                    .as_ref()
                    .and_then(|live| live.providers.get(&channel.id));
                *category == data::Selection::All
                    || families::selection(channel, provider) == *category
            })
            .filter(|channel| list::matches(channel, &query))
            .collect::<Vec<_>>();
        let rows = Group::new("provider_channels")
            .heading(false)
            .empty(IconName::Cpu, "provider_empty_live")
            .children(
                matching
                    .into_iter()
                    .map(|channel| self.provider_row(channel, cx)),
            );
        let header = h_flex()
            .debug_selector(|| "settings-heading-provider_channels".into())
            .w_full()
            .min_w_0()
            .min_h_8()
            .gap_2()
            .child(
                h_flex()
                    .debug_selector(|| "settings-title-provider_channels".into())
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .items_baseline()
                    .child(div().font_semibold().child(tr("provider_channels")))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.providers.channels.len().to_string()),
                    ),
            )
            .child(
                h_flex()
                    .debug_selector(|| "settings-actions-provider_channels".into())
                    .flex_shrink_0()
                    .gap_2()
                    .child(
                        div()
                            .debug_selector(|| "provider-search".into())
                            .w(px(220.))
                            .child(
                                Input::new(&self.provider_query)
                                    .cleanable(true)
                                    .prefix(Icon::new(IconName::Search).small()),
                            ),
                    )
                    .child(self.provider_families(cx))
                    .child(
                        Button::new("settings-add")
                            .disabled(busy)
                            .primary()
                            .debug_selector(|| "settings-add".into())
                            .label(tr("settings_add"))
                            .icon(IconName::Plus)
                            .on_click(move |_, window, cx| {
                                Self::add_provider(owner.clone(), None, window, cx);
                            }),
                    ),
            );
        v_flex()
            .gap_6()
            .child(self.model_catalog(cx))
            .child(v_flex().gap_3().child(header).child(rows))
            .into_any_element()
    }
}

pub(super) fn query(window: &mut Window, cx: &mut Context<Workspace>) -> Entity<InputState> {
    let query = cx.new(|cx| InputState::new(window, cx).placeholder(tr("provider_list_search")));
    cx.subscribe(&query, |_, _, event, cx| {
        if matches!(event, InputEvent::Change) {
            cx.notify();
        }
    })
    .detach();
    query
}
