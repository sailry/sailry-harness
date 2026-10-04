use super::*;
use gpui_kit::base::{Presence, Transition, spring};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    progress::Progress,
};
use gpui_kit::prelude::FluentBuilder as _;

mod view;
mod viewport;
use viewport::{MAX_ZOOM, MIN_ZOOM, Viewport};

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) fn open(
    images: &Entity<Images>,
    source: impl Into<ImageSource>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Preview> {
    open_gallery(images, vec![source.into()], 0, window, cx)
}

pub(super) fn open_gallery(
    images: &Entity<Images>,
    sources: Vec<ImageSource>,
    index: usize,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Preview> {
    let index = index.min(sources.len().saturating_sub(1));
    let images = images.read(cx);
    let binding = images.binding.clone();
    let slots = images.slots.clone();
    open_sources(binding, slots, sources, index, window, cx)
}

pub(super) fn open_sources(
    binding: Binding,
    slots: Arc<tokio::sync::Semaphore>,
    sources: Vec<ImageSource>,
    index: usize,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Preview> {
    let view = cx.new(|cx: &mut Context<Preview>| {
        cx.on_release(|view, cx| view.clear(None, cx)).detach();
        Preview {
            binding,
            slots,
            sources,
            index,
            focus: cx.focus_handle(),
            result: None,
            transforming: false,
            cancel: CancellationToken::new(),
            viewport: Viewport::default(),
            closing: false,
        }
    });
    view.update(cx, |view, cx| view.reload(window, cx));
    let content = view.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let close = content.clone();
        let cancel = content.clone();
        dialog
            .full_screen(true)
            .on_ok(|_, _, _| false)
            .on_cancel(move |_, _, cx| {
                if cx.reduce_motion() {
                    return true;
                }
                cancel.update(cx, |view, cx| {
                    view.closing = true;
                    cx.notify();
                });
                false
            })
            .on_close(move |_, window, cx| {
                close.update(cx, |view, cx| view.clear(Some(window), cx))
            })
            .child(content.clone())
    });
    view.read(cx).focus.clone().focus(window, cx);
    view
}

pub(super) struct Preview {
    binding: Binding,
    slots: Arc<tokio::sync::Semaphore>,
    sources: Vec<ImageSource>,
    index: usize,
    focus: FocusHandle,
    pub(super) result: Option<Result<Arc<RenderImage>, &'static str>>,
    transforming: bool,
    cancel: CancellationToken,
    viewport: Viewport,
    closing: bool,
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Preview {
    fn clear(&mut self, window: Option<&mut Window>, cx: &mut App) {
        self.cancel.cancel();
        self.transforming = false;
        if let Some(Ok(image)) = self.result.take() {
            cx.drop_image(image, window);
        }
    }

    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear(Some(window), cx);
        self.viewport = Viewport::default();
        self.cancel = CancellationToken::new();
        self.result = None;
        let Some(source) = self.sources.get(self.index).cloned() else {
            self.result = Some(Err("chat_image_unavailable"));
            cx.notify();
            return;
        };
        let cancel = self.cancel.clone();
        let task = self.binding.runtime.spawn(load::load(
            self.binding.client.clone(),
            source,
            1600,
            cancel.clone(),
            self.slots.clone(),
        ));
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await.unwrap_or(Err("chat_image_unavailable"));
            if !cancel.is_cancelled() {
                let _ = view.update(cx, |view, cx| {
                    view.result = Some(result);
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    fn step(&mut self, next: bool, window: &mut Window, cx: &mut Context<Self>) {
        let index = if next {
            (self.index + 1).min(self.sources.len().saturating_sub(1))
        } else {
            self.index.saturating_sub(1)
        };
        if index != self.index {
            self.index = index;
            self.reload(window, cx);
        }
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers != Modifiers::default() {
            return;
        }
        match event.keystroke.key.as_str() {
            "left" => self.step(false, window, cx),
            "right" => self.step(true, window, cx),
            "+" | "=" => self.zoom_by(1.25, cx),
            "-" => self.zoom_by(0.8, cx),
            "0" => {
                self.viewport = Viewport::default();
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        if matches!(self.result, Some(Ok(_))) && !self.closing {
            self.viewport.zoom_by(factor);
            cx.notify();
        }
    }

    fn wheel(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = f32::from(event.delta.pixel_delta(px(24.)).y);
        self.zoom_by((delta * 0.008).exp(), cx);
        cx.stop_propagation();
    }

    fn pinch(&mut self, event: &PinchEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_by(1. + event.delta, cx);
        cx.stop_propagation();
    }

    fn drag(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.viewport.drag = None;
            return;
        }
        if let Some(previous) = self.viewport.drag {
            self.viewport.drag = Some(event.position);
            self.viewport.pan += event.position - previous;
            cx.notify();
            cx.stop_propagation();
        }
    }

    fn transform(&mut self, change: Change, window: &mut Window, cx: &mut Context<Self>) {
        if self.transforming {
            return;
        }
        let Some(Ok(image)) = &self.result else {
            return;
        };
        // Kit has no raster transform API; transform the bounded preview on a worker.
        let image = image.clone();
        let cancel = self.cancel.clone();
        let work_cancel = cancel.clone();
        let slots = self.slots.clone();
        self.transforming = true;
        let task = self.binding.runtime.spawn(async move {
            let permit = tokio::select! {
                biased;
                _ = work_cancel.cancelled() => return Err("chat_image_unavailable"),
                permit = slots.acquire_owned() => permit.map_err(|_| "chat_image_unavailable")?,
            };
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                if work_cancel.is_cancelled() {
                    return Err("chat_image_unavailable");
                }
                change.apply(&image)
            })
            .await
            .unwrap_or(Err("chat_image_unavailable"))
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await.unwrap_or(Err("chat_image_unavailable"));
            if !cancel.is_cancelled() {
                let _ = view.update_in(cx, |view, window, cx| {
                    if let Some(Ok(image)) = view.result.replace(result) {
                        cx.drop_image(image, Some(window));
                    }
                    view.transforming = false;
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }
}

#[derive(Clone, Copy)]
enum Change {
    Rotate,
    Horizontal,
    Vertical,
}

impl Change {
    fn apply(self, image: &RenderImage) -> Result<Arc<RenderImage>, &'static str> {
        let size = image.size(0);
        let bytes = image.as_bytes(0).ok_or("chat_image_unavailable")?;
        let pixels =
            image::RgbaImage::from_raw(size.width.0 as u32, size.height.0 as u32, bytes.to_vec())
                .ok_or("chat_image_unavailable")?;
        // RenderImage stores BGRA, and these operations only move whole pixels.
        let pixels = match self {
            Self::Rotate => image::imageops::rotate90(&pixels),
            Self::Horizontal => image::imageops::flip_horizontal(&pixels),
            Self::Vertical => image::imageops::flip_vertical(&pixels),
        };
        Ok(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
    }
}
