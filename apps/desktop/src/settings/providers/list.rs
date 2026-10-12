use super::*;

pub(super) fn content(channel: &Channel, real: bool, cx: &App) -> AnyElement {
    let id = channel.id;
    h_flex()
        .w_full()
        .min_w_0()
        .gap_3()
        .child(
            v_flex()
                .debug_selector(move || format!("provider-details-{id}"))
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .gap_1()
                .child(
                    h_flex().min_w_0().min_h_6().gap_1().child(
                        div()
                            .debug_selector(move || format!("provider-name-{id}"))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(channel.name.clone()),
                    ),
                )
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_3()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            div()
                                .debug_selector(move || format!("provider-kind-{id}"))
                                .min_w_0()
                                .truncate()
                                .child(channel.label()),
                        )
                        .child(
                            div()
                                .debug_selector(move || format!("provider-count-{id}"))
                                .flex_shrink_0()
                                .child(
                                    rust_i18n::t!(
                                        "provider_model_count",
                                        count = channel.models.len()
                                    )
                                    .to_string(),
                                ),
                        )
                        .when(!real, |summary| {
                            summary.child(div().flex_shrink_0().child(tr("preview")))
                        }),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;
