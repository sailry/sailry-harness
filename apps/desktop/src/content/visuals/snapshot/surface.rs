//! One hidden page renders every thumbnail, one job at a time. It has no native
//! message channel: authored code stays in the sandboxed frame and the
//! renderer polls the Sailry-owned page with script results instead.
use super::{LAYOUT, Measure, Placement, Snapshot, VIEWPORT, crop, place};
use crate::theme::css::Palette;
use futures::{
    future::{Either, select},
    pin_mut,
};
use gpui_kit::{AnyWindowHandle, App, AppContext, Context, Entity, Global, Task, Window};
use raw_window_handle::HasWindowHandle;
use serde::Deserialize;
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};

const DEADLINE: Duration = Duration::from_secs(30);
const JOB_DEADLINE: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(100);
const QUEUE: usize = 64;
const BLANK: &str = "<!doctype html><title></title>";
const STATE: &str = "JSON.stringify({job:window.sailryJob===undefined?null:window.sailryJob,measure:window.sailrySnapshot||null,placed:window.sailryPlaced===true})";

pub(super) struct Request {
    pub(super) html: String,
    pub(super) palette: Palette,
    /// Card width in points; the page itself always lays out at `LAYOUT`.
    pub(super) width: f32,
    /// Card width in device pixels.
    pub(super) pixels: u32,
}

struct Service(Entity<Surface>);
impl Global for Service {}

struct Job {
    id: u64,
    request: Request,
    sender: oneshot::Sender<Result<Snapshot, String>>,
}

#[derive(Clone, Copy)]
enum Phase {
    Measuring,
    Placing(Placement),
    Capturing,
}

enum Event {
    Loaded(u64),
    State(u64, String),
}

#[derive(Deserialize)]
struct State {
    job: Option<u64>,
    measure: Option<Measure>,
    placed: bool,
}

/// Script results arrive as JSON of the returned JSON text.
fn state(raw: &str) -> Option<State> {
    let text: String = serde_json::from_str(raw).ok()?;
    serde_json::from_str(&text).ok()
}

/// Visible only while rendering and placed outside the window: hidden WebKit
/// and WebView2 views stop painting, which leaves native captures blank.
fn bounds() -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(-f64::from(LAYOUT) - 64., 0.).into(),
        size: wry::dpi::LogicalSize::new(f64::from(LAYOUT), f64::from(VIEWPORT)).into(),
    }
}

struct Surface {
    view: Option<wry::WebView>,
    parent: Option<AnyWindowHandle>,
    loaded: bool,
    generation: u64,
    serial: u64,
    queue: VecDeque<Job>,
    active: Option<(Job, Phase)>,
    events: mpsc::UnboundedSender<Event>,
    _events: Task<()>,
    work: Option<Task<()>>,
    deadline: Option<Task<()>>,
}

pub(super) fn init(cx: &mut App) {
    if cx.has_global::<Service>() {
        return;
    }
    let surface = cx.new(Surface::new);
    cx.set_global(Service(surface));
}

pub(super) fn release(window: &mut Window, cx: &mut App) {
    let Some(service) = cx.try_global::<Service>().map(|service| service.0.clone()) else {
        return;
    };
    service.update(cx, |surface, _| {
        if surface.parent == Some(Window::window_handle(window)) {
            surface.close("Snapshot renderer closed");
        }
    });
}

pub(super) fn render(
    request: Request,
    window: &mut Window,
    cx: &mut App,
) -> Task<Result<Snapshot, String>> {
    let Some(service) = cx.try_global::<Service>().map(|service| service.0.clone()) else {
        return Task::ready(Err("Snapshot renderer is unavailable".into()));
    };
    let (sender, receiver) = oneshot::channel();
    service.update(cx, |surface, cx| {
        surface.enqueue(request, sender, window, cx)
    });
    let timer = cx.background_executor().timer(DEADLINE);
    cx.spawn(async move |_| {
        pin_mut!(receiver, timer);
        match select(receiver, timer).await {
            Either::Left((result, _)) => {
                result.unwrap_or_else(|_| Err("Snapshot renderer closed".into()))
            }
            Either::Right(_) => Err("Snapshot rendering timed out".into()),
        }
    })
}

impl Surface {
    fn new(cx: &mut Context<Self>) -> Self {
        let (events, mut incoming) = mpsc::unbounded_channel();
        let task = cx.spawn(async move |surface, cx| {
            while let Some(event) = incoming.recv().await {
                if surface
                    .update(cx, |surface, cx| surface.accept(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            view: None,
            parent: None,
            loaded: false,
            generation: 0,
            serial: 0,
            queue: VecDeque::new(),
            active: None,
            events,
            _events: task,
            work: None,
            deadline: None,
        }
    }

    fn enqueue(
        &mut self,
        request: Request,
        sender: oneshot::Sender<Result<Snapshot, String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.queue.retain(|job| !job.sender.is_closed());
        if self.queue.len() >= QUEUE {
            let _ = sender.send(Err("Snapshot renderer is busy".into()));
            return;
        }
        if self.view.is_none() {
            if let Err(error) = self.create(window) {
                let _ = sender.send(Err(error));
                return;
            }
            self.start_deadline(cx);
        }
        self.serial += 1;
        self.queue.push_back(Job {
            id: self.serial,
            request,
            sender,
        });
        self.dispatch(cx);
    }

    fn create(&mut self, window: &mut Window) -> Result<(), String> {
        self.generation += 1;
        let generation = self.generation;
        let loaded = self.events.clone();
        let handle = HasWindowHandle::window_handle(window).map_err(|error| error.to_string())?;
        let view = wry::WebViewBuilder::new()
            .with_html(BLANK)
            .with_incognito(true)
            .with_focused(false)
            .with_visible(false)
            .with_background_throttling(wry::BackgroundThrottlingPolicy::Disabled)
            .with_bounds(bounds())
            .with_navigation_handler(|url| url == "about:blank" || url == "about:srcdoc")
            .with_download_started_handler(|_, _| false)
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_on_page_load_handler(move |event, _| {
                if matches!(event, wry::PageLoadEvent::Finished) {
                    let _ = loaded.send(Event::Loaded(generation));
                }
            })
            .build_as_child(&handle)
            .map_err(|error| error.to_string())?;
        self.parent = Some(Window::window_handle(window));
        self.view = Some(view);
        self.loaded = false;
        Ok(())
    }

    fn resume(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view.is_some() {
            return;
        }
        if let Err(error) = self.create(window) {
            self.close(&error);
            return;
        }
        self.start_deadline(cx);
    }

    fn accept(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Loaded(generation) => {
                // Later loads belong to jobs; script results work after the first.
                if generation == self.generation && self.view.is_some() && !self.loaded {
                    self.loaded = true;
                    self.deadline = None;
                    self.dispatch(cx);
                }
            }
            Event::State(generation, raw) => {
                if generation == self.generation
                    && let Some(state) = state(&raw)
                {
                    self.advance(state, cx);
                }
            }
        }
    }

    fn dispatch(&mut self, cx: &mut Context<Self>) {
        if !self.loaded || self.active.is_some() {
            return;
        }
        let Some(view) = self.view.as_ref() else {
            return;
        };
        while let Some(job) = self.queue.pop_front() {
            if job.sender.is_closed() {
                continue;
            }
            let page = crate::content::files::html::surface::snapshot(
                &job.request.html,
                &job.request.palette,
                LAYOUT,
                VIEWPORT,
                job.id,
            );
            let shown = view
                .set_bounds(bounds())
                .and_then(|()| view.set_visible(true))
                .and_then(|()| view.load_html(&page));
            if let Err(error) = shown {
                let _ = job.sender.send(Err(error.to_string()));
                continue;
            }
            self.active = Some((job, Phase::Measuring));
            self.start_deadline(cx);
            self.poll(cx);
            return;
        }
        let _ = view.set_visible(false);
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        let generation = self.generation;
        self.work = Some(cx.spawn(async move |surface, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let polled = surface.update(cx, |surface, _| surface.evaluate(generation));
                if !matches!(polled, Ok(true)) {
                    return;
                }
            }
        }));
    }

    fn evaluate(&self, generation: u64) -> bool {
        if generation != self.generation
            || !matches!(self.active, Some((_, Phase::Measuring | Phase::Placing(_))))
        {
            return false;
        }
        let Some(view) = &self.view else {
            return false;
        };
        let events = self.events.clone();
        view.evaluate_script_with_callback(STATE, move |raw| {
            let _ = events.send(Event::State(generation, raw));
        })
        .is_ok()
    }

    fn advance(&mut self, state: State, cx: &mut Context<Self>) {
        let Some((job, phase)) = self.active.as_ref() else {
            return;
        };
        if state.job != Some(job.id) {
            return;
        }
        if job.sender.is_closed() {
            return self.finish(Err("Snapshot request was dropped".into()), cx);
        }
        let phase = *phase;
        match phase {
            Phase::Measuring => {
                let Some(measure) = state.measure else {
                    return;
                };
                let Some(placement) = place(&measure) else {
                    return self.finish(Err("Preview has no visible content".into()), cx);
                };
                let script = format!("window.sailryPlace({},{})", placement.x, placement.y);
                let placed = self
                    .view
                    .as_ref()
                    .is_some_and(|view| view.evaluate_script(&script).is_ok());
                if !placed {
                    return self.finish(Err("Snapshot placement failed".into()), cx);
                }
                if let Some((_, phase)) = &mut self.active {
                    *phase = Phase::Placing(placement);
                }
            }
            Phase::Placing(placement) if state.placed => {
                if let Some((_, phase)) = &mut self.active {
                    *phase = Phase::Capturing;
                }
                self.capture(placement, cx);
            }
            _ => (),
        }
    }

    fn capture(&mut self, placement: Placement, cx: &mut Context<Self>) {
        let Some((job, _)) = self.active.as_ref() else {
            return;
        };
        let (width, pixels) = (job.request.width, job.request.pixels);
        // WebKit draws the layout at the card width; WebView2 copies the full
        // layout frame, which `crop` scales down.
        let pending = self
            .view
            .as_ref()
            .map(|view| crate::ui::webview_capture::capture(view, Some(f64::from(width)), true));
        let Some(Ok(receiver)) = pending else {
            return self.finish(Err("Snapshot capture failed".into()), cx);
        };
        let generation = self.generation;
        self.work = Some(cx.spawn(async move |surface, cx| {
            let encoded = match receiver.await {
                Ok(Ok(buffer)) => {
                    cx.background_executor()
                        .spawn(async move { crop(&buffer.bytes, buffer.format, placement, pixels) })
                        .await
                }
                _ => Err("Snapshot capture failed".into()),
            };
            let result = encoded.map(|bytes| Snapshot {
                image: Arc::new(gpui_kit::Image::from_bytes(
                    gpui_kit::ImageFormat::Png,
                    bytes,
                )),
                width: LAYOUT,
                height: placement.height,
            });
            let _ = surface.update(cx, |surface, cx| {
                if surface.generation == generation
                    && matches!(surface.active, Some((_, Phase::Capturing)))
                {
                    surface.finish(result, cx);
                }
            });
        }));
    }

    fn finish(&mut self, result: Result<Snapshot, String>, cx: &mut Context<Self>) {
        self.deadline = None;
        self.work = None;
        if let Some((job, _)) = self.active.take() {
            let _ = job.sender.send(result);
        }
        self.dispatch(cx);
    }

    fn start_deadline(&mut self, cx: &mut Context<Self>) {
        let generation = self.generation;
        self.deadline = Some(cx.spawn(async move |surface, cx| {
            cx.background_executor().timer(JOB_DEADLINE).await;
            let _ = surface.update(cx, |surface, cx| {
                if surface.generation == generation {
                    surface.recover("Snapshot rendering timed out", cx);
                }
            });
        }));
    }

    /// A page that stops answering may hold its content process, so it is
    /// replaced; queued jobs continue on a new page in the same window.
    fn recover(&mut self, message: &str, cx: &mut Context<Self>) {
        self.reset(message);
        self.queue.retain(|job| !job.sender.is_closed());
        let Some(parent) = self.parent.filter(|_| !self.queue.is_empty()) else {
            return;
        };
        let surface = cx.entity();
        cx.defer(move |cx| {
            let resumed = parent.update(cx, |_, window, cx| {
                surface.update(cx, |surface, cx| surface.resume(window, cx))
            });
            if resumed.is_err() {
                surface.update(cx, |surface, _| surface.close("Snapshot renderer closed"));
            }
        });
    }

    fn reset(&mut self, message: &str) {
        self.deadline = None;
        self.work = None;
        self.loaded = false;
        self.view.take();
        if let Some((job, _)) = self.active.take() {
            let _ = job.sender.send(Err(message.into()));
        }
    }

    fn close(&mut self, message: &str) {
        self.reset(message);
        self.parent = None;
        for job in self.queue.drain(..) {
            let _ = job.sender.send(Err(message.into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn reads_double_encoded_state() {
        let raw = serde_json::to_string(
            r#"{"job":3,"measure":{"left":8,"top":8,"width":300,"height":120,"scroll_height":408,"opaque":false},"placed":false}"#,
        )
        .unwrap();
        let parsed = state(&raw).unwrap();
        assert_eq!(parsed.job, Some(3));
        assert!(!parsed.placed);
        assert_eq!(parsed.measure.unwrap().width, 300.);
        let empty = serde_json::to_string(r#"{"job":null,"measure":null,"placed":false}"#).unwrap();
        assert!(state(&empty).is_some_and(|empty| empty.job.is_none() && empty.measure.is_none()));
        for invalid in ["", "null", "{\"job\":1}", "\"not json\""] {
            assert!(state(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn renders_desktop_viewport_outside_the_window() {
        let rect = bounds();
        let wry::dpi::Position::Logical(position) = rect.position else {
            panic!("snapshot bounds use logical coordinates");
        };
        let wry::dpi::Size::Logical(size) = rect.size else {
            panic!("snapshot bounds use logical coordinates");
        };
        assert!(position.x + size.width <= 0.);
        assert_eq!(size.width, f64::from(LAYOUT));
        assert_eq!(size.height, f64::from(VIEWPORT));
    }
}
