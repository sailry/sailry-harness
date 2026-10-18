use super::*;

impl Market {
    pub(super) fn load_plugins(&mut self, page: u32, force: bool, cx: &mut Context<Self>) {
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
                    force,
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
                        state.page = Some(page);
                    }
                    _ => state.error = Some("plugins_market_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(in crate::settings) fn install(
        &mut self,
        source: Source,
        entry: Listing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            owner.update(cx, |owner, cx| {
                owner.install_bundled(&entry.name, window, cx)
            });
            return;
        }
        let stop = self.begin(Loading::Repository(entry.id.clone()));
        let cancelled = stop.clone();
        let binding = self.binding.clone();
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                biased;
                _ = cancelled.cancelled() => None,
                result = binding.client.execute(binding.client.prepare(Command::ReadCatalogPlugin { source, id: entry.id })) => Some(result),
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

    pub(super) fn details(&self, entry: Listing, window: &mut Window, cx: &mut Context<Self>) {
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
            details::open_catalog(
                cx.entity(),
                self.binding.clone(),
                self.source,
                entry,
                window,
                cx,
            );
        }
    }
}

impl Market {
    pub(super) fn plugin_cards(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(page) = &self.page else {
            return if self.pending.is_some() {
                card::skeleton(cx)
            } else {
                self.empty(cx)
            };
        };
        let owner = self.owner.upgrade();
        let packages = owner
            .as_ref()
            .map(|owner| owner.read(cx).plugin_catalog.packages.clone())
            .unwrap_or_default();
        let visible: Vec<_> = page
            .entries
            .iter()
            .filter(|entry| {
                self.filter.plugin(
                    entry.category,
                    packages.iter().any(|package| package.name == entry.name),
                )
            })
            .collect();
        let busy = self.busy(cx);
        let connected = self.connected(cx);
        let previous = page.page.saturating_sub(1);
        let next = page.page + 1;
        v_flex()
            .gap_3()
            .when(visible.is_empty(), |view| view.child(self.empty(cx)))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_3()
                    .children(visible.into_iter().map(|entry| {
                        let known = packages.iter().any(|package| package.name == entry.name);
                        let update = owner
                            .as_ref()
                            .and_then(|owner| owner.read(cx).plugin_catalog.updates.as_ref())
                            .is_some_and(|state| state.read(cx).has_candidate(&entry.name));
                        let info = owner
                            .as_ref()
                            .and_then(|owner| owner.read(cx).plugin_catalog.metadata.as_ref())
                            .and_then(|metadata| metadata.read(cx).entries.get(&entry.name));
                        let title = entry
                            .display
                            .as_ref()
                            .map(|display| display.label(&rust_i18n::locale()).to_owned())
                            .or_else(|| info.map(metadata::title))
                            .unwrap_or_else(|| entry.name.clone());
                        let icon = crate::plugins::emblem::grid(
                            &entry.name,
                            entry.tint,
                            entry
                                .display
                                .as_ref()
                                .and_then(|display| display.icon.as_ref())
                                .or_else(|| info.and_then(crate::plugins::emblem::glyph)),
                            entry
                                .icon
                                .as_deref()
                                .or_else(|| info.and_then(|info| info.icon.as_deref())),
                            px(40.),
                        );
                        let summary = card::summary(
                            format!("market-details-{}", entry.id),
                            title.into(),
                            entry
                                .description(&rust_i18n::locale())
                                .unwrap_or_default()
                                .to_owned(),
                            Some(icon),
                            cx,
                        )
                        .disabled(!connected)
                        .on_click(cx.listener({
                            let entry = entry.clone();
                            move |state, _, window, cx| state.details(entry.clone(), window, cx)
                        }));
                        let action = card::action(
                            format!("market-action-{}", entry.id),
                            known,
                            update,
                            cx,
                        )
                        .disabled(busy || known && !update)
                        .loading(
                            matches!(&self.pending,Some(Loading::Repository(id)) if *id==entry.id),
                        )
                        .on_click(cx.listener({
                            let entry = entry.clone();
                            let source = self.source;
                            move |state, _, window, cx| {
                                cx.stop_propagation();
                                if update {
                                    if let Some(owner) = state.owner.upgrade() {
                                        owner.update(cx, |owner, cx| {
                                            owner.update_plugins(Some(&entry.name), window, cx)
                                        });
                                    }
                                } else {
                                    state.install(source, entry.clone(), window, cx);
                                }
                            }
                        }));
                        card::row(format!("market-card-{}", entry.id), summary, action, cx)
                    })),
            )
            .when(page.pages > 1, |view| {
                view.child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("market-previous")
                                .ghost()
                                .small()
                                .icon(IconName::ChevronLeft)
                                .accessibility_label(tr("plugins_previous"))
                                .tooltip(tr("plugins_previous"))
                                .disabled(busy || page.page <= 1)
                                .on_click(cx.listener(move |state, _, _, cx| {
                                    state.search(previous, false, cx)
                                })),
                        )
                        .child(
                            Button::new("market-next")
                                .ghost()
                                .small()
                                .icon(IconName::ChevronRight)
                                .accessibility_label(tr("plugins_next"))
                                .tooltip(tr("plugins_next"))
                                .disabled(busy || page.page >= page.pages)
                                .on_click(cx.listener(move |state, _, _, cx| {
                                    state.search(next, false, cx)
                                })),
                        ),
                )
            })
            .into_any_element()
    }
}
