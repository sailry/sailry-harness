//! Directory selection changes a draft; it never saves or authenticates it.
use super::super::presets;
use super::*;
use gpui_kit::component::{
    menu::{DropdownMenu, PopupMenuItem},
    tag::Tag,
};
use sailry_protocol::{
    Command, Output,
    conversation::presets::{Entry, PresetRef, Route},
};

impl Editor {
    pub(super) fn authentication(&self) -> sailry_protocol::Authentication {
        use sailry_protocol::Authentication;
        self.selected.as_ref().map_or_else(
            || {
                self.provider
                    .as_ref()
                    .filter(|provider| provider.authentication == Authentication::Plugin)
                    .map_or(self.preset.authentication(), |provider| {
                        provider.authentication
                    })
            },
            presets::authentication,
        )
    }

    pub(super) fn custom_endpoint(&self) -> bool {
        self.selected
            .as_ref()
            .zip(self.selected_route())
            .map_or_else(
                || self.preset.custom_endpoint(),
                |(entry, route)| presets::custom_endpoint(entry, route),
            )
    }

    pub(super) fn metadata_only(&self) -> bool {
        self.preset.cloud()
            || self
                .selected
                .as_ref()
                .is_some_and(|entry| routes::metadata_only(&entry.preset, self.selected_route()))
    }

    pub(super) fn clear_route(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.probe = None;
        self.models.clear();
        self.default_model = 0;
        self.configured = false;
        self.original_key = None;
        self.key_ready = true;
        self.credential
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.error = None;
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
                    && self
                        .selected_route()
                        .is_some_and(|route| route.endpoint.is_empty())
                    && !self.preset.cloud()
                    && !self.uses_login()
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
                        editor.directory = presets::presentation(listing.entries);
                        // Resolving a label for an existing channel must preserve its draft.
                        editor.selected = refreshed(
                            &editor.directory,
                            editor.selected_ref.as_ref(),
                            editor.selected.as_ref(),
                        );
                        if let Some(entry) = &editor.selected
                            && let Some(route) = editor.selected_route()
                        {
                            editor.preset = presets::kind(entry, route);
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
        self.selected_ref = Some(reference);
        self.selected = Some(entry);
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
        let route = routes::target(&entry.preset.routes, self.selected_route(), route);
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
        for id in routes::models(&entry.preset, self.selected_route()) {
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
        let badge = entry.preset.badge.clone()?;
        Some(
            h_flex()
                .gap_2()
                .child(Tag::secondary().small().child(badge)),
        )
    }

    pub(super) fn route_field(&self, form: Form, region: bool, cx: &Context<Self>) -> Form {
        let Some(entry) = &self.selected else {
            return form;
        };
        let Some(current) = self.selected_route() else {
            return form;
        };
        let choices = routes::choices(&entry.preset.routes, current, region);
        if choices.len() < 2 {
            return form;
        }
        let key = if region {
            "provider_region"
        } else {
            "provider_product"
        };
        let label = routes::label(&entry.preset, current, region);
        let preset = entry.preset.clone();
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
                            let label = routes::label(&preset, &route, region);
                            menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                owner.update(cx, |editor, cx| {
                                    editor.change_route(route.clone(), window, cx)
                                })
                            }))
                        })
                    }),
            ),
        )
    }
}

pub(super) fn refreshed(
    directory: &[Entry],
    reference: Option<&PresetRef>,
    current: Option<&Entry>,
) -> Option<Entry> {
    let reference = reference?;
    directory
        .iter()
        .find(|entry| entry.source == reference.source && entry.preset.id == reference.id)
        .or_else(|| {
            current
                .filter(|entry| entry.source == reference.source && entry.preset.id == reference.id)
        })
        .cloned()
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

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn refreshes_metadata_without_reselecting() {
        let current = presets::bundled().remove(0);
        let reference = presets::reference(&current, &current.preset.routes[0]);
        let mut update = current.clone();
        update.preset.name = "Execution Node method".into();
        update.preset.routes[0].endpoint = "https://new-default.invalid/v1".into();
        let selected = refreshed(
            std::slice::from_ref(&update),
            Some(&reference),
            Some(&current),
        );
        assert_eq!(selected.as_ref(), Some(&update));
        assert_eq!(
            reference,
            presets::reference(&current, &current.preset.routes[0])
        );
        assert_eq!(
            refreshed(&[], Some(&reference), Some(&current)),
            Some(current)
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
