//! Native selection and attachment transfer retain the captured page and controller.
use super::*;
use sailry_link::CancellationToken;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, sync::Arc};

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Element {
    pub url: String,
    pub selector: String,
    pub text: String,
    pub bounds: [f64; 4],
    pub viewport: [f64; 2],
    pub truncated: bool,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Message {
    Element { token: String, element: Element },
    Cancelled { token: String },
}

#[derive(Clone)]
struct Target {
    token: String,
    tab: usize,
    url: String,
    lease: u64,
    stop: CancellationToken,
}

#[derive(Default)]
pub(super) struct State {
    target: Option<Target>,
    task: Option<Task<()>>,
    captured: bool,
    capture: Option<Capture>,
    pub(super) error: Option<&'static str>,
}

struct Capture {
    element: Element,
    image: Arc<[u8]>,
    preview: Arc<Image>,
}

impl State {
    pub fn active(&self) -> bool {
        self.target.is_some()
    }
    pub fn tab(&self) -> Option<usize> {
        self.target.as_ref().map(|target| target.tab)
    }
    pub fn clear(&mut self) {
        if let Some(target) = self.target.take() {
            target.stop.cancel();
        }
        self.task = None;
        self.captured = false;
        self.capture = None;
        self.error = None;
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.clear();
    }
}

pub(crate) struct Transfer {
    pub reference: String,
    pub image: Arc<[u8]>,
    pub stop: CancellationToken,
    pub reply: RefCell<Option<tokio::sync::oneshot::Sender<Result<(), &'static str>>>>,
}

impl Element {
    fn validate(&self) -> bool {
        url::Url::parse(&self.url).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
        }) && self.url.len() <= 8192
            && !self.selector.is_empty()
            && self.selector.len() <= 2048
            && !self.selector.contains('\0')
            && self.text.len() <= 16 * 1024
            && !self.text.contains('\0')
            && self
                .bounds
                .iter()
                .chain(self.viewport.iter())
                .all(|value| value.is_finite())
            && self
                .viewport
                .iter()
                .all(|value| (1.0..=16384.0).contains(value))
            && self.bounds[2] > 0.0
            && self.bounds[3] > 0.0
            && self.bounds[2] <= 1_000_000.0
            && self.bounds[3] <= 1_000_000.0
            && self.bounds[0] < self.viewport[0]
            && self.bounds[1] < self.viewport[1]
            && self.bounds[0] + self.bounds[2] > 0.0
            && self.bounds[1] + self.bounds[3] > 0.0
    }
}

pub(crate) struct Submit {
    pub token: String,
    pub reference: String,
    pub stop: CancellationToken,
    pub reply: RefCell<Option<tokio::sync::oneshot::Sender<Result<(), &'static str>>>>,
}
impl EventEmitter<Submit> for Browser {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    page_element: Element,
    annotation: String,
}

impl Reference {
    fn matches(value: &str, element: &Element) -> bool {
        element.validate()
            && value.len() <= 256 * 1024
            && serde_json::from_str::<Self>(value).is_ok_and(|reference| {
                reference.page_element == *element
                    && reference.annotation.len() <= 4096
                    && !reference.annotation.contains('\0')
            })
    }
}

impl Browser {
    pub(crate) fn annotation_snapshot(&self) -> Option<serde_json::Value> {
        let target = self.annotation.target.as_ref()?;
        let capture = self.annotation.capture.as_ref()?;
        self.annotation_matches(target)
            .then(|| serde_json::json!({"token":target.token,"element":capture.element}))
    }

    pub(crate) fn annotation_preview(&self, token: &str) -> Option<Arc<Image>> {
        let target = self.annotation.target.as_ref()?;
        (target.token == token && self.annotation_matches(target))
            .then(|| {
                self.annotation
                    .capture
                    .as_ref()
                    .map(|capture| capture.preview.clone())
            })
            .flatten()
    }

    pub(crate) fn submit_annotation(&self, request: &Submit, cx: &mut Context<Self>) {
        let Some(reply) = request.reply.borrow_mut().take() else {
            return;
        };
        let target = self.annotation.target.as_ref().filter(|target| {
            !request.stop.is_cancelled()
                && target.token == request.token
                && self.annotation_matches(target)
        });
        let Some((target, capture)) = target.zip(self.annotation.capture.as_ref()) else {
            let _ = reply.send(Err("browser_annotation_stale"));
            return;
        };
        if !Reference::matches(&request.reference, &capture.element) {
            let _ = reply.send(Err("browser_annotation_invalid"));
            return;
        }
        cx.emit(Transfer {
            reference: request.reference.clone(),
            image: capture.image.clone(),
            stop: target.stop.clone(),
            reply: RefCell::new(Some(reply)),
        });
    }

    pub(super) fn cancel_captured_annotation(&mut self, token: &str, cx: &mut Context<Self>) {
        if self
            .annotation
            .target
            .as_ref()
            .is_some_and(|target| target.token == token)
        {
            self.cancel_annotation(cx);
            self.changed(cx);
        }
    }
    fn annotation_matches(&self, target: &Target) -> bool {
        !target.stop.is_cancelled()
            && self.mounted
            && self.lease == target.lease
            && self
                .annotation
                .target
                .as_ref()
                .is_some_and(|active| active.token == target.token)
            && self.tabs.get(self.selected).is_some_and(|tab| {
                tab.id == target.tab && tab.url == target.url && !tab.loading && tab.error.is_none()
            })
    }

    pub(super) fn cancel_annotation(&mut self, cx: &mut Context<Self>) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(target) = &self.annotation.target
            && let Some(page) = self
                .tabs
                .iter()
                .find(|tab| tab.id == target.tab)
                .and_then(|tab| tab.page.as_ref())
        {
            let _ = page
                .read(cx)
                .raw()
                .evaluate_script("window.__sailryElementSelection?.cancel()");
        }
        self.annotation.clear();
        cx.notify();
    }

    pub(super) fn annotate(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.annotation.active() {
            self.cancel_annotation(cx);
            self.changed(cx);
            return Ok(());
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        return self.begin_annotation(window, cx);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (window, cx);
            Err("browser_annotation_unavailable".into())
        }
    }
}
