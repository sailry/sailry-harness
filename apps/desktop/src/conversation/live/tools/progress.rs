use super::*;
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants};
use sailry_protocol::conversation::progress::{Progress, State};

/// Completed steps over all steps; skipped steps count as settled.
pub(in crate::conversation::live) fn counts(progress: &Progress) -> (usize, usize) {
    let done = progress
        .steps
        .iter()
        .filter(|step| matches!(step.state, State::Completed | State::Skipped))
        .count();
    (done, progress.steps.len())
}

/// Pi d9b56405 task rows: status icon, one-line label, and settled strike-through.
/// Kit owns animation, truncation, row styling, and scrolling.
pub(in crate::conversation::live) fn render(
    key: &str,
    progress: &Progress,
    continuing: bool,
    cx: &App,
) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .debug_selector({
            let key = key.to_owned();
            move || format!("live-progress-card-{key}")
        })
        .child(
            GroupBox::new()
                .id(format!("progress-card-{key}"))
                .outline()
                .content_style(
                    StyleRefinement::default()
                        .px_3()
                        .py_0()
                        .overflow_hidden()
                        .rounded(cx.theme().radius_lg)
                        .bg(cx.theme().group_box.opacity(0.8)),
                )
                .child(list(key, progress, continuing, cx)),
        )
        .into_any_element()
}

/// Popovers supply their own frame; only the bounded rows belong inside them.
pub(in crate::conversation::live) fn list(
    key: &str,
    progress: &Progress,
    continuing: bool,
    _cx: &App,
) -> AnyElement {
    let active = continuing
        .then(|| {
            progress
                .steps
                .iter()
                .position(|step| step.state == State::InProgress)
                .or_else(|| progress.steps.len().checked_sub(1))
        })
        .flatten();
    div()
        .w_full()
        .min_w_0()
        .debug_selector({
            let key = key.to_owned();
            move || format!("live-progress-{key}")
        })
        .child(surface::group_scroll(
            format!("progress-scroll-{key}"),
            crate::ui::status_list::Rows {
                id: format!("progress-{key}"),
                items: progress
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(index, step)| crate::ui::status_list::Row {
                        id: format!("progress-{key}-{index}"),
                        text: step.description.clone(),
                        state: match step.state {
                            State::Pending => crate::ui::status_list::State::Pending,
                            State::InProgress => crate::ui::status_list::State::InProgress,
                            State::Completed => crate::ui::status_list::State::Completed,
                            State::Skipped => crate::ui::status_list::State::Skipped,
                        },
                        details: false,
                        highlight: active == Some(index),
                    })
                    .collect(),
            },
        ))
        .into_any_element()
}
