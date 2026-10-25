//! One marketplace presents execution-Node packages and controller-local themes.
use super::plugins::{details as plugin_details, download, metadata};
use super::{Workspace, group::Group};
use crate::tr;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input::{Input, InputEvent, InputState},
        menu::{DropdownMenu, PopupMenuItem},
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
pub(in crate::settings) mod details;
mod filter;
mod paging;
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
    paging: paging::State,
    scroll: ScrollHandle,
    skills: Option<sailry_protocol::plugin::skillcatalog::Page>,
    skill_query: Option<skills::cache::Key>,
    skill_cache: skills::cache::Cache,
    skill_details: Vec<details::skills::Snapshot>,
    themes: themes::State,
    updates: Entity<super::plugins::updates::State>,
    pending: Option<Loading>,
    stop: CancellationToken,
    detail_stop: CancellationToken,
    error: Option<&'static str>,
    task: Option<Task<()>>,
}

#[derive(Clone, PartialEq, Eq)]
enum Loading {
    Catalog,
    Repository(String),
}

impl Drop for Market {
    fn drop(&mut self) {
        self.stop.cancel();
        self.detail_stop.cancel();
    }
}

pub(super) fn create(
    owner: Entity<Workspace>,
    binding: crate::settings::providers::Binding,
    themes: Entity<super::appearance::updates::State>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Market> {
    let updates = super::plugins::updates::bind(binding.clone(), cx);
    let state = cx.new(|cx| {
        crate::feedback::observe(window, cx, |state: &Market, _| {
            state.error.into_iter().collect()
        });
        cx.observe(&owner, |_, _, cx| cx.notify()).detach();
        cx.observe(&themes, |_, _, cx| cx.notify()).detach();
        cx.observe(&updates, |_, _, cx| cx.notify()).detach();
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr("market_search_plugins")));
        cx.subscribe_in(&query, window, |state: &mut Market, _, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Change) {
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
            skill_query: None,
            skill_cache: Default::default(),
            skill_details: Vec::new(),
            themes: themes::State::new(themes, cx),
            updates,
            page: None,
            paging: Default::default(),
            scroll: ScrollHandle::new(),
            pending: None,
            stop: CancellationToken::new(),
            detail_stop: CancellationToken::new(),
            error: None,
            task: None,
        }
    });
    state.update(cx, |state, cx| state.search(1, false, cx));
    state
}

impl Market {
    pub(in crate::settings) fn installed(
        &self,
        name: &str,
        cx: &App,
    ) -> Option<sailry_protocol::plugin::Summary> {
        let owner = self.owner.upgrade()?;
        let owner = owner.read(cx);
        if owner
            .provider_link
            .as_ref()
            .is_none_or(|live| live.binding.client.target() != self.binding.client.target())
        {
            return None;
        }
        owner
            .plugin_catalog
            .packages
            .iter()
            .find(|package| package.name == name)
            .cloned()
    }

    fn update(&self, name: &str, window: &mut Window, cx: &mut App) {
        if !self.connected(cx) || self.updates.read(cx).busy() || self.updates.read(cx).selecting()
        {
            return;
        }
        let Some(current) = self.installed(name, cx) else {
            return;
        };
        if self.updates.read(cx).candidate(&current).is_none() {
            return;
        }
        self.updates
            .update(cx, |state, cx| state.apply(Some(name), window, cx));
    }
    pub(in crate::settings) fn begin_details(&mut self) -> CancellationToken {
        self.detail_stop.cancel();
        self.detail_stop = CancellationToken::new();
        self.detail_stop.child_token()
    }

    fn begin(&mut self, loading: Loading) -> CancellationToken {
        self.stop.cancel();
        self.task = None;
        self.stop = CancellationToken::new();
        self.pending = Some(loading);
        self.error = None;
        self.stop.clone()
    }

    fn search(&mut self, page: u32, force: bool, cx: &mut Context<Self>) {
        if let (Some(key), Some(page)) = (self.skill_query.take(), &self.skills) {
            self.skill_cache.remember(
                key,
                skills::cache::Snapshot {
                    page: page.clone(),
                    limit: self.paging.limit,
                    offset: self.scroll.offset(),
                },
            );
        }
        self.paging.reset();
        self.stop.cancel();
        self.task = None;
        self.pending = None;
        self.error = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        match self.kind {
            Kind::Plugins => self.page = None,
            Kind::Skills => {
                let key = skills::cache::Key {
                    node: self.binding.client.target(),
                    source: self.filter.skills.source(),
                    query: self.query.read(cx).value().trim().to_owned(),
                    installed: self.filter.installed,
                };
                let cached = self.skill_cache.get(&key);
                self.skills = cached.as_ref().map(|cached| cached.page.clone());
                if let Some(cached) = cached {
                    self.paging.limit = cached.limit;
                    self.scroll.set_offset(cached.offset);
                }
                self.skill_query = Some(key);
            }
            Kind::Themes if !force && self.themes.listing.is_some() => {
                cx.notify();
                return;
            }
            Kind::Themes => {}
        }
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
            .empty(
                IconName::Search,
                match self.kind {
                    Kind::Plugins => "plugins_none",
                    Kind::Skills => "skills_no_matches",
                    Kind::Themes => "market_no_themes",
                },
            )
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
