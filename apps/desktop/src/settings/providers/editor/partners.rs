//! Suggestions fill the current draft; they never discover, authenticate or save.
use super::*;
use sailry_protocol::{
    Command, Output,
    conversation::partners::{Partner, Protocol},
};

fn protocol(preset: Preset) -> Option<Protocol> {
    match preset {
        Preset::CompatibleResponses => Some(Protocol::OpenAiResponses),
        Preset::CompatibleChat => Some(Protocol::OpenAiChatCompletions),
        Preset::CompatibleAnthropic => Some(Protocol::Anthropic),
        Preset::CompatibleGemini => Some(Protocol::Gemini),
        _ => None,
    }
}

fn route(partner: &Partner, preset: Preset) -> Option<&str> {
    let protocol = protocol(preset)?;
    partner
        .routes
        .iter()
        .find(|route| route.protocol == protocol)
        .map(|route| route.endpoint.as_str())
}

impl Editor {
    fn suggestions_active(&self) -> bool {
        !self.closed
            && self.editing.is_none()
            && self.flow.step == flow::Step::Configure
            && protocol(self.preset).is_some()
    }

    pub(super) fn load_partners(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.suggestions_active() || self.partner_task.is_some() {
            return;
        }
        let Some(binding) = self.binding.clone() else {
            return;
        };
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(binding.client.prepare(Command::ReadModelPartners))
                .await
        });
        self.partner_task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                if editor.closed {
                    return;
                }
                match result {
                    Ok(Ok(Output::ModelPartners(listing))) => {
                        editor.partners = listing.catalog;
                        if listing.unavailable && editor.suggestions_active() {
                            editor.report_error("provider_partners_unavailable", window, cx);
                        }
                    }
                    _ if editor.suggestions_active() => {
                        editor.report_error("provider_partners_unavailable", window, cx)
                    }
                    _ => {}
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn partner_choices(&self, cx: &Context<Self>) -> Div {
        let choices = self
            .partners
            .partners
            .iter()
            .filter(|partner| route(partner, self.preset).is_some())
            .cloned()
            .collect::<Vec<_>>();
        let body = v_flex().w_full().min_w_0().gap_2();
        if self.editing.is_some() || choices.is_empty() {
            return body;
        }
        body.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(tr("provider_partners")),
        )
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .flex_wrap()
                .gap_2()
                .children(choices.into_iter().map(|partner| {
                    let selected = self.partner.as_deref() == Some(partner.id.as_str());
                    Button::new(SharedString::from(format!(
                        "provider-partner-{}",
                        partner.id
                    )))
                    .small()
                    .outline()
                    .selected(selected)
                    .disabled(self.navigation_blocked())
                    .label(partner.name.clone())
                    .on_click(cx.listener(move |editor, _, window, cx| {
                        editor.select_partner(&partner, false, window, cx)
                    }))
                })),
        )
    }

    fn select_partner(
        &mut self,
        partner: &Partner,
        preserve_name: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation_blocked() || self.editing.is_some() {
            return;
        }
        let Some(endpoint) = route(partner, self.preset) else {
            return;
        };
        if self.partner.as_deref() == Some(partner.id.as_str())
            && self.endpoint.read(cx).value().as_str() == endpoint
        {
            return;
        }
        self.clear_route(window, cx);
        self.partner = Some(partner.id.clone());
        self.partner_key_url = partner.key_url.clone();
        if !preserve_name {
            self.name.update(cx, |input, cx| {
                input.set_value(partner.name.clone(), window, cx)
            });
        }
        self.endpoint.update(cx, |input, cx| {
            input.set_value(endpoint.to_owned(), window, cx)
        });
        // Use the ordinary editable manual-model path; the directory supplies no capabilities.
        for id in &partner.models {
            self.add_model(window, cx);
            self.models
                .last()
                .unwrap()
                .id
                .update(cx, |input, cx| input.set_value(id.clone(), window, cx));
        }
        cx.notify();
    }

    pub(super) fn select_protocol(
        &mut self,
        preset: Preset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.navigation_blocked() || self.editing.is_some() || self.preset == preset {
            return;
        }
        let partner = self
            .partner
            .as_ref()
            .and_then(|id| {
                self.partners
                    .partners
                    .iter()
                    .find(|partner| &partner.id == id)
            })
            .cloned();
        self.select_preset(preset, window, cx);
        if let Some(partner) = partner {
            if route(&partner, preset).is_some() {
                self.select_partner(&partner, true, window, cx);
            } else {
                self.clear_route(window, cx);
                self.endpoint
                    .update(cx, |input, cx| input.set_value("", window, cx));
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn limits_suggestions_to_compatible_protocols() {
        for preset in Preset::all() {
            assert_eq!(
                protocol(preset).is_some(),
                matches!(
                    preset,
                    Preset::CompatibleResponses
                        | Preset::CompatibleChat
                        | Preset::CompatibleAnthropic
                        | Preset::CompatibleGemini
                )
            );
        }
        assert_ne!(
            protocol(Preset::CompatibleResponses),
            protocol(Preset::CompatibleChat)
        );
    }

    #[test]
    fn never_substitutes_another_protocol() {
        let partner = Partner {
            id: "sample".into(),
            name: "Sample".into(),
            logo: None,
            description: None,
            homepage: None,
            key_url: None,
            badge: None,
            models: vec![],
            routes: vec![sailry_protocol::conversation::partners::Route {
                protocol: Protocol::OpenAiResponses,
                endpoint: "https://example.test/v1".into(),
            }],
        };
        assert_eq!(
            route(&partner, Preset::CompatibleResponses),
            Some("https://example.test/v1")
        );
        assert_eq!(route(&partner, Preset::CompatibleChat), None);
        assert_eq!(route(&partner, Preset::OpenAi), None);
    }
}
