//! One marketplace presents execution-Node packages and controller-local themes.
use super::plugins::{details, download, metadata};
use super::{Workspace, group::Group};
use crate::tr;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input::{Input, InputEvent, InputState},
        menu::{DropdownMenu, PopupMenuItem},
        skeleton::Skeleton,
        switch::Switch,
        tab::{Tab, TabBar},
        *,
    },
    prelude::FluentBuilder as _,
    *,
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, Output,
    plugin::{
        Category,
        catalog::{Entry as Listing, Page, Source},
    },
};

mod card;
mod filter;
mod plugins;
mod skills;
mod themes;
mod view;

const WIDTH: f32 = 960.;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Kind {
    #[default]
    Plugins,
    Skills,
    Themes,
}

pub(in crate::settings) struct Market {
    owner: WeakEntity<Workspace>,
    binding: crate::settings::providers::Binding,
    query: Entity<InputState>,
    source: Source,
    kind: Kind,
    filter: filter::Filter,
    page: Option<Page>,
    skills: Option<sailry_protocol::plugin::skillcatalog::Page>,
    themes: themes::State,
    pending: Option<Loading>,
    stop: CancellationToken,
    error: Option<&'static str>,
    task: Option<Task<()>>,
}

#[derive(Clone, PartialEq, Eq)]
enum Loading {
    Catalog,
    Repository(String),
    Theme(String),
}

impl Drop for Market {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(super) fn create(
    owner: Entity<Workspace>,
    binding: crate::settings::providers::Binding,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Market> {
    let state = cx.new(|cx| {
        crate::feedback::observe(window, cx, |state: &Market, _| {
            state.error.into_iter().collect()
        });
        cx.observe(&owner, |_, _, cx| cx.notify()).detach();
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr("market_search_plugins")));
        cx.subscribe_in(&query, window, |state: &mut Market, input, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. })
                || (matches!(event, InputEvent::Change) && input.read(cx).value().trim().is_empty())
            {
                state.search(1, false, cx);
            }
        })
        .detach();
        Market {
            owner: owner.downgrade(),
            binding,
            query,
            source: Source::Official,
            kind: Kind::Plugins,
            filter: Default::default(),
            skills: None,
            themes: themes::State::new(cx),
            page: None,
            pending: None,
            stop: CancellationToken::new(),
            error: None,
            task: None,
        }
    });
    state.update(cx, |state, cx| state.search(1, false, cx));
    state
}

impl Market {
    fn begin(&mut self, loading: Loading) -> CancellationToken {
        self.stop.cancel();
        self.task = None;
        self.stop = CancellationToken::new();
        self.pending = Some(loading);
        self.error = None;
        self.stop.clone()
    }

    fn search(&mut self, page: u32, force: bool, cx: &mut Context<Self>) {
        match self.kind {
            Kind::Plugins => self.load_plugins(page, force, cx),
            Kind::Skills => self.load_skills(page, force, cx),
            Kind::Themes => self.load_themes(force, cx),
        }
    }

    fn select_kind(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        if self.kind == kind {
            return;
        }
        self.kind = kind;
        self.filter = Default::default();
        self.query.update(cx, |query, cx| {
            query.set_placeholder(
                tr(match kind {
                    Kind::Plugins => "market_search_plugins",
                    Kind::Skills => "market_search_skills",
                    Kind::Themes => "market_search_themes",
                }),
                window,
                cx,
            )
        });
        self.search(1, false, cx);
    }

    fn empty(&self, _cx: &App) -> AnyElement {
        Group::new("market_title")
            .heading(false)
            .empty(IconName::Search, "plugins_none")
            .into_any_element()
    }

    fn connected(&self, cx: &App) -> bool {
        self.owner.upgrade().is_some_and(|owner| {
            owner.read(cx).provider_link.as_ref().is_some_and(|live| {
                live.connected && live.binding.client.target() == self.binding.client.target()
            })
        })
    }

    fn busy(&self, cx: &App) -> bool {
        self.pending.is_some()
            || !self.connected(cx)
            || self
                .owner
                .upgrade()
                .is_some_and(|owner| owner.read(cx).plugin_catalog.busy(cx))
    }
}
