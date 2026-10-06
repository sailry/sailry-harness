use super::*;
use gpui_kit::base::Link;

const USAGE_URL: &str = "https://chatgpt.com/settings/usage";

pub(super) fn content(channel: &Channel, real: bool, cx: &App) -> AnyElement {
    let id = channel.id;
    h_flex()
        .w_full()
        .min_w_0()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .gap_1()
                .child(div().truncate().child(channel.name.clone()))
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
                                .child(tr(channel.preset.key())),
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
        .child(usage(channel, real, cx))
        .into_any_element()
}

fn usage(channel: &Channel, real: bool, cx: &App) -> AnyElement {
    let id = channel.id;
    let link = cx.theme().link;
    let accent = cx.theme().accent;
    // No verified quota snapshot is available for this SIWC integration. Keep
    // the slot stable without presenting missing account data as sample usage.
    v_flex()
        .debug_selector(move || format!("provider-usage-{id}"))
        .w_48()
        .flex_shrink_0()
        .gap_1()
        .child(
            h_flex().w_full().gap_2().children(
                [
                    ("plan", "provider_plan"),
                    ("5h", "provider_usage_5h"),
                    ("7d", "provider_usage_7d"),
                ]
                .map(|(name, label)| {
                    v_flex()
                        .debug_selector(move || format!("provider-{name}-{id}"))
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr(label)),
                        )
                        .child(
                            div()
                                .debug_selector(move || format!("provider-{name}-value-{id}"))
                                .child(tr("provider_usage_unknown")),
                        )
                }),
            ),
        )
        .child(
            h_flex()
                .h_5()
                .justify_end()
                .when(channel.preset == Preset::ChatGpt, |row| {
                    row.child(
                        Link::new(("provider-manage-usage", id))
                            .debug_selector(move || format!("provider-manage-usage-{id}"))
                            .href(USAGE_URL)
                            .disabled(!real)
                            .accessibility_label(tr("provider_manage_usage"))
                            .text_xs()
                            .text_color(link)
                            .cursor_pointer()
                            .hover(|style| style.text_decoration_1())
                            .focus_visible(move |style| style.bg(accent).text_decoration_1())
                            .styles(move |style| {
                                style
                                    .disabled(|style| style.text_color(cx.theme().muted_foreground))
                            })
                            .open_with(|url, _, _, cx| cx.open_url(url))
                            .child(tr("provider_manage_usage")),
                    )
                }),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;
