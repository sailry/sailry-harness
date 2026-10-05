//! Presentation for explicit OS access requests; platform and Node owners keep authority.
use crate::{theme::DialogStyle, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
};
use gpui_kit::{component::*, prelude::FluentBuilder as _, *};
use sailry_link::CancellationToken;
use std::{cell::RefCell, rc::Rc};

mod app;
mod microphone;
#[cfg(test)]
mod tests;
mod view;
pub(crate) use microphone::open as microphone;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resource {
    FullDisk,
    Screen,
    Accessibility,
    Microphone,
    Chrome,
    Keychain,
}
impl Resource {
    fn is_system(self) -> bool {
        !matches!(self, Self::Chrome | Self::Keychain)
    }
    fn key(self) -> &'static str {
        match self {
            Self::FullDisk => "permission_full_disk",
            Self::Screen => "permission_screen",
            Self::Accessibility => "permission_accessibility",
            Self::Microphone => "permission_microphone",
            Self::Chrome => "permission_chrome",
            Self::Keychain => "permission_keychain",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Unknown,
    Required,
    Granted,
    NotNeeded,
    Denied,
    Restricted,
    Remote,
    Unavailable,
}
impl Status {
    fn key(self) -> &'static str {
        match self {
            Self::Unknown => "permission_unknown",
            Self::Required => "permission_required",
            Self::Granted => "permission_granted",
            Self::NotNeeded => "permission_not_needed",
            Self::Denied => "permission_denied",
            Self::Restricted => "permission_restricted",
            Self::Remote => "permission_remote",
            Self::Unavailable => "permission_unavailable",
        }
    }
}
pub(crate) struct Failure {
    pub key: String,
    pub status: Status,
}
pub(crate) type Changes = Vec<(Resource, Status)>;
pub(crate) type Action = Rc<dyn Fn(&mut App, CancellationToken) -> Task<Result<Changes, Failure>>>;
#[derive(Clone)]
pub(crate) struct Card {
    pub resource: Resource,
    pub status: Status,
    pub settings: Option<&'static str>,
    pub check: Option<Action>,
    pub request: Option<Action>,
    pub requires: Option<Resource>,
}
pub(crate) type Completion = Box<dyn FnOnce(bool, &mut Window, &mut App)>;
pub(crate) struct Request {
    pub cards: Vec<Card>,
    pub stop: CancellationToken,
    pub completion: RefCell<Option<Completion>>,
}
pub(crate) struct Controller;
impl EventEmitter<Request> for Controller {}
impl Controller {
    pub(crate) fn new(window: &Window, cx: &mut Context<Self>) -> Self {
        cx.subscribe_in(
            &cx.entity(),
            window,
            |_, _, request: &Request, window, cx| {
                if let Some(done) = request.completion.borrow_mut().take() {
                    open(
                        request.cards.clone(),
                        request.stop.clone(),
                        done,
                        window,
                        cx,
                    );
                }
            },
        )
        .detach();
        Self
    }
}
pub(crate) struct Panel {
    cards: Vec<Card>,
    required: Vec<Resource>,
    continuing: bool,
    active: bool,
    refresh: bool,
    pending: Option<Resource>,
    closed: bool,
    stop: CancellationToken,
    completion: Option<Completion>,
    task: Option<Task<()>>,
    checking: bool,
}
impl Drop for Panel {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
pub(crate) fn open(
    cards: Vec<Card>,
    stop: CancellationToken,
    done: Completion,
    window: &mut Window,
    cx: &mut App,
) {
    if stop.is_cancelled() {
        window.defer(cx, move |window, cx| done(false, window, cx));
        return;
    }
    let external = stop;
    let flow = cx.new(|cx| {
        cx.observe_window_activation(window, |flow: &mut Panel, window, cx| {
            if flow.active && window.is_window_active() {
                flow.check(window, cx);
            }
        })
        .detach();
        let required = cards.iter().map(|card| card.resource).collect();
        Panel {
            cards: app::catalog(cards, cx),
            required,
            continuing: false,
            active: true,
            refresh: false,
            pending: None,
            closed: false,
            stop: CancellationToken::new(),
            completion: Some(done),
            task: None,
            checking: false,
        }
    });
    let closed = flow.downgrade();
    let finished = flow.read(cx).stop.clone();
    window
        .spawn(cx, async move |cx| {
            tokio::select! {
                biased;
                _ = finished.cancelled() => return,
                _ = external.cancelled() => {},
            }
            let _ = closed.update_in(cx, |flow, window, cx| {
                if !flow.closed {
                    flow.finish(false, window, cx);
                    window.close_dialog(cx);
                }
            });
        })
        .detach();
    let cancel = flow.clone();
    let close = flow.clone();
    let initial = flow.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let footer = flow.clone();
        let proceed = flow.clone();
        let has_workflow = flow.read(cx).has_workflow();
        dialog
            .form_title(
                div()
                    .debug_selector(|| "permissions-title".into())
                    .child(tr("permission_title")),
            )
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .on_ok(|_, _, _| false)
            .on_cancel({
                let cancel = cancel.clone();
                move |_, window, cx| {
                    cancel.update(cx, |flow, cx| flow.finish(false, window, cx));
                    true
                }
            })
            .on_close({
                let close = close.clone();
                move |_, window, cx| {
                    close.update(cx, |flow, cx| flow.finish(false, window, cx));
                }
            })
            .child(flow.clone())
            .footer(
                DialogFooter::new()
                    .w_full()
                    .when(has_workflow, |footer| {
                        let panel = proceed.read(cx);
                        footer.child(
                            Button::new("permissions-continue")
                                .primary()
                                .label(tr("permission_continue"))
                                .loading(panel.pending.is_some())
                                .disabled(
                                    panel.checking || panel.pending.is_some() || panel.remote(),
                                )
                                .debug_selector(|| "permissions-continue".into())
                                .on_click(move |_, window, cx| {
                                    proceed.update(cx, |panel, cx| panel.proceed(window, cx));
                                }),
                        )
                    })
                    .child(
                        Button::new("permissions-cancel")
                            .label(tr("settings_cancel"))
                            .debug_selector(|| "permissions-cancel".into())
                            .on_click(move |_, window, cx| {
                                footer.update(cx, |flow, cx| flow.finish(false, window, cx));
                                window.close_dialog(cx);
                            }),
                    ),
            )
    });
    // Native prompts and operation access start only from explicit actions.
    initial.update(cx, |flow, cx| flow.check(window, cx));
}
impl Panel {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_window_activation(window, |panel: &mut Self, window, cx| {
            if panel.active && window.is_window_active() {
                panel.check(window, cx);
            }
        })
        .detach();
        Self {
            cards: app::catalog(Vec::new(), cx),
            required: Vec::new(),
            continuing: false,
            active: false,
            refresh: false,
            pending: None,
            closed: false,
            stop: CancellationToken::new(),
            completion: None,
            task: None,
            checking: false,
        }
    }
    pub(crate) fn activate(&mut self, active: bool, cx: &mut Context<Self>) {
        self.active = active;
        self.refresh = active;
        cx.notify();
    }
    fn remote(&self) -> bool {
        self.cards.iter().any(|card| card.status == Status::Remote)
    }
    fn has_workflow(&self) -> bool {
        self.required.iter().any(|resource| !resource.is_system())
    }
    fn proceed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.checking || self.pending.is_some() || self.remote() {
            return;
        }
        let next = self
            .required
            .iter()
            .find(|resource| {
                !resource.is_system()
                    && self.cards.iter().any(|card| {
                        card.resource == **resource
                            && !matches!(card.status, Status::Granted | Status::NotNeeded)
                    })
            })
            .copied();
        if let Some(resource) = next {
            self.continuing = true;
            self.request(resource, window, cx);
        } else {
            self.complete(window, cx);
        }
    }
    fn disk_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self
            .cards
            .iter()
            .find(|card| card.resource == Resource::FullDisk)
        else {
            return;
        };
        if matches!(card.status, Status::Remote | Status::Unavailable) {
            return;
        }
        let Some(url) = card.settings else {
            return;
        };
        // A denied protected directory open can register the app before showing the pane.
        let Some(check) = card.check.clone() else {
            cx.open_url(url);
            return;
        };
        self.pending = Some(Resource::FullDisk);
        let stop = self.stop.clone();
        let job = check(cx, stop.clone());
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            if stop.is_cancelled() {
                return;
            }
            let _ = owner.update_in(cx, |panel, _, cx| {
                panel.pending = None;
                if let Ok(changes) = result {
                    panel.apply(changes);
                }
                cx.open_url(url);
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn finish(&mut self, granted: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.stop.cancel();
        self.task = None;
        if let Some(done) = self.completion.take() {
            done(granted, window, cx);
        }
    }
    fn apply(&mut self, changes: Changes) {
        for (resource, status) in changes {
            if let Some(card) = self.cards.iter_mut().find(|card| card.resource == resource) {
                card.status = status;
            }
        }
    }
    fn complete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.completion.is_some()
            && !self.required.is_empty()
            && self.required.iter().all(|resource| {
                self.cards.iter().any(|card| {
                    card.resource == *resource
                        && matches!(card.status, Status::Granted | Status::NotNeeded)
                })
            })
        {
            self.finish(true, window, cx);
            window.close_dialog(cx);
        }
    }
    fn check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.pending.is_some() || self.checking {
            return;
        }
        if self.remote() {
            for card in &mut self.cards {
                if card.resource.is_system() {
                    card.status = Status::Remote;
                }
            }
            cx.notify();
            return;
        }
        let mut checks: Vec<_> = self
            .cards
            .iter()
            .filter_map(|card| card.check.clone().map(|check| (card.resource, check)))
            .collect();
        checks.sort_by_key(|(resource, _)| !self.required.contains(resource));
        if checks.is_empty() {
            return;
        }
        self.checking = true;
        let stop = self.stop.clone();
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            for (resource, check) in checks {
                let task = cx.update(|_, cx| check(cx, stop.clone()));
                let Ok(task) = task else {
                    return;
                };
                let result = task.await;
                if stop.is_cancelled() {
                    return;
                }
                let remote = owner
                    .update_in(cx, |flow, window, cx| {
                        match result {
                            Ok(changes) => flow.apply(changes),
                            Err(failure) => {
                                flow.apply(vec![(resource, failure.status)]);
                                crate::feedback::info("", tr(&failure.key).as_ref(), window, cx);
                            }
                        }
                        if flow.remote() {
                            for card in &mut flow.cards {
                                if card.resource.is_system() {
                                    card.status = Status::Remote;
                                }
                            }
                        }
                        cx.notify();
                        flow.remote()
                    })
                    .unwrap_or(true);
                if remote {
                    break;
                }
            }
            let _ = owner.update_in(cx, |flow, window, cx| {
                flow.checking = false;
                flow.complete(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn request(&mut self, resource: Resource, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.pending.is_some() || self.checking {
            return;
        }
        let Some(card) = self.cards.iter().find(|card| card.resource == resource) else {
            return;
        };
        if resource == Resource::FullDisk {
            self.disk_settings(window, cx);
            return;
        }
        if matches!(card.status, Status::Denied | Status::Restricted)
            && let Some(settings) = card.settings
            && resource != Resource::Chrome
        {
            cx.open_url(settings);
            return;
        }
        if matches!(
            card.status,
            Status::Granted | Status::Remote | Status::Unavailable | Status::Restricted
        ) {
            return;
        }
        if card.requires.is_some_and(|required| {
            !self
                .cards
                .iter()
                .any(|card| card.resource == required && card.status == Status::Granted)
        }) {
            return;
        }
        let Some(action) = card.request.clone() else {
            return;
        };
        self.pending = Some(resource);
        let stop = self.stop.clone();
        let job = action(cx, stop.clone());
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            if stop.is_cancelled() {
                return;
            }
            let _ = owner.update_in(cx, |flow, window, cx| {
                flow.pending = None;
                let advanced = result.as_ref().is_ok_and(|changes| {
                    changes.iter().any(|(changed, status)| {
                        *changed == resource
                            && matches!(status, Status::Granted | Status::NotNeeded)
                    })
                });
                match result {
                    Ok(changes) => flow.apply(changes),
                    Err(failure) => {
                        flow.continuing = false;
                        flow.apply(vec![(resource, failure.status)]);
                        crate::feedback::info("", tr(&failure.key).as_ref(), window, cx);
                    }
                }
                flow.complete(window, cx);
                if !flow.closed {
                    if flow.continuing && advanced {
                        flow.proceed(window, cx);
                    } else {
                        flow.continuing = false;
                        flow.check(window, cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
