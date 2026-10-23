use super::*;
use crate::theme::{self, market as service};
use std::{collections::BTreeMap, sync::Arc};

const PREVIEW_RATIO: f32 = 5. / 2.;

pub(super) struct State {
    columns: u16,
    pub(super) service: Option<Arc<service::Service>>,
    pub(super) listing: Option<service::Listing>,
    pub(super) updates: Entity<crate::settings::appearance::updates::State>,
    previews: BTreeMap<(String, bool), Arc<Image>>,
    selected: Selection,
}

#[derive(Default)]
struct Selection {
    opened: Option<(String, EntityId)>,
}

impl Selection {
    fn select(&mut self, id: String, owner: EntityId) {
        self.opened = Some((id, owner));
    }

    fn contains(&self, id: &str) -> bool {
        self.opened
            .as_ref()
            .is_some_and(|(selected, _)| selected == id)
    }

    fn close(&mut self, owner: EntityId) -> bool {
        if self
            .opened
            .as_ref()
            .is_some_and(|(_, current)| *current == owner)
        {
            self.opened = None;
            true
        } else {
            false
        }
    }
}

impl State {
    pub fn new(updates: Entity<crate::settings::appearance::updates::State>, cx: &App) -> Self {
        Self {
            columns: 3,
            service: updates.read(cx).service.clone(),
            updates,
            listing: None,
            previews: BTreeMap::new(),
            selected: Default::default(),
        }
    }
}

fn open(
    market: Entity<Market>,
    id: String,
    stop: CancellationToken,
    window: &mut Window,
    cx: &mut App,
) {
    if stop.is_cancelled() {
        return;
    }
    let (source, updates) = {
        let state = market.read(cx);
        (
            state
                .themes
                .listing
                .as_ref()
                .filter(|listing| !listing.unavailable)
                .and_then(|listing| listing.selection(&id)),
            state.themes.updates.clone(),
        )
    };
    let package = cx
        .global::<theme::Catalog>()
        .packages
        .iter()
        .find(|package| package.id == id)
        .cloned();
    let Some(view) =
        details::themes::open(market.clone(), package, source, updates, stop, window, cx)
    else {
        return;
    };
    let owner = view.entity_id();
    market.update(cx, |market, cx| {
        market.themes.selected.select(id, owner);
        cx.observe_release(&view, move |market, _, cx| {
            if market.themes.selected.close(owner) {
                cx.notify();
            }
        })
        .detach();
        cx.notify();
    });
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
            return;
        }
        self.themes
            .updates
            .update(cx, |state, cx| state.install(selection, window, cx));
    }

    pub(super) fn theme_cards(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let catalog = cx.global::<theme::Catalog>();
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
        let mut cards = Vec::new();
        for entry in entries {
            let package = catalog
                .packages
                .iter()
                .find(|package| package.id == entry.id);
            let installed = package.is_some();
            let current = self.themes.selected.contains(&entry.id);
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
            let action = card::action(
                format!("market-theme-action-{id}"),
                card::Action::new(installed, false),
                cx,
            )
            .size_6()
            .disabled(installed || pending || selection.is_none())
            .loading(pending && !installed)
            .on_click(cx.listener(move |state, _, window, cx| {
                cx.stop_propagation();
                if installed {
                    return;
                }
                if let Some(selection) = selection.clone() {
                    state.install_theme(selection, window, cx);
                }
            }));
            cards.push(Self::theme_card(
                entry.id.clone(),
                entry.name.clone().into(),
                preview,
                action,
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
        preview: AnyElement,
        action: Button,
        selected: bool,
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
            .border_color(if selected {
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
                            .aspect_ratio(PREVIEW_RATIO)
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
                            .p_3()
                            .gap_2()
                            .items_center()
                            .child(div().flex_1().min_w_0().text_sm().truncate().child(name))
                            .child(action.flex_shrink_0()),
                    ),
            )
            .on_click(cx.listener(move |state, _, window, cx| {
                let selection = state.begin_details();
                let market = cx.entity();
                let target = target.clone();
                window.defer(cx, move |window, cx| {
                    open(market, target, selection, window, cx)
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

    #[test]
    fn pane_identity_owns_selection() {
        let mut selection = Selection::default();
        let first = EntityId::from(1);
        let second = EntityId::from(2);
        assert!(!selection.contains("default"));
        selection.select("ink".into(), first);
        assert!(selection.contains("ink"));
        selection.select("paper".into(), second);
        assert!(!selection.contains("ink"));
        assert!(!selection.close(first));
        assert!(selection.contains("paper"));
        assert!(selection.close(second));
        assert!(!selection.contains("paper"));
        assert!(!selection.close(second));
    }
}
