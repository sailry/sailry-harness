use super::*;
use crate::theme::{self, market as service};
use std::{collections::BTreeMap, sync::Arc};

pub(super) struct State {
    columns: u16,
    pub(super) service: Option<Arc<service::Service>>,
    pub(super) listing: Option<service::Listing>,
    pub(super) installed: BTreeMap<String, service::Installed>,
    previews: BTreeMap<(String, bool), Arc<Image>>,
}

impl State {
    pub fn new(cx: &App) -> Self {
        Self {
            columns: 3,
            service: cx
                .global::<theme::Catalog>()
                .root
                .clone()
                .and_then(|root| service::Service::new(root).ok())
                .map(Arc::new),
            listing: None,
            installed: BTreeMap::new(),
            previews: BTreeMap::new(),
        }
    }
}

fn image(bytes: Vec<u8>) -> Option<Arc<Image>> {
    let format = match image::guess_format(&bytes).ok()? {
        image::ImageFormat::Png => ImageFormat::Png,
        image::ImageFormat::Jpeg => ImageFormat::Jpeg,
        image::ImageFormat::WebP => ImageFormat::Webp,
        _ => return None,
    };
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

impl Market {
    pub(super) fn load_themes(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(service) = self.themes.service.clone() else {
            cx.notify();
            return;
        };
        let stop = self.begin(Loading::Catalog);
        let cancelled = stop.clone();
        let job = self.binding.runtime.spawn(async move {
            let listing = service.list(force, &cancelled).await?;
            let mut installed = BTreeMap::new();
            let mut previews = Vec::new();
            for entry in &listing.catalog.themes {
                if cancelled.is_cancelled() {
                    return Err(service::Failure::Cancelled);
                }
                if let Some(value) = service.installed(&entry.id).await? {
                    installed.insert(entry.id.clone(), value);
                }
                if let Some(selection) = listing.selection(&entry.id) {
                    for dark in [false, true] {
                        if let Ok(Some(bytes)) = service
                            .preview(&selection, if dark { "dark" } else { "light" }, &cancelled)
                            .await
                        {
                            previews.push((entry.id.clone(), dark, bytes));
                        }
                    }
                }
            }
            Ok((listing, installed, previews))
        });
        self.task = Some(cx.spawn(async move |state, cx| {
            let result = job.await;
            _ = state.update(cx, |state, cx| {
                if stop.is_cancelled() {
                    return;
                }
                state.pending = None;
                match result {
                    Ok(Ok((listing, installed, previews))) => {
                        state.error = listing.unavailable.then_some("market_themes_failed");
                        state.paging.failed = listing.unavailable.then_some(1);
                        state.themes.listing = Some(listing);
                        state.themes.installed = installed;
                        for (id, dark, bytes) in previews {
                            if let Some(image) = image(bytes) {
                                state.themes.previews.insert((id, dark), image);
                            }
                        }
                    }
                    _ => {
                        state.error = Some("market_themes_failed");
                        state.paging.failed = Some(1);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn install_theme(
        &mut self,
        selection: service::Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() {
            return;
        }
        let Some(service) = self.themes.service.clone() else {
            return;
        };
        let id = selection.entry.id.clone();
        let stop = self.begin(Loading::Theme(id));
        let cancelled = stop.clone();
        let downloader = service.clone();
        let job = self
            .binding
            .runtime
            .spawn(async move { downloader.download(selection, &cancelled).await });
        self.task = Some(cx.spawn_in(window, async move |state, cx| {
            let result = job.await;
            _ = state.update_in(cx, |state, window, cx| {
                if stop.is_cancelled() {
                    return;
                }
                match result {
                    Ok(Ok(prepared)) => {
                        if let Err(error) = cx.global::<theme::Catalog>().check(&prepared.loaded) {
                            eprintln!("could not install theme: {error}");
                            state.pending = None;
                            state.error = Some("appearance_import_conflict");
                            cx.notify();
                            return;
                        }
                        state.publish_theme(service, prepared, stop, window, cx);
                    }
                    _ => {
                        state.pending = None;
                        state.error = Some("appearance_load_failed");
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }

    fn publish_theme(
        &mut self,
        service: Arc<service::Service>,
        prepared: service::Prepared,
        stop: CancellationToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cancelled = stop.clone();
        let job = self
            .binding
            .runtime
            .spawn(async move { service.install(prepared, &stop).await });
        let origin = window.window_handle();
        // Publication belongs to the controller Catalog even if this view is closed.
        cx.spawn(async move |state, cx| {
            let result = job.await;
            let feedback = cx.update(|cx| {
                _ = state.update(cx, |state, cx| {
                    if !cancelled.is_cancelled() {
                        state.pending = None;
                        cx.notify();
                    }
                });
                match result {
                    Ok(Ok(result)) => {
                        let id = result.loaded.manifest.id.clone();
                        let registered = cx.update_global::<theme::Catalog, _>(|catalog, _| {
                            let active = catalog.packages[catalog.selected].id == id;
                            catalog.add(result.loaded)?;
                            catalog.error = None;
                            Ok::<_, String>(
                                active.then(|| catalog.packages[catalog.selected].clone()),
                            )
                        });
                        match registered {
                            Ok(active) => {
                                _ = state.update(cx, |state, cx| {
                                    state.themes.installed.insert(id, result.installed);
                                    cx.notify();
                                });
                                if let Some(package) = active {
                                    theme::apply_colors(package, None, cx);
                                }
                                cx.refresh_windows();
                                Some(("appearance_import_done", false))
                            }
                            Err(error) => {
                                eprintln!("could not register installed theme: {error}");
                                Some(("appearance_load_failed", true))
                            }
                        }
                    }
                    Ok(Err(service::Failure::Cancelled)) => None,
                    Ok(Err(error)) => {
                        eprintln!("could not publish theme: {error}");
                        Some((
                            if matches!(error, service::Failure::Publication { .. }) {
                                "appearance_save_uncertain"
                            } else {
                                "appearance_save_failed"
                            },
                            true,
                        ))
                    }
                    Err(_) => Some(("appearance_save_uncertain", true)),
                }
            });
            if let Some((key, failed)) = feedback
                && let Some(target) = cx.update(|cx| crate::installations::target(origin, cx))
            {
                _ = target.update(cx, |_, window, cx| {
                    let message = tr(key);
                    let notification = if failed {
                        gpui_kit::component::notification::Notification::error(message.clone())
                    } else {
                        gpui_kit::component::notification::Notification::success(message.clone())
                    };
                    crate::feedback::toast(window, message, notification, cx);
                });
            }
        })
        .detach();
    }

    pub(super) fn theme_cards(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let catalog = cx.global::<theme::Catalog>();
        let selected = &catalog.packages[catalog.selected].id;
        let query = self.query.read(cx).value().trim().to_lowercase();
        let entries: Vec<_> = self
            .themes
            .listing
            .iter()
            .flat_map(|listing| &listing.catalog.themes)
            .filter(|entry| {
                self.filter.theme(
                    &entry.modes,
                    catalog
                        .packages
                        .iter()
                        .any(|package| package.id == entry.id),
                )
            })
            .filter(|entry| {
                query.is_empty()
                    || entry.name.to_lowercase().contains(&query)
                    || entry
                        .description(&rust_i18n::locale())
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(&query)
            })
            .collect();
        let locals: Vec<_> = catalog
            .packages
            .iter()
            .filter(|package| {
                !self.themes.listing.as_ref().is_some_and(|listing| {
                    listing
                        .catalog
                        .themes
                        .iter()
                        .any(|entry| entry.id == package.id)
                }) && (query.is_empty() || package.name.to_lowercase().contains(&query))
            })
            .collect();
        let mut cards = Vec::new();
        for package in locals {
            let id = package.id.clone();
            let current = *selected == id;
            cards.push(
                self.theme_card(
                    id.clone(),
                    package.name.clone(),
                    if id == "builtin" {
                        tr("market_theme_builtin").to_string()
                    } else {
                        String::new()
                    },
                    theme::preview(package, cx.theme().is_dark(), cx)
                        .h_full()
                        .into_any_element(),
                    Button::new(SharedString::from(format!("market-theme-apply-{id}")))
                        .small()
                        .label(tr(if current {
                            "market_theme_current"
                        } else {
                            "market_theme_apply"
                        }))
                        .disabled(current)
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            apply(&id, window, cx)
                        }),
                    cx,
                ),
            );
        }
        for entry in entries {
            let package = catalog
                .packages
                .iter()
                .find(|package| package.id == entry.id);
            let installed = package.is_some();
            let update = self
                .themes
                .installed
                .get(&entry.id)
                .is_some_and(|installed| entry.update_available(installed));
            let current = *selected == entry.id;
            let selection = self
                .themes
                .listing
                .as_ref()
                .filter(|listing| !listing.unavailable)
                .and_then(|listing| listing.selection(&entry.id));
            let preview = if let Some(package) = package {
                theme::preview(package, cx.theme().is_dark(), cx)
                    .h_full()
                    .into_any_element()
            } else if let Some(image) = self
                .themes
                .previews
                .get(&(entry.id.clone(), cx.theme().is_dark()))
            {
                img(image.clone())
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element()
            } else {
                Skeleton::new().size_full().into_any_element()
            };
            let id = entry.id.clone();
            let key = if update {
                "market_update"
            } else if current {
                "market_theme_current"
            } else if installed {
                "market_theme_apply"
            } else {
                "market_install"
            };
            let action = Button::new(SharedString::from(format!("market-theme-action-{id}")))
                .small()
                .label(tr(key))
                .disabled(
                    self.pending.is_some()
                        || current && !update
                        || (!installed || update) && selection.is_none(),
                )
                .loading(matches!(&self.pending, Some(Loading::Theme(value)) if *value == id))
                .on_click(cx.listener(move |state, _, window, cx| {
                    cx.stop_propagation();
                    if installed && !update {
                        apply(&id, window, cx);
                    } else if let Some(selection) = selection.clone() {
                        state.install_theme(selection, window, cx);
                    }
                }));
            cards.push(
                self.theme_card(
                    entry.id.clone(),
                    entry.name.clone().into(),
                    entry
                        .description(&rust_i18n::locale())
                        .unwrap_or_default()
                        .to_owned(),
                    preview,
                    action,
                    cx,
                ),
            );
        }
        self.paging.count(cards.len());
        let columns = self.themes.columns;
        let owner = cx.weak_entity();
        div()
            .w_full()
            .on_children_prepainted(move |bounds, window, cx| {
                let Some(bounds) = bounds.first() else {
                    return;
                };
                let count = column_count(bounds.size.width);
                if count != columns {
                    let owner = owner.clone();
                    window.defer(cx, move |_, cx| {
                        _ = owner.update(cx, |state, cx| {
                            if state.themes.columns != count {
                                state.themes.columns = count;
                                cx.notify();
                            }
                        });
                    });
                }
            })
            .child(
                div()
                    .w_full()
                    .grid()
                    .grid_cols(columns)
                    .items_start()
                    .content_start()
                    .gap_4()
                    .when(cards.is_empty(), |view| view.child(self.empty(cx)))
                    .children(cards.into_iter().take(self.paging.limit)),
            )
            .into_any_element()
    }

    fn theme_card(
        &self,
        id: String,
        name: SharedString,
        description: String,
        preview: AnyElement,
        action: Button,
        cx: &Context<Self>,
    ) -> AnyElement {
        let target = id.clone();
        list::ListItem::new(SharedString::from(format!("market-theme-{id}")))
            .w_full()
            .h_auto()
            .min_w_0()
            .p_0()
            .rounded(cx.theme().radius_lg)
            .accessibility_label(name.clone())
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .gap(px(10.))
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .aspect_ratio(16. / 10.)
                            .flex_shrink_0()
                            .overflow_hidden()
                            .rounded(cx.theme().radius_lg)
                            .border_1()
                            .border_color(cx.theme().border)
                            .child(div().absolute().inset_0().child(preview)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .px_0p5()
                            .gap_3()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(div().text_sm().truncate().font_medium().child(name))
                                    .when(!description.is_empty(), |view| {
                                        view.child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .truncate()
                                                .child(description),
                                        )
                                    }),
                            )
                            .child(action.flex_shrink_0()),
                    ),
            )
            .on_click(cx.listener(move |_, _, window, cx| {
                details::themes::open(cx.entity(), target.clone(), window, cx)
            }))
            .into_any_element()
    }
}

pub(super) fn apply(id: &str, window: &mut Window, cx: &mut App) {
    if let Some(index) = cx
        .global::<theme::Catalog>()
        .packages
        .iter()
        .position(|package| package.id == id)
    {
        theme::apply(index, Some(window), cx);
    }
}

fn column_count(width: Pixels) -> u16 {
    const CARD_WIDTH: f32 = 240.;
    const GAP: f32 = 16.;
    (((f32::from(width) + GAP) / (CARD_WIDTH + GAP)).floor() as u16).clamp(1, 3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn adapts_to_available_width() {
        for (width, expected) in [
            (960., 3),
            (1600., 3),
            (752., 3),
            (744., 2),
            (496., 2),
            (480., 1),
        ] {
            assert_eq!(column_count(px(width)), expected);
        }
    }
}
