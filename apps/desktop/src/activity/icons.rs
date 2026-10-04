//! Shared status glyphs for notifications and native resource surfaces.
use super::*;
use sailry_client::activity::Lane;

pub(super) fn path(state: Lane) -> &'static str {
    match state {
        Lane::Waiting => "icons/remix/progress-4-line.svg",
        Lane::Running => "icons/remix/progress-2-line.svg",
        Lane::Completed => "icons/remix/progress-6-line.svg",
        Lane::Failed => "icons/remix/close-circle-line.svg",
        Lane::Idle => "icons/remix/checkbox-blank-circle-line.svg",
    }
}

pub(crate) fn indicator(state: Lane, cx: &App) -> Icon {
    let color = match state {
        Lane::Waiting => cx.theme().warning,
        Lane::Running => cx.theme().primary,
        Lane::Completed => cx.theme().success,
        Lane::Failed => cx.theme().danger,
        Lane::Idle => cx.theme().muted_foreground,
    };
    Icon::default().path(path(state)).text_color(color)
}
