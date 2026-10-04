use super::*;

const PRESETS: [u32; 5] = [128_000, 200_000, 256_000, 400_000, 1_000_000];

impl Editor {
    pub(super) fn limits(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let draft = &self.models[index];
        let key = draft.key;
        let presets = h_flex()
            .gap_1()
            .ml_auto()
            .children(PRESETS.into_iter().map(|tokens| {
                let label = if tokens >= 1_000_000 {
                    rust_i18n::t!("provider_tokens_m", count = tokens / 1_000_000)
                } else {
                    rust_i18n::t!("provider_tokens_k", count = tokens / 1_000)
                }
                .to_string();
                let description = format!("{} · {tokens}", tr("provider_context"));
                Button::new(format!("model-context-{key}-{tokens}"))
                    .debug_selector(move || format!("model-context-{key}-{tokens}"))
                    .ghost()
                    .xsmall()
                    .rounded_full()
                    .label(label)
                    .tooltip(description.clone())
                    .accessibility_label(description)
                    .disabled(self.pending)
                    .on_click(cx.listener(move |editor, _, window, cx| {
                        let draft = &mut editor.models[index];
                        draft.missing.context = false;
                        draft.context.update(cx, |input, cx| {
                            input.set_value(tokens.to_string(), window, cx);
                        });
                        cx.notify();
                    }))
            }));
        Form::vertical()
            .child(
                Field::new().label(tr("provider_context")).child(
                    v_flex()
                        .gap_2()
                        .child(h_flex().w_full().flex_wrap().gap_2().child(presets))
                        .child(
                            div()
                                .debug_selector(move || format!("model-context-input-{key}"))
                                .child(
                                    Input::new(&draft.context)
                                        .disabled(self.pending)
                                        .aria_label(tr("provider_context")),
                                ),
                        ),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
