use super::*;
use crate::theme::{self, market as service};
use std::sync::Arc;

struct Details {
    market: WeakEntity<Market>,
    id: String,
    entry: Option<service::Entry>,
    selection: Option<service::Selection>,
    service: Option<Arc<service::Service>>,
    preview: Option<theme::Package>,
    loading: bool,
    error: Option<&'static str>,
    stop: CancellationToken,
    task: Option<Task<()>>,
    scroll: ScrollHandle,
}

impl Drop for Details {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(in crate::settings) fn open(
    market: Entity<Market>,
    id: String,
    window: &mut Window,
    cx: &mut App,
) {
    let state = market.read(cx);
    let entry = state
        .themes
        .listing
        .as_ref()
        .and_then(|listing| listing.catalog.themes.iter().find(|entry| entry.id == id))
        .cloned();
    let selection = state
        .themes
        .listing
        .as_ref()
        .filter(|listing| !listing.unavailable)
        .and_then(|listing| listing.selection(&id));
    let service = state.themes.service.clone();
    let preview = cx
        .global::<theme::Catalog>()
        .packages
        .iter()
        .find(|package| package.id == id)
        .cloned();
    let details = cx.new(|cx| {
        crate::feedback::observe(window, cx, |state: &Details, _| {
            state.error.into_iter().collect()
        });
        cx.observe(&market, |_, _, cx| cx.notify()).detach();
        Details {
            market: market.downgrade(),
            id,
            entry,
            selection,
            service,
            preview,
            loading: false,
            error: None,
            stop: CancellationToken::new(),
            task: None,
            scroll: ScrollHandle::new(),
        }
    });
    details.update(cx, |state, cx| state.load(cx));
    window.dispatch_action(
        Box::new(super::super::super::OpenDetails(details.into())),
        cx,
    );
}

impl Details {
    fn load(&mut self, cx: &mut Context<Self>) {
        if self.preview.is_some() {
            return;
        }
        let (Some(service), Some(selection), Some(market)) = (
            self.service.clone(),
            self.selection.clone(),
            self.market.upgrade(),
        ) else {
            return;
        };
        self.stop.cancel();
        self.stop = CancellationToken::new();
        self.loading = true;
        self.error = None;
        let stop = self.stop.clone();
        let cancelled = stop.clone();
        let job = market
            .read(cx)
            .binding
            .runtime
            .spawn(async move { service.download(selection, &cancelled).await });
        self.task = Some(cx.spawn(async move |state, cx| {
            let result = job.await;
            _ = state.update(cx, |state, cx| {
                if stop.is_cancelled() {
                    return;
                }
                state.loading = false;
                match result {
                    Ok(Ok(prepared)) => {
                        state.preview = Some(theme::Package::preview(&prepared.loaded))
                    }
                    _ => state.error = Some("appearance_load_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn submit(&self, installed: bool, update: bool, window: &mut Window, cx: &mut Context<Self>) {
        if installed && !update {
            super::super::themes::apply(&self.id, window, cx);
        } else if let Some(selection) = self.selection.clone()
            && let Some(market) = self.market.upgrade()
        {
            market.update(cx, |market, cx| market.install_theme(selection, window, cx));
        }
    }
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let catalog = cx.global::<theme::Catalog>();
        let installed = catalog
            .packages
            .iter()
            .find(|package| package.id == self.id);
        let package = installed.or(self.preview.as_ref());
        let manifest = package.and_then(|package| package.manifest.as_ref());
        let current = catalog.packages[catalog.selected].id == self.id;
        let market = self.market.upgrade();
        let update = self.entry.as_ref().is_some_and(|entry| {
            market
                .as_ref()
                .and_then(|market| market.read(cx).themes.installed.get(&self.id))
                .is_some_and(|installed| entry.update_available(installed))
        });
        let pending = market
            .as_ref()
            .is_some_and(|market| market.read(cx).pending.is_some());
        let has_package = installed.is_some();
        let action = Button::new("theme-details-action")
            .small()
            .primary()
            .label(tr(if update {
                "market_update"
            } else if current {
                "market_theme_current"
            } else if has_package {
                "market_theme_apply"
            } else {
                "market_install"
            }))
            .disabled(
                pending
                    || current && !update
                    || (!has_package || update) && self.selection.is_none(),
            )
            .on_click(cx.listener(move |state, _, window, cx| {
                state.submit(has_package, update, window, cx)
            }));
        let version = self
            .entry
            .as_ref()
            .map(|entry| entry.package_version.as_str())
            .or_else(|| manifest.and_then(|manifest| manifest.package_version.as_deref()));
        let author = self.entry.as_ref().map(|entry| entry.author.as_str());
        let byline = author
            .into_iter()
            .chain(version)
            .collect::<Vec<_>>()
            .join(" · ");
        let title = self
            .entry
            .as_ref()
            .map(|entry| entry.name.clone().into())
            .or_else(|| package.map(|package| package.name.clone()))
            .unwrap_or_else(|| self.id.clone().into());
        let description = self
            .entry
            .as_ref()
            .and_then(|entry| entry.description(&rust_i18n::locale()).map(str::to_owned))
            .or_else(|| manifest.and_then(|manifest| manifest.description.clone()));
        let summary = super::summary(title, byline, description, None, Some(action), cx);
        let mut content = v_flex().w_full().min_w_0().gap_5();
        if let Some(package) = package {
            content = content.child(
                div()
                    .relative()
                    .w_full()
                    .aspect_ratio(16. / 10.)
                    .overflow_hidden()
                    .rounded(cx.theme().radius_lg)
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .child(theme::preview(package, cx.theme().is_dark(), cx).h_full()),
                    ),
            );
        } else if self.error.is_some() {
            content = content.child(
                crate::feedback::retry("theme-details-retry")
                    .on_click(cx.listener(|state, _, _, cx| state.load(cx))),
            );
        } else if self.loading {
            content = content.child(Skeleton::new().w_full().h_48());
        }
        let mut information = vec![
            rows::row(
                "theme-source".into(),
                IconName::Link,
                tr("market_details_source"),
                None,
                tr(if self.id == "builtin" {
                    "market_theme_builtin"
                } else if self.entry.is_some() {
                    "market_details_source_official"
                } else {
                    "market_details_source_local"
                }),
                cx,
            )
            .into_any_element(),
        ];
        if let Some(version) = version {
            information.push(
                rows::row(
                    "theme-version".into(),
                    IconName::Tag,
                    tr("market_details_version"),
                    None,
                    version.to_owned(),
                    cx,
                )
                .into_any_element(),
            );
        }
        if let Some(license) = self
            .entry
            .as_ref()
            .map(|entry| entry.license.as_str())
            .or_else(|| manifest.and_then(|manifest| manifest.license.name.as_deref()))
        {
            information.push(
                rows::row(
                    "theme-license".into(),
                    IconName::Tag,
                    tr("settings_license"),
                    None,
                    license.to_owned(),
                    cx,
                )
                .into_any_element(),
            );
        }
        let homepage = self
            .entry
            .as_ref()
            .and_then(|entry| entry.homepage.as_ref())
            .or_else(|| manifest.and_then(|manifest| manifest.homepage.as_ref()));
        if let Some(link) = homepage {
            let link = link.clone();
            information.push(
                Button::new("theme-homepage")
                    .ghost()
                    .small()
                    .label(tr("appearance_homepage"))
                    .on_click(move |_, _, cx| cx.open_url(&link))
                    .into_any_element(),
            );
        }
        content = content.child(rows::section(
            "market_details_information",
            None,
            information,
            cx,
        ));
        if let Some(package) = package
            && !package.images.is_empty()
        {
            let resources = package
                .images
                .keys()
                .map(|path| {
                    rows::row(
                        format!("theme-resource-{path}"),
                        IconName::BookOpen,
                        path.clone(),
                        None,
                        div(),
                        cx,
                    )
                    .into_any_element()
                })
                .collect();
            content = content.child(rows::section(
                "market_details_contents",
                Some(package.images.len()),
                resources,
                cx,
            ));
        }
        super::panel(summary, content.into_any_element(), &self.scroll, cx)
    }
}
