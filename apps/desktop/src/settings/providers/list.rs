use super::*;
use gpui_kit::base::Link;
use gpui_kit::component::progress::Progress;
use sailry_protocol::conversation::quota::Window;

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
    let now = super::live::quota::now();
    let quota = (real && channel.preset == Preset::ChatGpt)
        .then_some(channel.quota.as_ref())
        .flatten();
    v_flex()
        .debug_selector(move || format!("provider-usage-{id}"))
        .w_48()
        .flex_shrink_0()
        .gap_1()
        .child(
            h_flex().w_full().gap_2().children(
                [
                    ("plan", "provider_plan", None),
                    (
                        "5h",
                        "provider_usage_5h",
                        quota.and_then(|quota| quota.five_hour.as_ref()),
                    ),
                    (
                        "7d",
                        "provider_usage_7d",
                        quota.and_then(|quota| quota.seven_day.as_ref()),
                    ),
                ]
                .map(|(name, label, window)| {
                    let remaining = remaining(window, now);
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
                                .child(remaining.map_or_else(
                                    || tr("provider_usage_unknown").to_string(),
                                    |value| format!("{}%", percent(value)),
                                )),
                        )
                        .child(div().w_full().h_1().when_some(remaining, |row, value| {
                            row.child(
                                div()
                                    .debug_selector(move || {
                                        format!("provider-{name}-progress-{id}")
                                    })
                                    .w_full()
                                    .child(
                                        Progress::new((
                                            SharedString::from(format!("provider-{name}-progress")),
                                            id,
                                        ))
                                        .xsmall()
                                        .value(value as f32 / 100.)
                                        .accessibility_label(remaining_label(label)),
                                    ),
                            )
                        }))
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

fn remaining(window: Option<&Window>, now_ms: u64) -> Option<u32> {
    window
        .filter(|window| window.available_at(now_ms))
        .map(Window::remaining_basis_points)
}

fn percent(basis_points: u32) -> String {
    let whole = basis_points / 100;
    let fraction = basis_points % 100;
    if fraction == 0 {
        whole.to_string()
    } else if fraction % 10 == 0 {
        format!("{whole}.{}", fraction / 10)
    } else {
        format!("{whole}.{fraction:02}")
    }
}

fn remaining_label(window: &'static str) -> SharedString {
    rust_i18n::t!("provider_quota_remaining", window = tr(window))
        .to_string()
        .into()
}

#[cfg(test)]
mod tests;
