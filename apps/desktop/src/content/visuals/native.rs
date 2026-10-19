use futures::{
    future::{Either, select},
    pin_mut,
};
use gpui_kit::{AnyWindowHandle, App, AppContext, Context, Entity, Global, Task, Window};
use raw_window_handle::HasWindowHandle;
use sailry_content::markdown::visuals::{self, Renderer, Request, Svg};
use serde::Deserialize;
use std::{borrow::Cow, collections::VecDeque, rc::Rc, time::Duration};
use tokio::sync::{mpsc, oneshot};

const URL: &str = "sailry-visual://localhost/index.html";
const DEADLINE: Duration = Duration::from_secs(30);
const BOOT: &str = r#"(() => {
  const fail = error => window.ipc.postMessage(JSON.stringify({
    id: 0, result: null,
    error: String(error?.message || error || 'Visual renderer failed').slice(0, 2048)
  }));
  window.addEventListener('error', event => fail(event.error || event.message || 'Visual asset could not load'), true);
  window.addEventListener('unhandledrejection', event => fail(event.reason));
  window.addEventListener('securitypolicyviolation', event => fail(`Visual asset blocked by ${event.violatedDirective}`));
})();"#;

struct Service(Entity<Surface>);
impl Global for Service {}

struct Job {
    id: u64,
    request: Request,
    sender: oneshot::Sender<Result<Svg, String>>,
}

enum Event {
    Ready(u64),
    Result(u64, Message),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Message {
    id: u64,
    result: Option<Svg>,
    error: Option<String>,
}

struct Surface {
    view: Option<wry::WebView>,
    parent: Option<AnyWindowHandle>,
    ready: bool,
    serial: u64,
    generation: u64,
    queue: VecDeque<Job>,
    active: Option<Job>,
    events: mpsc::UnboundedSender<Event>,
    _events: Task<()>,
    deadline: Option<Task<()>>,
}

pub(super) fn init(cx: &mut App) {
    if cx.has_global::<Service>() {
        return;
    }
    let surface = cx.new(Surface::new);
    cx.set_global(Service(surface.clone()));
    cx.set_global(Renderer(Rc::new(move |request, window, cx| {
        if let Err(error) = request.validate() {
            return Task::ready(Err(error));
        }
        let (sender, receiver) = oneshot::channel();
        surface.update(cx, |surface, cx| {
            surface.enqueue(request, sender, window, cx)
        });
        let timer = cx.background_executor().timer(DEADLINE);
        cx.spawn(async move |_| {
            pin_mut!(receiver, timer);
            match select(receiver, timer).await {
                Either::Left((result, _)) => {
                    result.unwrap_or_else(|_| Err("Visual renderer closed".into()))
                }
                Either::Right(_) => Err("Visual rendering timed out".into()),
            }
        })
    })));
}

pub(super) fn release(window: &mut Window, cx: &mut App) {
    let Some(service) = cx.try_global::<Service>().map(|service| service.0.clone()) else {
        return;
    };
    service.update(cx, |surface, _| {
        if surface.parent == Some(Window::window_handle(window)) {
            surface.close("Visual renderer closed");
        }
    });
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
            ready: false,
            serial: 0,
            generation: 0,
            queue: VecDeque::new(),
            active: None,
            events,
            _events: task,
            deadline: None,
        }
    }

    fn enqueue(
        &mut self,
        request: Request,
        sender: oneshot::Sender<Result<Svg, String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.queue.retain(|job| !job.sender.is_closed());
        if self.queue.len() >= 256 {
            let _ = sender.send(Err("Visual renderer is busy".into()));
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
        let events = self.events.clone();
        let loaded = self.events.clone();
        let handle = HasWindowHandle::window_handle(window).map_err(|error| error.to_string())?;
        let view = wry::WebViewBuilder::new()
            .with_custom_protocol("sailry-visual".into(), |_, request| asset(&request))
            .with_url(URL)
            .with_incognito(true)
            .with_visible(false)
            .with_background_throttling(wry::BackgroundThrottlingPolicy::Disabled)
            .with_initialization_script(BOOT)
            .with_bounds(wry::Rect {
                position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: wry::dpi::LogicalSize::new(640.0, 480.0).into(),
            })
            .with_navigation_handler(navigation)
            .with_download_started_handler(|_, _| false)
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_on_page_load_handler(move |event, _| {
                if matches!(event, wry::PageLoadEvent::Finished) {
                    let _ = loaded.send(Event::Ready(generation));
                }
            })
            .with_ipc_handler(move |request| {
                if request.body().len() <= visuals::MAX_SVG_BYTES + 4096
                    && let Ok(message) = serde_json::from_str::<Message>(request.body())
                {
                    let _ = events.send(Event::Result(generation, message));
                }
            })
            .build_as_child(&handle)
            .map_err(|error| error.to_string())?;
        self.parent = Some(Window::window_handle(window));
        self.view = Some(view);
        self.ready = false;
        Ok(())
    }

    fn accept(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Ready(generation) => {
                if generation != self.generation {
                    return;
                }
                if self.view.is_some() {
                    self.ready = true;
                    self.deadline = None;
                }
            }
            Event::Result(generation, message) => {
                if generation != self.generation {
                    return;
                }
                if message.id == 0 && message.result.is_none() {
                    if let Some(error) = message.error {
                        self.close(&error);
                    }
                    return;
                }
                if self
                    .active
                    .as_ref()
                    .is_none_or(|active| active.id != message.id)
                {
                    return;
                }
                self.deadline = None;
                let active = self
                    .active
                    .take()
                    .expect("Matching active visual request exists");
                let result = match (message.result, message.error) {
                    (Some(svg), None) => svg.validate().map(|()| svg),
                    (None, Some(error)) => Err(error),
                    _ => Err("Visual renderer returned an invalid result".into()),
                };
                let _ = active.sender.send(result);
            }
        }
        self.dispatch(cx);
    }

    fn dispatch(&mut self, cx: &mut Context<Self>) {
        if !self.ready || self.active.is_some() {
            return;
        }
        while let Some(job) = self.queue.pop_front() {
            if job.sender.is_closed() {
                continue;
            }
            let script = script(job.id, &job.request);
            if let Err(error) = self
                .view
                .as_ref()
                .expect("Ready rendering surface exists")
                .evaluate_script(&script)
            {
                let _ = job.sender.send(Err(error.to_string()));
                continue;
            }
            self.active = Some(job);
            self.start_deadline(cx);
            return;
        }
    }

    fn start_deadline(&mut self, cx: &mut Context<Self>) {
        self.deadline = Some(cx.spawn(async move |surface, cx| {
            cx.background_executor().timer(DEADLINE).await;
            let _ = surface.update(cx, |surface, _| {
                surface.close("Visual rendering timed out");
            });
        }));
    }

    fn close(&mut self, message: &str) {
        self.deadline = None;
        self.ready = false;
        self.parent = None;
        self.view.take();
        for job in self.active.take().into_iter().chain(self.queue.drain(..)) {
            let _ = job.sender.send(Err(message.into()));
        }
    }
}

fn navigation(url: String) -> bool {
    url == URL || url == "http://sailry-visual.localhost/index.html"
}

fn asset(request: &wry::http::Request<Vec<u8>>) -> wry::http::Response<Cow<'static, [u8]>> {
    let path = request.uri().path();
    let content = (request.method() == wry::http::Method::GET && request.uri().query().is_none())
        .then(|| visuals::asset(path))
        .flatten();
    let kind = if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else {
        "text/javascript; charset=utf-8"
    };
    wry::http::Response::builder()
        .status(if content.is_some() { 200 } else { 404 })
        .header("Content-Type", kind)
        .header("X-Content-Type-Options", "nosniff")
        .body(Cow::Borrowed(content.unwrap_or_default()))
        .expect("Static visual response headers are valid")
}

fn script(id: u64, request: &Request) -> String {
    let request = serde_json::to_string(request).expect("Validated request must serialize");
    format!(
        "Promise.resolve().then(()=>window.sailryRender({request})).then(result=>window.ipc.postMessage(JSON.stringify({{id:{id},result,error:null}})),error=>window.ipc.postMessage(JSON.stringify({{id:{id},result:null,error:String(error?.message||error).slice(0,2048)}})))"
    )
}

#[cfg(test)]
mod tests;
