use super::*;
use std::{
    cell::RefCell,
    sync::Arc,
    time::{Duration, Instant},
};

type Frame = (Bounds<Pixels>, Bounds<Pixels>);

#[derive(Default)]
pub(super) struct Snapshot {
    image: RefCell<Option<(Frame, Arc<RenderImage>)>>,
    pending: Cell<bool>,
    captured: Cell<Option<Instant>>,
    pub(super) shown: Cell<bool>,
}

impl Snapshot {
    pub(super) fn request(
        &self,
        native: &Rc<Native>,
        blocked: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let frame = native.frame.get().unwrap();
        let cached = self.image.borrow();
        let stale = cached
            .as_ref()
            .is_none_or(|(previous, _)| !matches(*previous, frame))
            || self
                .captured
                .get()
                .is_none_or(|time| time.elapsed() >= Duration::from_secs(1));
        drop(cached);
        if !stale || self.pending.replace(true) {
            return;
        }
        let native = Rc::downgrade(native);
        window
            .spawn(cx, async move |cx| {
                // The first painted native frame may still be committing its document.
                // Warm the cache after layout; overlapping surfaces request it immediately.
                if !blocked {
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                }
                let receive = cx
                    .update(|_, _| {
                        let native = native.upgrade().ok_or(())?;
                        if !native.mounted.get() {
                            return Err(());
                        }
                        let frame = native.frame.get().ok_or(())?;
                        crate::ui::webview_capture::capture(&native.page, None)
                            .map(|receive| (frame, receive))
                    })
                    .ok()
                    .and_then(Result::ok);
                let (frame, result) = if let Some((frame, receive)) = receive {
                    let timer = cx.background_executor().timer(Duration::from_secs(5));
                    let buffer = tokio::select! {
                        result = receive => result.ok().and_then(Result::ok),
                        _ = timer => None,
                    };
                    if let Some(buffer) = buffer {
                        let image = cx
                            .background_executor()
                            .spawn(async move {
                                crate::content::images::decode::decode(
                                    &buffer.bytes,
                                    buffer.format,
                                    16_384,
                                )
                                .ok()
                            })
                            .await;
                        (frame, image)
                    } else {
                        (frame, None)
                    }
                } else {
                    (frame, None)
                };
                let _ = cx.update(|window, _| {
                    let Some(native) = native.upgrade() else {
                        return;
                    };
                    native.snapshot.pending.set(false);
                    native.snapshot.captured.set(Some(Instant::now()));
                    if let Some(image) = result {
                        let previous = native.snapshot.image.replace(Some((frame, image)));
                        if let Some((_, image)) = previous {
                            let _ = window.drop_image(image);
                        }
                        if native.mounted.get() {
                            window.refresh();
                        }
                    }
                });
            })
            .detach();
    }

    pub(super) fn paint(&self, frame: Frame, window: &mut Window) -> bool {
        let image = self.image.borrow();
        let Some((_, image)) = image
            .as_ref()
            .filter(|(previous, _)| matches(*previous, frame))
        else {
            return false;
        };
        window
            .paint_image(
                frame.1,
                frame.1,
                Corners::default(),
                image.clone(),
                0,
                false,
            )
            .is_ok()
    }
}

fn matches(previous: Frame, current: Frame) -> bool {
    layout(previous.0, previous.1) == layout(current.0, current.1)
        && previous.1.size == current.1.size
}
