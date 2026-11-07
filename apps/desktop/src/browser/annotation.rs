//! Native selection and attachment transfer retain the captured page and controller.
//! A plain pick becomes a composer chip directly; a review pick waits for the
//! package's note flow.
use super::*;
use sailry_link::CancellationToken;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

pub(crate) mod context;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(crate) mod crop;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native;
#[cfg(test)]
mod tests;

actions!(browser, [SelectElement]);

/// Attribute names a capture may carry; input values are never captured.
const ATTRIBUTES: &[&str] = &[
    "role",
    "type",
    "name",
    "href",
    "src",
    "alt",
    "title",
    "placeholder",
    "for",
    "aria-label",
    "aria-describedby",
    "aria-expanded",
    "aria-selected",
    "aria-checked",
    "aria-disabled",
    "aria-current",
    "data-testid",
    "data-test",
    "data-qa",
    "data-cy",
];
const STYLES: &[&str] = &[
    "color",
    "background-color",
    "font-size",
    "font-family",
    "display",
    "position",
    "padding",
    "margin",
];

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Element {
    pub url: String,
    pub title: String,
    pub selector: String,
    pub path: String,
    pub tag: String,
    pub id: String,
    pub classes: Vec<String>,
    pub attributes: BTreeMap<String, String>,
    /// Accessible name.
    pub name: String,
    pub text: String,
    pub html: String,
    pub styles: BTreeMap<String, String>,
    pub component: Option<Component>,
    pub bounds: [f64; 4],
    pub viewport: [f64; 2],
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Framework {
    React,
    Vue,
    Svelte,
    Angular,
}

/// Development-build component metadata; production pages omit it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Component {
    pub framework: Framework,
    pub name: String,
    pub source: Option<String>,
    pub stack: Vec<String>,
}

/// Hover-card facts retained with a composer chip.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Details {
    pub selector: String,
    pub component: String,
    pub page: String,
}

/// One picked element ready for the captured composer.
pub(crate) struct Selection {
    pub label: String,
    pub context: Arc<[u8]>,
    pub image: Arc<[u8]>,
    pub details: Details,
    /// The package's optional note, inserted after the chip as user text.
    pub note: String,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Message {
    Element {
        token: String,
        element: Box<Element>,
        review: bool,
    },
    Cancelled {
        token: String,
    },
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
    timeout: Option<Task<()>>,
    work: Option<Task<()>>,
    picks: VecDeque<(Box<Element>, bool)>,
    capturing: bool,
    reviewing: bool,
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
        self.timeout = None;
        self.work = None;
        self.picks.clear();
        self.capturing = false;
        self.reviewing = false;
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
    pub selection: Selection,
    pub stop: CancellationToken,
    pub reply: RefCell<Option<tokio::sync::oneshot::Sender<Result<(), &'static str>>>>,
}

fn bounded(value: &str, limit: usize) -> bool {
    value.len() <= limit && !value.contains('\0')
}

impl Component {
    fn validate(&self) -> bool {
        !self.name.trim().is_empty()
            && bounded(&self.name, 128)
            && self
                .source
                .as_deref()
                .is_none_or(|source| !source.is_empty() && bounded(source, 512))
            && self.stack.len() <= 8
            && self
                .stack
                .iter()
                .all(|name| !name.is_empty() && bounded(name, 128))
    }
}

impl Element {
    fn validate(&self) -> bool {
        url::Url::parse(&self.url).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
        }) && self.url.len() <= 8192
            && bounded(&self.title, 1024)
            && !self.selector.is_empty()
            && bounded(&self.selector, 2048)
            && bounded(&self.path, 2048)
            && !self.tag.is_empty()
            && self.tag.len() <= 64
            && self
                .tag
                .chars()
                .all(|letter| letter.is_ascii_alphanumeric() || matches!(letter, '-' | '_' | ':'))
            && bounded(&self.id, 256)
            && self.classes.len() <= 16
            && self.classes.iter().all(|class| {
                !class.is_empty() && bounded(class, 128) && !class.contains(char::is_whitespace)
            })
            && self.attributes.len() <= 16
            && self
                .attributes
                .iter()
                .all(|(name, value)| ATTRIBUTES.contains(&name.as_str()) && bounded(value, 512))
            && bounded(&self.name, 256)
            && self.text.chars().count() <= 500
            && bounded(&self.text, 2000)
            && bounded(&self.html, 2048)
            && self
                .styles
                .iter()
                .all(|(name, value)| STYLES.contains(&name.as_str()) && bounded(value, 256))
            && self.component.as_ref().is_none_or(Component::validate)
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

    fn selection(&self, image: Arc<[u8]>, note: String) -> Selection {
        Selection {
            label: context::label(self),
            context: context::document(self).into(),
            image,
            details: context::details(self),
            note,
        }
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
    /// The reviewed note for the original capture, or nothing for altered context.
    fn note(value: &str, element: &Element) -> Option<String> {
        if !element.validate() || value.len() > 256 * 1024 {
            return None;
        }
        serde_json::from_str::<Self>(value)
            .ok()
            .filter(|reference| {
                reference.page_element == *element
                    && reference.annotation.len() <= 4096
                    && !reference.annotation.contains('\0')
            })
            .map(|reference| reference.annotation)
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
        let Some(note) = Reference::note(&request.reference, &capture.element) else {
            let _ = reply.send(Err("browser_annotation_invalid"));
            return;
        };
        cx.emit(Transfer {
            selection: capture.element.selection(capture.image.clone(), note),
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
            && self.mounted.get()
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

    /// The shortcut toggles picking; its failure reaches the package as feedback.
    pub(super) fn annotate_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.annotate(window, cx).is_err() {
            self.annotation.error = Some("browser_annotation_unavailable");
            self.changed(cx);
        }
    }
}
