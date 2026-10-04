//! Marketplace state owns presentation and requests; the execution Node owns acquisition.
use super::*;
use crate::settings::resource_card;
use gpui_kit::component::{
    group_box::{GroupBox, GroupBoxVariants},
    input::{Input, InputEvent, InputState},
    skeleton::Skeleton,
    tab::{Tab, TabBar},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, Output,
    plugin::catalog::{Entry as Listing, Page, Source},
};

pub(super) struct Market {
    owner: WeakEntity<Workspace>,
    binding: crate::settings::providers::Binding,
    query: Entity<InputState>,
    source: Source,
    page: Option<Page>,
    pending: Option<Loading>,
    stop: CancellationToken,
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
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(tr("plugins_search")));
        cx.subscribe_in(&query, window, |state: &mut Market, input, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. })
                || (matches!(event, InputEvent::Change) && input.read(cx).value().trim().is_empty())
            {
                state.search(1, cx);
            }
        })
        .detach();
        Market {
            owner: owner.downgrade(),
            binding,
            query,
            source: Source::Official,
            page: None,
            pending: None,
            stop: CancellationToken::new(),
            error: None,
            task: None,
        }
    });
    state.update(cx, |state, cx| state.search(1, cx));
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

    fn search(&mut self, page: u32, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().trim().to_owned();
        let stop = self.begin(Loading::Catalog);
        let cancelled = stop.clone();
        let binding = self.binding.clone();
        let source = self.source;
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                biased;
                _ = cancelled.cancelled() => None,
                result = binding.client.execute(binding.client.prepare(Command::SearchPluginCatalog {
                    source,
                    query,
                    page,
                })) => Some(result),
            }
        });
        self.task = Some(cx.spawn(async move |state, cx| {
            let result = job.await;
            _ = state.update(cx, |state, cx| {
                if stop.is_cancelled() {
                    return;
                }
                state.pending = None;
                match result {
                    Ok(Some(Ok(Output::PluginCatalog(page)))) => {
                        state.error = page.unavailable.then_some("plugins_market_failed");
                        if !page.unavailable {
                            state.page = Some(page);
                        }
                    }
                    _ => state.error = Some("plugins_market_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn install(&mut self, entry: Listing, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        if owner
            .read(cx)
            .provider_link
            .as_ref()
            .is_none_or(|live| live.binding.client.target() != self.binding.client.target())
        {
            return;
        }
        if owner
            .read(cx)
            .plugin_catalog
            .packages
            .iter()
            .any(|package| package.name == entry.name)
        {
            return;
        }
        if entry.bundled {
            owner.update(cx, |owner, cx| owner.install_bundled(&entry.name, cx));
            return;
        }
        let stop = self.begin(Loading::Repository(entry.id.clone()));
        let cancelled = stop.clone();
        let binding = self.binding.clone();
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                biased;
                _ = cancelled.cancelled() => None,
                result = binding.client.execute(binding.client.prepare(Command::ReadCatalogPlugin { id: entry.id })) => Some(result),
            }
        });
        self.task = Some(cx.spawn_in(window, async move |state, cx| {
            let result = job.await;
            _ = state.update_in(cx, |state, window, cx| {
                if stop.is_cancelled() {
                    return;
                }
                state.pending = None;
                if owner.read(cx).provider_link.as_ref().is_none_or(|live| {
                    live.binding.client.target() != state.binding.client.target()
                }) {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Some(Ok(Output::PluginRepository(source)))) => {
                        download::install(owner, source, entry.name.clone(), window, cx)
                    }
                    _ => state.error = Some("plugins_market_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn details(&self, entry: Listing, window: &mut Window, cx: &mut Context<Self>) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        if owner.read(cx).provider_link.as_ref().is_none_or(|live| {
            !live.connected || live.binding.client.target() != self.binding.client.target()
        }) {
            return;
        }
        let installed = owner
            .read(cx)
            .plugin_catalog
            .packages
            .iter()
            .find(|package| package.name == entry.name)
            .cloned();
        if let Some(installed) = installed {
            details::open(owner, installed, window, cx);
        } else {
            details::open_catalog(self.binding.clone(), entry, window, cx);
        }
    }
}

impl Render for Market {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = self.owner.upgrade();
        let connected = owner.as_ref().is_some_and(|owner| {
            owner
                .read(cx)
                .provider_link
                .as_ref()
                .is_some_and(|live| live.connected)
        });
        let busy = self.pending.is_some()
            || !connected
            || owner
                .as_ref()
                .is_some_and(|owner| owner.read(cx).plugin_catalog.busy());
        let installed = owner
            .as_ref()
            .map(|owner| {
                let catalog = &owner.read(cx).plugin_catalog;
                catalog.packages.to_vec()
            })
            .unwrap_or_default();
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        TabBar::new("plugin-market-sources")
                            .flex_shrink_0()
                            .segmented()
                            .selected_index(usize::from(self.source == Source::ThirdParty))
                            .children([
                                Tab::new()
                                    .label(tr("plugins_official"))
                                    .debug_selector(|| "plugins-official".into()),
                                Tab::new()
                                    .label(tr("plugins_third_party"))
                                    .debug_selector(|| "plugins-third-party".into()),
                            ])
                            .on_click(cx.listener(|state, index, _, cx| {
                                let source = if *index == 0 {
                                    Source::Official
                                } else {
                                    Source::ThirdParty
                                };
                                if state.source != source {
                                    state.page = None;
                                    state.source = source;
                                }
                                state.search(1, cx);
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .debug_selector(|| "plugins-search-input".into())
                            .child(
                                Input::new(&self.query)
                                    .cleanable(true)
                                    .aria_label(tr("plugins_search")),
                            ),
                    )
                    .child(
                        Button::new("plugins-search")
                            .icon(IconName::Search)
                            .disabled(!connected)
                            .accessibility_label(tr("plugins_search"))
                            .tooltip(tr("plugins_search"))
                            .debug_selector(|| "plugins-search".into())
                            .on_click(cx.listener(|state, _, _, cx| state.search(1, cx))),
                    ),
            )
            .when(self.error.is_some(), |view| {
                view.child(
                    h_flex().gap_2().child(
                        Button::new("plugins-market-retry")
                            .debug_selector(|| "plugins-market-retry".into())
                            .label(tr("plugins_retry"))
                            .disabled(!connected)
                            .on_click(cx.listener(|state, _, _, cx| state.search(1, cx))),
                    ),
                )
            })
            .when(self.pending == Some(Loading::Catalog), |view| {
                view.child(skeleton(cx))
            })
            .when_some(self.page.as_ref().filter(|_| self.pending != Some(Loading::Catalog)), |view, page| {
                let previous = page.page.saturating_sub(1);
                let next = page.page + 1;
                view.when(page.entries.is_empty() && self.pending.is_none(), |view| {
                    view.child(Group::new("plugins_market").heading(false).empty(IconName::Search, "plugins_none"))
                })
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_3()
                        .children(page.entries.iter().map(|entry| {
                            let entry = entry.clone();
                            let known = installed.iter().find(|plugin| plugin.name == entry.name);
                            let info = owner
                                .as_ref()
                                .and_then(|owner| owner.read(cx).plugin_catalog.metadata.as_ref())
                                .and_then(|metadata| metadata.read(cx).entries.get(&entry.name));
                            let title = info
                                .map(metadata::title)
                                .or_else(|| {
                                    entry.display.as_ref().map(|display| {
                                        display.label(&rust_i18n::locale()).to_owned()
                                    })
                                })
                                .unwrap_or_else(|| entry.name.clone());
                            let description = info
                                .and_then(metadata::description)
                                .or_else(|| entry.description(&rust_i18n::locale()).map(str::to_owned));
                            let action_label = tr(if known.is_some() { "plugins_already_installed" } else { "plugins_install" });
                            let summary = resource_card::summary(
                                format!("market-details-{}", entry.id),
                                title,
                                description.unwrap_or_default(),
                                {
                                    let icon = crate::plugins::emblem::render(
                                        &entry.name,
                                        info.and_then(crate::plugins::emblem::glyph)
                                            .or_else(|| entry.display.as_ref().and_then(|display| display.icon.as_ref())),
                                        info.and_then(|info| info.icon.as_deref())
                                            .or(entry.icon.as_deref()),
                                        px(48.),
                                        cx,
                                    );
                                    div()
                                        .flex_shrink_0()
                                        .debug_selector({
                                            let id = entry.id.clone();
                                            move || format!("market-icon-{id}")
                                        })
                                        .child(icon).into_any_element()
                                },
                                cx,
                            )
                            .disabled(!connected)
                            .on_click(cx.listener({
                                let entry = entry.clone();
                                move |state, _, window, cx| state.details(entry.clone(), window, cx)
                            }));
                            let install =
                                    Button::new(SharedString::from(format!(
                                        "market-install-{}",
                                        entry.id
                                    )))
                                    .flex_shrink_0()
                                    .ghost()
                                    .icon(IconName::Plus)
                                    .disabled(busy || known.is_some())
                                    .loading(matches!(&self.pending, Some(Loading::Repository(id)) if id == &entry.id))
                                    .tooltip(action_label.clone())
                                    .accessibility_label(action_label)
                                    .debug_selector({
                                        let id = entry.id.clone();
                                        move || format!("market-install-{id}")
                                    })
                                    .on_click(cx.listener({
                                        let entry = entry.clone();
                                        move |state, _, window, cx| {
                                            cx.stop_propagation();
                                            state.install(entry.clone(), window, cx);
                                        }
                                    }));
                            resource_card::card(format!("market-card-{}", entry.id), summary, install, cx)
                        })),
                )
                .when(page.pages > 1, |view| {
                    view.child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("plugins-market-previous")
                                    .icon(IconName::ChevronLeft)
                                    .disabled(busy || page.page <= 1)
                                    .accessibility_label(tr("plugins_previous"))
                                    .tooltip(tr("plugins_previous"))
                                    .on_click(cx.listener(move |state, _, _, cx| {
                                        state.search(previous, cx)
                                    })),
                            )
                            .child(
                                Button::new("plugins-market-next")
                                    .icon(IconName::ChevronRight)
                                    .disabled(busy || page.page >= page.pages)
                                    .accessibility_label(tr("plugins_next"))
                                    .tooltip(tr("plugins_next"))
                                    .on_click(
                                        cx.listener(move |state, _, _, cx| state.search(next, cx)),
                                    ),
                            ),
                    )
                })
            })
    }
}

fn skeleton(cx: &App) -> impl IntoElement {
    div()
        .grid()
        .grid_cols(2)
        .gap_3()
        .w_full()
        .debug_selector(|| "plugin-market-skeleton".into())
        .children((0usize..6).map(|index| {
            let content = h_flex()
                .min_w_0()
                .items_center()
                .gap_3()
                .child(
                    Skeleton::new()
                        .size_12()
                        .flex_shrink_0()
                        .rounded(cx.theme().radius_lg),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_2()
                        .child(Skeleton::new().w_1_2().h_4().rounded_md())
                        .child(Skeleton::new().w_full().h_3().rounded_md())
                        .child(Skeleton::new().w_2_3().h_3().rounded_md()),
                )
                .child(Skeleton::new().size_5().flex_shrink_0().rounded_md());
            GroupBox::new()
                .id(("plugin-market-placeholder", index))
                .fill()
                .min_w_0()
                .content_style(
                    StyleRefinement::default()
                        .h_full()
                        .justify_center()
                        .px_4()
                        .py_3()
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().radius_lg),
                )
                .child(content)
        }))
}
