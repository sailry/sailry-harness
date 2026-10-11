//! Native child views yield to intersecting GPUI overlays through a warm image.
//! Native snapshots own freshness; synchronous pixels only keep a matching fallback.
use gpui_kit::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;

type Frame = (Bounds<Pixels>, Bounds<Pixels>);

enum Signal {
    Dirty,
    Ready,
}

pub(crate) struct Signals(mpsc::UnboundedSender<Signal>);

impl Signals {
    pub(crate) fn prepare<'a>(&self, builder: wry::WebViewBuilder<'a>) -> wry::WebViewBuilder<'a> {
        builder.with_initialization_script_for_main_only(include_str!("webview_snapshot.js"), false)
    }

    pub(crate) fn receive(&self, message: &str) -> bool {
        let signal = match message {
            "sailry-snapshot:dirty" => Signal::Dirty,
            "sailry-snapshot:ready" => Signal::Ready,
            _ => return false,
        };
        let _ = self.0.send(signal);
        true
    }
}

struct Image {
    frame: Frame,
    generation: Option<u64>,
    image: Arc<RenderImage>,
}

pub(crate) struct Snapshot {
    image: RefCell<Option<Image>>,
    frame: Cell<Option<Frame>>,
    generation: Cell<u64>,
    pending: Cell<bool>,
    failed: Cell<Option<u64>>,
    settled: Cell<bool>,
    mounted: Cell<bool>,
    pub(crate) shown: Cell<bool>,
    signals: Rc<Signals>,
    receiver: RefCell<Option<mpsc::UnboundedReceiver<Signal>>>,
}

impl Snapshot {
    pub(crate) fn new() -> Rc<Self> {
        let (sender, receiver) = mpsc::unbounded_channel();
        Rc::new(Self {
            image: RefCell::new(None),
            frame: Cell::new(None),
            generation: Cell::new(0),
            pending: Cell::new(false),
            failed: Cell::new(None),
            settled: Cell::new(true),
            mounted: Cell::new(false),
            shown: Cell::new(false),
            signals: Rc::new(Signals(sender)),
            receiver: RefCell::new(Some(receiver)),
        })
    }

    pub(crate) fn signals(&self) -> Rc<Signals> {
        self.signals.clone()
    }

    pub(crate) fn bind(self: &Rc<Self>, window: &mut Window, cx: &mut App) {
        let Some(mut receiver) = self.receiver.borrow_mut().take() else {
            return;
        };
        let snapshot = Rc::downgrade(self);
        window
            .spawn(cx, async move |cx| {
                while let Some(signal) = receiver.recv().await {
                    if cx
                        .update(|window, _| {
                            let snapshot = snapshot.upgrade().ok_or(())?;
                            match signal {
                                Signal::Dirty => {
                                    snapshot
                                        .generation
                                        .set(snapshot.generation.get().wrapping_add(1));
                                    snapshot.settled.set(false);
                                }
                                Signal::Ready => {
                                    // Frames settle independently. A later frame completion
                                    // must refresh a capture made after another frame settled.
                                    snapshot
                                        .generation
                                        .set(snapshot.generation.get().wrapping_add(1));
                                    snapshot.settled.set(true);
                                }
                            }
                            if snapshot.mounted.get() {
                                window.refresh();
                            }
                            Ok::<_, ()>(())
                        })
                        .ok()
                        .and_then(Result::ok)
                        .is_none()
                    {
                        return;
                    }
                }
            })
            .detach();
    }

    pub(crate) fn mount(&self, frame: Frame) {
        if self
            .frame
            .get()
            .is_none_or(|previous| !matches(previous, frame))
        {
            self.generation.set(self.generation.get().wrapping_add(1));
        }
        self.frame.set(Some(frame));
        self.mounted.set(true);
    }

    pub(crate) fn unmount(&self) {
        self.mounted.set(false);
        self.shown.set(false);
    }

    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn refresh(
        self: &Rc<Self>,
        blocked: bool,
        capture: impl FnOnce() -> Result<super::webview_capture::Buffer, ()>,
        window: &mut Window,
    ) {
        if !blocked {
            return;
        }
        let Some(frame) = self.frame.get() else {
            return;
        };
        if self
            .image
            .borrow()
            .as_ref()
            .is_some_and(|image| matches(image.frame, frame))
        {
            return;
        }
        // AppKit can return opaque backing pixels before WebKit renders recent
        // changes. Keep their geometry without claiming the DOM is current.
        let image = capture().ok().and_then(decode);
        if let Some(image) = image {
            self.publish(frame, None, image, window);
        }
    }

    pub(crate) fn request(
        self: &Rc<Self>,
        capture: impl FnOnce() -> Result<super::webview_capture::Pending, ()>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(frame) = self.frame.get() else {
            return;
        };
        let generation = self.generation.get();
        let current = self.image.borrow().as_ref().is_some_and(|image| {
            image.generation == Some(generation) && matches(image.frame, frame)
        });
        if current && self.settled.get()
            || self.failed.get() == Some(generation)
            || !cfg!(target_os = "macos") && !self.settled.get()
            || self.pending.replace(true)
        {
            return;
        }
        let Ok(receive) = capture() else {
            self.pending.set(false);
            self.failed.set(Some(generation));
            return;
        };
        let snapshot = Rc::downgrade(self);
        window
            .spawn(cx, async move |cx| {
                let timer = cx.background_executor().timer(Duration::from_secs(5));
                let buffer = tokio::select! {
                    result = receive => result.ok().and_then(Result::ok),
                    _ = timer => None,
                };
                let image = if let Some(buffer) = buffer {
                    cx.background_executor()
                        .spawn(async move { decode(buffer) })
                        .await
                } else {
                    None
                };
                let _ = cx.update(|window, _| {
                    let Some(snapshot) = snapshot.upgrade() else {
                        return;
                    };
                    snapshot.pending.set(false);
                    let superseded = snapshot.generation.get() != generation
                        || snapshot
                            .frame
                            .get()
                            .is_none_or(|current| !matches(frame, current));
                    if !superseded {
                        if let Some(image) = image {
                            snapshot.publish(frame, Some(generation), image, window);
                        } else {
                            snapshot.failed.set(Some(generation));
                        }
                    }
                    // Completion drives the next capture while dirty, without
                    // forcing extra paints of an unchanged AppKit fallback.
                    if snapshot.mounted.get() {
                        window.refresh();
                    }
                });
            })
            .detach();
    }

    fn publish(
        &self,
        frame: Frame,
        generation: Option<u64>,
        image: Arc<RenderImage>,
        window: &mut Window,
    ) {
        self.failed.set(None);
        if let Some(previous) = self.image.replace(Some(Image {
            frame,
            generation,
            image,
        })) {
            let _ = window.drop_image(previous.image);
        }
    }

    pub(crate) fn paint(&self, window: &mut Window) -> bool {
        let Some(frame) = self.frame.get() else {
            return false;
        };
        let cached = self.image.borrow();
        let Some(image) = cached.as_ref().filter(|image| matches(image.frame, frame)) else {
            return false;
        };
        window
            .paint_image(
                frame.1,
                frame.1,
                Corners::default(),
                image.image.clone(),
                0,
                false,
            )
            .is_ok()
    }

    #[cfg(any(target_os = "macos", test, feature = "workload-tests"))]
    pub(crate) fn current(&self) -> bool {
        self.image.borrow().as_ref().is_some_and(|image| {
            image.generation == Some(self.generation.get())
                && self
                    .frame
                    .get()
                    .is_some_and(|frame| matches(image.frame, frame))
        })
    }

    #[cfg(feature = "workload-tests")]
    pub(crate) fn state(&self) -> serde_json::Value {
        let geometry = |bounds: Bounds<Pixels>| {
            serde_json::json!({
                "left": f32::from(bounds.left()),
                "top": f32::from(bounds.top()),
                "right": f32::from(bounds.right()),
                "bottom": f32::from(bounds.bottom()),
                "width": f32::from(bounds.size.width),
                "height": f32::from(bounds.size.height),
            })
        };
        serde_json::json!({
            "mounted": self.mounted.get(),
            "shown": self.shown.get(),
            "pending": self.pending.get(),
            "settled": self.settled.get(),
            "failed": self.failed.get(),
            "current": self.current(),
            "generation": self.generation.get(),
            "frame": self.frame.get().map(|(bounds, clip)| serde_json::json!({
                "bounds": geometry(bounds),
                "clip": geometry(clip),
            })),
        })
    }

    #[cfg(feature = "workload-tests")]
    pub(crate) fn generation(&self) -> u64 {
        self.generation.get()
    }
}

fn decode(buffer: super::webview_capture::Buffer) -> Option<Arc<RenderImage>> {
    crate::content::images::decode::decode(&buffer.bytes, buffer.format, 16_384).ok()
}

fn matches(previous: Frame, current: Frame) -> bool {
    previous.0.size == current.0.size
        && previous.0.origin - previous.1.origin == current.0.origin - current.1.origin
        && previous.1.size == current.1.size
}

#[cfg(test)]
#[path = "webview_snapshot/tests.rs"]
mod tests;
