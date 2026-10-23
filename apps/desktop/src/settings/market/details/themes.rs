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
    selection_stop: CancellationToken,
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
    selection_stop: CancellationToken,
    window: &mut Window,
    cx: &mut App,
) {
    if selection_stop.is_cancelled() {
        return;
    }
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
            selection_stop,
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
        if self.selection_stop.is_cancelled() {
            return;
        }
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
        self.stop = self.selection_stop.child_token();
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

    fn submit(&self, installed: bool, window: &mut Window, cx: &mut Context<Self>) {
        if installed {
            window.dispatch_action(Box::new(crate::settings::OpenTheme(self.id.clone())), cx);
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
        let market = self.market.upgrade();
        let pending = market
            .as_ref()
            .is_some_and(|market| market.read(cx).themes.updates.read(cx).pending(&self.id));
        let has_package = installed.is_some();
        let action = Button::new("theme-details-action")
            .small()
            .primary()
            .label(tr(if has_package {
                "market_manage"
            } else {
                "market_install"
            }))
            .disabled(!has_package && (pending || self.selection.is_none()))
            .on_click(
                cx.listener(move |state, _, window, cx| state.submit(has_package, window, cx)),
            );
        let version = has_package
            .then(|| manifest.and_then(|manifest| manifest.package_version.as_deref()))
            .flatten()
            .or_else(|| {
                self.entry
                    .as_ref()
                    .map(|entry| entry.package_version.as_str())
            });
        let author = self.entry.as_ref().map(|entry| entry.author.as_str());
        let mut byline = author
            .into_iter()
            .chain(version)
            .collect::<Vec<_>>()
            .join(" · ");
        if has_package {
            if !byline.is_empty() {
                byline.push_str(" · ");
            }
            byline.push_str(&tr("market_installed"));
        }
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
            information.push(rows::homepage("theme-homepage", link, cx));
        }
        let information = rows::information(information, cx);

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
        super::panel(
            summary,
            information,
            content.into_any_element(),
            &self.scroll,
            cx,
        )
    }
}
