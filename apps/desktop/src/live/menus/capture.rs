//! Test-only observations at the real native menu boundary, not an OS popup simulator.
use super::*;

#[derive(Default)]
struct Captures(std::collections::HashMap<WindowId, Vec<(Dispatch, bool)>>);
impl Global for Captures {}

pub(super) fn record(window: &Window, cx: &mut App, actions: Vec<(Dispatch, bool)>) {
    cx.default_global::<Captures>()
        .0
        .insert(window.window_handle().window_id(), actions);
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn opened(window: &Window, cx: &App, node: NodeId, target: Target) -> bool {
    cx.try_global::<Captures>()
        .and_then(|captures| captures.0.get(&window.window_handle().window_id()))
        .is_some_and(|actions| {
            !actions.is_empty()
                && actions
                    .iter()
                    .all(|(action, _)| action.node == node && action.target == target)
        })
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn actions(window: &Window, cx: &App) -> Vec<(Dispatch, bool)> {
    cx.try_global::<Captures>()
        .and_then(|captures| captures.0.get(&window.window_handle().window_id()))
        .cloned()
        .expect("native resource menu was not opened")
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn choose(command: Command, window: &mut Window, cx: &mut App) {
    let (action, disabled) = actions(window, cx)
        .into_iter()
        .find(|(action, _)| action.command == command)
        .expect("native resource menu action is absent");
    assert!(!disabled, "native resource menu action is disabled");
    window.dispatch_action(Box::new(action), cx);
}
