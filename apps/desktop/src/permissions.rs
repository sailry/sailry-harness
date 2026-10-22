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
mod catalog;
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
    Partial,
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
            Self::Partial => "permission_partial",
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
    pub owner: Option<(sailry_protocol::NodeId, String)>,
    pub resource: Resource,
    pub status: Status,
    pub settings: Option<&'static str>,
    pub check: Option<Action>,
    pub request: Option<Action>,
    pub requires: Option<Resource>,
}

impl Card {
    fn settings_action(&self) -> Option<&'static str> {
        if matches!(self.status, Status::Remote | Status::Unavailable) {
            return None;
        }
        (self.resource == Resource::FullDisk
            || matches!(self.status, Status::Denied | Status::Restricted)
                && self.resource != Resource::Chrome)
            .then_some(self.settings)
            .flatten()
    }

    fn actionable(&self) -> bool {
        !matches!(
            self.status,
            Status::Granted | Status::NotNeeded | Status::Remote | Status::Unavailable
        ) && (self.settings_action().is_some() || self.request.is_some())
    }

    fn operation(&self) -> (Option<&'static str>, Option<Action>) {
        let settings = self.settings_action();
        let action = if settings.is_some() {
            (self.resource == Resource::FullDisk)
                .then(|| self.check.clone())
                .flatten()
        } else {
            self.request.clone()
        };
        (settings, action)
    }
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
    sources: Vec<sailry_client::permissions::Entry>,
    binding: Option<(sailry_protocol::NodeId, bool)>,
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
            sources: Vec::new(),
            binding: None,
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
                cx,
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
            sources: Vec::new(),
            binding: None,
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
            .find(|card| card.resource == Resource::FullDisk && card.settings_action().is_some())
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
        let identity = card.owner.clone();
        let job = check(cx, stop.clone());
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            if stop.is_cancelled() {
                return;
            }
            let _ = owner.update_in(cx, |panel, _, cx| {
                panel.pending = None;
                if let Ok(changes) = result {
                    panel.apply(&identity, changes);
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
    fn apply(&mut self, owner: &Option<(sailry_protocol::NodeId, String)>, changes: Changes) {
        for (resource, status) in changes {
            if let Some(card) = self
                .cards
                .iter_mut()
                .find(|card| card.resource == resource && &card.owner == owner)
            {
                card.status = status;
            }
        }
    }
    fn complete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.completion.is_some()
            && !self.required.is_empty()
            && self.required.iter().all(|resource| {
                self.cards
                    .iter()
                    .filter(|card| card.resource == *resource)
                    .all(|card| matches!(card.status, Status::Granted | Status::NotNeeded))
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
        let mut checks: Vec<_> = self
            .cards
            .iter()
            .filter_map(|card| {
                card.check
                    .clone()
                    .map(|check| (card.resource, card.owner.clone(), check))
            })
            .collect();
        checks.sort_by_key(|(resource, _, _)| !self.required.contains(resource));
        if checks.is_empty() {
            return;
        }
        self.checking = true;
        let stop = self.stop.clone();
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            for (resource, identity, check) in checks {
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
                            Ok(changes) => flow.apply(&identity, changes),
                            Err(failure) => {
                                flow.apply(&identity, vec![(resource, failure.status)]);
                                crate::feedback::info("", tr(&failure.key).as_ref(), window, cx);
                            }
                        }
                        cx.notify();
                        false
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
        let selected: std::collections::BTreeSet<_> =
            sailry_client::permissions::requests(self.sources.iter().filter(|entry| {
                self.cards.iter().any(|card| {
                    card.resource == resource
                        && card.owner.as_ref()
                            == Some(&(entry.identity.device, entry.identity.subject.clone()))
                        && card.resource == Resource::from_kind(entry.identity.kind)
                        && !matches!(
                            card.status,
                            Status::Granted
                                | Status::NotNeeded
                                | Status::Remote
                                | Status::Unavailable
                        )
                })
            }))
            .into_iter()
            .map(|entry| (entry.identity.device, entry.identity.subject.clone()))
            .collect();
        let requests: Vec<_> = self
            .cards
            .iter()
            .filter(|card| {
                card.resource == resource
                    && !matches!(
                        card.status,
                        Status::Granted | Status::NotNeeded | Status::Remote | Status::Unavailable
                    )
            })
            .filter(|card| {
                card.owner.as_ref().is_none_or(|owner| {
                    !self.sources.iter().any(|entry| {
                        entry.declaration.check.is_some()
                            && entry.identity.device == owner.0
                            && entry.identity.subject == owner.1
                    }) || selected.contains(owner)
                })
            })
            .cloned()
            .collect();
        if requests.is_empty() {
            return;
        }
        if requests.len() == 1
            && requests[0]
                .owner
                .as_ref()
                .is_none_or(|(_, subject)| subject == "sailry")
        {
            if resource == Resource::FullDisk {
                self.disk_settings(window, cx);
                return;
            }
            if matches!(requests[0].status, Status::Denied | Status::Restricted)
                && let Some(settings) = requests[0].settings
                && resource != Resource::Chrome
            {
                cx.open_url(settings);
                return;
            }
        }
        let requests: Vec<_> = requests
            .into_iter()
            .filter(|card| {
                card.requires.is_none_or(|required| {
                    self.cards.iter().any(|other| {
                        other.resource == required
                            && other.owner == card.owner
                            && matches!(other.status, Status::Granted | Status::NotNeeded)
                    })
                })
            })
            .collect();
        self.pending = Some(resource);
        let stop = self.stop.clone();
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let mut advanced = false;
            for card in requests {
                let (settings, action) = card.operation();
                let result = if let Some(action) = action {
                    let Ok(job) = cx.update(|_, cx| action(cx, stop.clone())) else {
                        return;
                    };
                    Some(job.await)
                } else if settings.is_some() {
                    None
                } else {
                    continue;
                };
                if stop.is_cancelled() {
                    return;
                }
                let _ = owner.update_in(cx, |flow, window, cx| {
                    match result {
                        Some(Ok(changes)) => {
                            advanced |= changes.iter().any(|(changed, status)| {
                                *changed == resource
                                    && matches!(status, Status::Granted | Status::NotNeeded)
                            });
                            flow.apply(&card.owner, changes);
                        }
                        Some(Err(failure)) => {
                            flow.continuing = false;
                            flow.apply(&card.owner, vec![(resource, failure.status)]);
                            crate::feedback::info("", tr(&failure.key).as_ref(), window, cx);
                        }
                        None => {}
                    }
                    if let Some(settings) = settings {
                        cx.open_url(settings);
                    }
                    cx.notify();
                });
            }
            let _ = owner.update_in(cx, |flow, window, cx| {
                flow.pending = None;
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
