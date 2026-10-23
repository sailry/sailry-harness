use super::*;
use crate::theme::{self, market as service};
use std::{collections::BTreeMap, sync::Arc};

pub(super) struct State {
    columns: u16,
    pub(super) service: Option<Arc<service::Service>>,
    pub(super) listing: Option<service::Listing>,
    pub(super) updates: Entity<crate::settings::appearance::updates::State>,
    previews: BTreeMap<(String, bool), Arc<Image>>,
}

impl State {
    pub fn new(updates: Entity<crate::settings::appearance::updates::State>, cx: &App) -> Self {
        Self {
            columns: 3,
            service: updates.read(cx).service.clone(),
            updates,
            listing: None,
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
            let requests = listing
                .catalog
                .themes
                .iter()
                .filter_map(|entry| listing.selection(&entry.id))
                .flat_map(|selection| [false, true].map(|dark| (selection.clone(), dark)))
                .map(|(selection, dark)| {
                    let service = service.clone();
                    let stop = cancelled.clone();
                    async move {
                        service
                            .preview(&selection, if dark { "dark" } else { "light" }, &stop)
                            .await
                            .ok()
                            .flatten()
                            .map(|bytes| (selection.entry.id, dark, bytes))
                    }
                });
            let previews = futures::future::join_all(requests)
                .await
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            if cancelled.is_cancelled() {
                return Err(service::Failure::Cancelled);
            }
            Ok((listing, previews))
        });
        self.task = Some(cx.spawn(async move |state, cx| {
            let result = job.await;
            _ = state.update(cx, |state, cx| {
                if stop.is_cancelled() {
                    return;
                }
                state.pending = None;
                match result {
                    Ok(Ok((listing, previews))) => {
                        state.error = listing.unavailable.then_some("market_themes_failed");
                        state.paging.failed = listing.unavailable.then_some(1);
                        state.themes.listing = Some(listing);
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
        if cx
            .global::<theme::Catalog>()
            .packages
            .iter()
            .any(|package| package.id == selection.entry.id)
        {
            window.dispatch_action(Box::new(crate::settings::OpenTheme(selection.entry.id)), cx);
            return;
        }
        self.themes
            .updates
            .update(cx, |state, cx| state.install(selection, window, cx));
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
            cards.push(Self::theme_card(
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
                    .label(tr("market_manage"))
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        window
                            .dispatch_action(Box::new(crate::settings::OpenTheme(id.clone())), cx);
                    }),
                true,
                current,
                cx,
            ));
        }
        for entry in entries {
            let package = catalog
                .packages
                .iter()
                .find(|package| package.id == entry.id);
            let installed = package.is_some();
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
                Skeleton::new()
                    .size_full()
                    .rounded_none()
                    .into_any_element()
            };
            let id = entry.id.clone();
            let pending = self.themes.updates.read(cx).pending(&id);
            let action = Button::new(SharedString::from(format!("market-theme-action-{id}")))
                .small()
                .label(tr(if installed {
                    "market_manage"
                } else {
                    "market_install"
                }))
                .disabled(!installed && (pending || selection.is_none()))
                .loading(pending)
                .on_click(cx.listener(move |state, _, window, cx| {
                    cx.stop_propagation();
                    if installed {
                        window
                            .dispatch_action(Box::new(crate::settings::OpenTheme(id.clone())), cx);
                    } else if let Some(selection) = selection.clone() {
                        state.install_theme(selection, window, cx);
                    }
                }));
            cards.push(Self::theme_card(
                entry.id.clone(),
                entry.name.clone().into(),
                entry
                    .description(&rust_i18n::locale())
                    .unwrap_or_default()
                    .to_owned(),
                preview,
                action,
                installed,
                current,
                cx,
            ));
        }
        self.paging.count(cards.len());
        let columns = self.themes.columns;
        let owner = cx.weak_entity();
        div()
            .w_full()
            .when(cards.is_empty(), |view| view.child(self.empty(cx)))
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
                    .children(cards.into_iter().take(self.paging.limit)),
            )
            .into_any_element()
    }

    fn theme_card(
        id: String,
        name: SharedString,
        description: String,
        preview: AnyElement,
        action: Button,
        installed: bool,
        current: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let target = id.clone();
        list::ListItem::new(SharedString::from(format!("market-theme-{id}")))
            .w_full()
            .h_auto()
            .min_w_0()
            .p_0()
            .overflow_hidden()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(if current {
                cx.theme().primary
            } else {
                cx.theme().border
            })
            .bg(cx.theme().group_box)
            .accessibility_label(name.clone())
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .aspect_ratio(16. / 10.)
                            .flex_shrink_0()
                            .overflow_hidden()
                            .rounded_t(cx.theme().radius_lg)
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(div().absolute().inset_0().child(preview)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .py_3()
                            .px(px(14.))
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
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(card::status(installed))
                                    .child(action.flex_shrink_0()),
                            ),
                    ),
            )
            .on_click(cx.listener(move |state, _, window, cx| {
                let selection = state.begin_details();
                let market = cx.entity();
                let target = target.clone();
                window.defer(cx, move |window, cx| {
                    details::themes::open(market, target, selection, window, cx)
                });
            }))
            .into_any_element()
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
