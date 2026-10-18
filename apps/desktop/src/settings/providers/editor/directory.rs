//! Directory selection changes a draft; it never saves or authenticates it.
use super::super::presets;
use super::*;
use gpui_kit::component::{
    menu::{DropdownMenu, PopupMenuItem},
    tag::Tag,
};
use sailry_protocol::{
    Command, Output,
    conversation::presets::{Entry, Models, PresetRef, Provenance, Route},
};

impl Editor {
    pub(super) fn authentication(&self) -> sailry_protocol::Authentication {
        use sailry_protocol::{Authentication, conversation::presets::Auth};
        self.selected.as_ref().map_or_else(
            || {
                self.provider
                    .as_ref()
                    .filter(|provider| provider.authentication == Authentication::Plugin)
                    .map_or(self.preset.authentication(), |provider| {
                        provider.authentication
                    })
            },
            |entry| match entry.preset.auth {
                Auth::Builtin { authentication } => authentication,
                Auth::Plugin { .. } => Authentication::Plugin,
                _ => Authentication::ApiKey,
            },
        )
    }

    pub(super) fn uses_login(&self) -> bool {
        self.authentication() != sailry_protocol::Authentication::ApiKey
    }

    pub(super) fn uses_key(&self) -> bool {
        self.selected
            .as_ref()
            .map_or(self.preset.key_auth() && !self.uses_login(), |entry| {
                matches!(
                    entry.preset.auth,
                    sailry_protocol::conversation::presets::Auth::ApiKey { .. }
                )
            })
    }

    fn suggestions_active(&self) -> bool {
        !self.closed
            && self.editing.is_none()
            && self.flow.step == flow::Step::Configure
            && self.selected.as_ref().is_some_and(|entry| {
                entry.source == sailry_protocol::conversation::presets::Source::Builtin
                    && matches!(
                        entry.preset.id.as_str(),
                        "compatible-responses"
                            | "compatible-chat"
                            | "compatible-anthropic"
                            | "compatible-gemini"
                    )
            })
    }

    pub(super) fn load_directory(
        &mut self,
        refresh_partners: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if if refresh_partners {
            self.partners_requested || !self.suggestions_active()
        } else {
            self.directory_task.is_some()
        } {
            return;
        }
        let Some(binding) = self.binding.clone() else {
            return;
        };
        if refresh_partners {
            self.partners_requested = true;
            // An older local snapshot must not replace the refreshed directory.
            self.directory_task.take();
        }
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(
                    binding
                        .client
                        .prepare(Command::ListChannelPresets { refresh_partners }),
                )
                .await
        });
        let task = cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                if editor.closed {
                    return;
                }
                match result {
                    Ok(Ok(Output::ChannelPresets(listing))) => {
                        editor.directory = listing.entries;
                        // Resolving a label for an existing channel must preserve its draft.
                        if editor.selected.is_none() {
                            editor.selected = editor.provider.as_ref().and_then(|provider| {
                                let reference = provider.preset.as_ref()?;
                                editor
                                    .directory
                                    .iter()
                                    .find(|entry| {
                                        entry.source == reference.source
                                            && entry.preset.id == reference.id
                                    })
                                    .cloned()
                            });
                            editor.restore_consent();
                        }
                        if let Some(brand) = editor.flow.requested.take() {
                            if presets::groups(&editor.directory)
                                .iter()
                                .any(|(key, _)| *key == brand)
                            {
                                editor.choose_brand(brand, window, cx);
                            } else {
                                editor.report_error("provider_preset_unavailable", window, cx);
                            }
                        }
                        if refresh_partners
                            && listing.partners_unavailable
                            && editor.suggestions_active()
                        {
                            editor.report_error("provider_partners_unavailable", window, cx);
                        }
                    }
                    _ if refresh_partners && editor.suggestions_active() => {
                        editor.report_error("provider_partners_unavailable", window, cx)
                    }
                    _ if editor.flow.requested.take().is_some() => {
                        editor.report_error("provider_preset_unavailable", window, cx)
                    }
                    _ => {}
                }
                cx.notify();
            });
        });
        if refresh_partners {
            self.partner_task = Some(task);
        } else {
            self.directory_task = Some(task);
        }
    }

    pub(super) fn selected_route(&self) -> Option<&Route> {
        self.selected.as_ref()?.preset.routes.iter().find(|route| {
            self.selected_ref
                .as_ref()
                .and_then(|reference| reference.route.as_ref())
                == Some(&route.key())
        })
    }

    pub(super) fn preset_ref(&self) -> Option<PresetRef> {
        self.selected_ref.clone().or_else(|| {
            self.provider
                .as_ref()
                .and_then(|provider| provider.preset.clone())
        })
    }

    pub(super) fn choose_entry(
        &mut self,
        entry: Entry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation_blocked() || self.editing.is_some() {
            return;
        }
        let Some(route) = entry.preset.routes.first().cloned() else {
            return;
        };
        self.select_entry(entry, route, window, cx);
    }

    fn select_entry(
        &mut self,
        entry: Entry,
        route: Route,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation_blocked() || self.editing.is_some() {
            return;
        }
        let reference = presets::reference(&entry, &route);
        if self.selected_ref.as_ref() != Some(&reference) {
            self.select_preset(presets::kind(&entry, &route), window, cx);
            self.clear_route(window, cx);
            self.name.update(cx, |input, cx| {
                input.set_value(presets::label(&entry.preset), window, cx)
            });
            self.endpoint.update(cx, |input, cx| {
                input.set_value(route.endpoint.clone(), window, cx)
            });
        }
        self.flow.brand = Some(presets::brand(&entry));
        self.partner_key_url = entry.preset.key_url.clone();
        let preserve_consent = self.selected.as_ref().is_some_and(|previous| {
            previous.source == entry.source
                && previous.preset.id == entry.preset.id
                && previous.package == entry.package
                && previous.origin == entry.origin
        });
        self.selected_ref = Some(reference);
        self.selected = Some(entry);
        if !preserve_consent {
            self.restore_consent();
        }
        self.seed_directory(window, cx);
        self.flow.step = flow::Step::Configure;
        self.load_directory(true, window, cx);
        self.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    pub(super) fn change_route(
        &mut self,
        route: Route,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.selected.clone() else {
            return;
        };
        let route = route_target(&entry.preset.routes, self.selected_route(), route);
        if self.navigation_blocked()
            || self.editing.is_some()
            || self.selected_route() == Some(&route)
        {
            return;
        }
        self.select_preset(presets::kind(&entry, &route), window, cx);
        self.clear_route(window, cx);
        self.endpoint.update(cx, |input, cx| {
            input.set_value(route.endpoint.clone(), window, cx)
        });
        self.selected_ref = Some(presets::reference(&entry, &route));
        self.seed_directory(window, cx);
        cx.notify();
    }

    fn seed_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.models.is_empty() {
            return;
        }
        let Some(entry) = &self.selected else {
            return;
        };
        let mut ids = match &entry.preset.models {
            Models::Static { ids } => ids.clone(),
            Models::Discover => Vec::new(),
        };
        if let Some(product) = self.selected_route().and_then(|route| route.product)
            && let Some(id) = entry
                .preset
                .products
                .iter()
                .find(|item| item.kind == product)
                .and_then(|item| item.default_model.clone())
        {
            ids.insert(0, id);
        }
        let mut seen = HashSet::new();
        ids.retain(|id| seen.insert(id.clone()));
        for id in ids {
            self.add_model(window, cx);
            self.models.last_mut().unwrap().expanded = false;
            self.models
                .last()
                .unwrap()
                .id
                .update(cx, |input, cx| input.set_value(id, window, cx));
        }
    }

    pub(super) fn directory_icon(&self) -> AnyElement {
        self.selected.as_ref().map_or_else(
            || self.preset.category().icon().size_8().into_any_element(),
            |entry| presets::render_icon(entry, rems(2.)),
        )
    }

    pub(super) fn selection_name(&self) -> SharedString {
        self.selected
            .as_ref()
            .map_or_else(|| tr(self.preset.family().key()), super::entry::brand_label)
    }

    pub(super) fn method_label(&self) -> SharedString {
        self.selected.as_ref().map_or_else(
            || tr(self.preset.key()),
            |entry| presets::label(&entry.preset).into(),
        )
    }

    pub(super) fn suggestions(&self, cx: &Context<Self>) -> Option<Div> {
        if !self.suggestions_active() {
            return None;
        }
        let choices = partner_choices(&self.directory, self.selected_route()?);
        if choices.is_empty() {
            return None;
        }
        Some(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("provider_partners")),
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .children(choices.into_iter().enumerate().map(
                            |(index, (entry, route))| {
                                Button::new(("provider-suggestion", index))
                                    .small()
                                    .outline()
                                    .disabled(self.navigation_blocked())
                                    .child(presets::label(&entry.preset))
                                    .when(entry.provenance == Provenance::ThirdParty, |button| {
                                        button.child(
                                            Tag::secondary()
                                                .small()
                                                .child(tr("provider_third_party")),
                                        )
                                    })
                                    .on_click(cx.listener(move |editor, _, window, cx| {
                                        editor.select_entry(
                                            entry.clone(),
                                            route.clone(),
                                            window,
                                            cx,
                                        )
                                    }))
                            },
                        )),
                ),
        )
    }

    pub(super) fn source_label(&self) -> Option<Div> {
        let entry = self.selected.as_ref()?;
        if entry.provenance != Provenance::ThirdParty && entry.preset.badge.is_none() {
            return None;
        }
        let body = h_flex().gap_2();
        Some(
            body.when(entry.provenance == Provenance::ThirdParty, |body| {
                body.child(Tag::secondary().small().child(tr("provider_third_party")))
            })
            .when_some(entry.preset.badge.clone(), |body, badge| {
                body.child(Tag::secondary().small().child(badge))
            }),
        )
    }

    pub(super) fn route_field(&self, form: Form, region: bool, cx: &Context<Self>) -> Form {
        let Some(entry) = &self.selected else {
            return form;
        };
        let Some(current) = self.selected_route() else {
            return form;
        };
        let choices = route_choices(&entry.preset.routes, current, region);
        if choices.len() < 2 {
            return form;
        }
        let key = if region {
            "provider_region"
        } else {
            "provider_product"
        };
        let label = route_label(current, region);
        let owner = cx.entity();
        form.child(
            Field::new().label(tr(key)).child(
                Button::new(key)
                    .w_full()
                    .label(label)
                    .dropdown_caret(true)
                    .disabled(self.navigation_blocked() || self.editing.is_some())
                    .dropdown_menu(move |menu, _, _| {
                        choices.iter().fold(menu, |menu, route| {
                            let route = route.clone();
                            let owner = owner.clone();
                            menu.item(PopupMenuItem::new(route_label(&route, region)).on_click(
                                move |_, window, cx| {
                                    owner.update(cx, |editor, cx| {
                                        editor.change_route(route.clone(), window, cx)
                                    })
                                },
                            ))
                        })
                    }),
            ),
        )
    }
}

fn route_label(route: &Route, region: bool) -> SharedString {
    use sailry_protocol::conversation::{connection::Product, presets::Region};
    tr(if region {
        match route.region {
            Some(Region::China) => "provider_region_china",
            _ => "provider_region_global",
        }
    } else {
        match route.product {
            Some(Product::Coding) => "provider_coding",
            _ => "provider_metered",
        }
    })
}

fn route_target(routes: &[Route], current: Option<&Route>, requested: Route) -> Route {
    current
        .filter(|current| current.product != requested.product)
        .and_then(|current| {
            routes.iter().find(|candidate| {
                candidate.protocol == requested.protocol
                    && candidate.product == requested.product
                    && candidate.region == current.region
            })
        })
        .cloned()
        .unwrap_or(requested)
}

/// Partner entries offering the current route's protocol, for one-shot prefill.
fn partner_choices(directory: &[Entry], current: &Route) -> Vec<(Entry, Route)> {
    directory
        .iter()
        .filter(|entry| {
            !matches!(
                entry.source,
                sailry_protocol::conversation::presets::Source::Builtin
            ) && matches!(
                entry.preset.auth,
                sailry_protocol::conversation::presets::Auth::ApiKey { .. }
                    | sailry_protocol::conversation::presets::Auth::None
            )
        })
        .filter_map(|entry| {
            entry
                .preset
                .routes
                .iter()
                .find(|route| route.protocol == current.protocol)
                .map(|route| (entry.clone(), route.clone()))
        })
        .collect()
}

fn route_choices(routes: &[Route], current: &Route, region: bool) -> Vec<Route> {
    let mut choices: Vec<Route> = Vec::new();
    for route in routes {
        let applicable = if region {
            route.product == current.product && route.protocol == current.protocol
        } else {
            route.protocol == current.protocol
        };
        if applicable
            && !choices.iter().any(|other| {
                if region {
                    other.region == route.region
                } else {
                    other.product == route.product
                }
            })
        {
            choices.push(route.clone());
        }
    }
    choices
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use sailry_protocol::conversation::{connection::Product, presets::Region};

    #[test]
    fn preserves_region_across_products() {
        let entry = presets::bundled()
            .into_iter()
            .find(|entry| entry.preset.id == "qwen")
            .unwrap();
        let current = entry
            .preset
            .routes
            .iter()
            .find(|route| {
                route.region == Some(Region::Global) && route.product == Some(Product::Metered)
            })
            .unwrap();
        let requested = entry
            .preset
            .routes
            .iter()
            .find(|route| {
                route.region == Some(Region::China) && route.product == Some(Product::Coding)
            })
            .unwrap()
            .clone();
        let target = route_target(&entry.preset.routes, Some(current), requested);
        assert_eq!(target.region, Some(Region::Global));
        assert_eq!(target.product, Some(Product::Coding));
        assert_eq!(
            target.endpoint,
            entry
                .preset
                .routes
                .iter()
                .find(|route| route.key() == target.key())
                .unwrap()
                .endpoint
        );
        let choices = route_choices(&entry.preset.routes, &target, true);
        assert_eq!(choices.len(), 2);
        assert!(
            choices
                .iter()
                .all(|route| route.product == Some(Product::Coding))
        );
    }

    #[test]
    fn keeps_explicit_missing_region_fallback() {
        let entry = presets::bundled()
            .into_iter()
            .find(|entry| entry.preset.id == "doubao")
            .unwrap();
        let current = entry
            .preset
            .routes
            .iter()
            .find(|route| route.product == Some(Product::Metered))
            .unwrap();
        let requested = entry
            .preset
            .routes
            .iter()
            .find(|route| route.product == Some(Product::Coding))
            .unwrap()
            .clone();
        assert_eq!(
            route_target(&entry.preset.routes, Some(current), requested.clone()),
            requested
        );
    }

    mod partner_choices {
        use super::*;
        use sailry_protocol::conversation::presets::Source;

        fn compatible() -> Entry {
            presets::bundled()
                .into_iter()
                .find(|entry| entry.preset.id == "compatible-chat")
                .unwrap()
        }

        #[test]
        fn empty_without_partners() {
            let entry = compatible();
            let route = entry.preset.routes[0].clone();
            assert!(partner_choices(&[], &route).is_empty());
            assert!(partner_choices(&presets::bundled(), &route).is_empty());
        }

        #[test]
        fn matches_route_protocol() {
            let builtin = compatible();
            let route = builtin.preset.routes[0].clone();
            let mut partner = builtin.clone();
            partner.source = Source::Partner;
            let choices = partner_choices(&[builtin, partner], &route);
            assert_eq!(choices.len(), 1);
            assert_eq!(choices[0].0.source, Source::Partner);
            assert_eq!(choices[0].1.protocol, route.protocol);
        }
    }
}
