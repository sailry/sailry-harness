//! One bounded redraw request per visible view, shared by decorative canvases.
//! Scheduling happens during paint, so clipped and unmounted glyphs stop driving
//! their owner. No task state is inferred or retained by the animation clock.
use gpui_kit::*;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 30);

#[derive(Default)]
struct Clock {
    epoch: Option<Instant>,
    pending: HashSet<(WindowId, EntityId)>,
}

impl Global for Clock {}

pub(super) fn seconds(
    bounds: Bounds<Pixels>,
    animated: bool,
    rest: f32,
    window: &mut Window,
    cx: &mut App,
) -> Option<f32> {
    if !visible(bounds, window.content_mask().bounds) {
        return None;
    }
    if !animated || cx.reduce_motion() {
        return Some(rest);
    }
    let now = cx.background_executor().now();
    let handle = window.window_handle();
    let view = window.current_view();
    let key = (handle.window_id(), view);
    let clock = cx.default_global::<Clock>();
    let elapsed = now.duration_since(*clock.epoch.get_or_insert(now));
    if window.is_window_active() && clock.pending.insert(key) {
        cx.spawn(async move |cx| {
            cx.background_executor().timer(FRAME).await;
            cx.update(|cx| {
                cx.default_global::<Clock>().pending.remove(&key);
                let active = cx
                    .update_window(handle, |_, window, cx| {
                        window.is_window_active() && !cx.reduce_motion()
                    })
                    .unwrap_or(false);
                if active {
                    cx.notify(view);
                }
            });
        })
        .detach();
    }
    Some(elapsed.as_secs_f64() as f32)
}

fn visible(bounds: Bounds<Pixels>, mask: Bounds<Pixels>) -> bool {
    let clipped = bounds.intersect(&mask);
    clipped.size.width > px(0.) && clipped.size.height > px(0.)
}

#[cfg(test)]
mod tests;
