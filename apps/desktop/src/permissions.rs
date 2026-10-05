//! Presentation for explicit OS access requests; platform and Node owners keep authority.
use crate::tr;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::{DialogFooter, DialogHeader, DialogTitle},
    group_box::{GroupBox, GroupBoxVariants},
};
use gpui_kit::{component::*, prelude::FluentBuilder as _, *};
use sailry_link::CancellationToken;
use std::{cell::RefCell, rc::Rc};

mod microphone;
#[cfg(test)]
mod tests;
pub(crate) use microphone::open as microphone;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resource {
    Screen,
    Accessibility,
    Microphone,
    Chrome,
    Keychain,
}
impl Resource {
    fn key(self) -> &'static str {
        match self {
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
struct Flow {
    cards: Vec<Card>,
    pending: Option<Resource>,
    closed: bool,
    stop: CancellationToken,
    completion: Option<Completion>,
    task: Option<Task<()>>,
    checking: bool,
}
impl Drop for Flow {
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
        cx.observe_window_activation(window, |flow: &mut Flow, window, cx| {
            if window.is_window_active() {
                flow.check(window, cx);
            }
        })
        .detach();
        Flow {
            cards,
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
    window.open_dialog(cx, move |dialog, window, _| {
        dialog
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .p_0()
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
    });
    // Checks never prompt. Actual authorization starts only from a card button.
    initial.update(cx, |flow, cx| flow.check(window, cx));
}
impl Flow {
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
        if self
            .cards
            .iter()
            .all(|card| matches!(card.status, Status::Granted | Status::NotNeeded))
        {
            self.finish(true, window, cx);
            window.close_dialog(cx);
        }
    }
    fn check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.pending.is_some() || self.checking {
            return;
        }
        let checks: Vec<_> = self
            .cards
            .iter()
            .filter_map(|card| card.check.clone().map(|check| (card.resource, check)))
            .collect();
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
                let _ = owner.update_in(cx, |flow, window, cx| {
                    match result {
                        Ok(changes) => flow.apply(changes),
                        Err(failure) => {
                            flow.apply(vec![(resource, failure.status)]);
                            crate::feedback::info("", tr(&failure.key).as_ref(), window, cx);
                        }
                    }
                    cx.notify();
                });
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
        if matches!(card.status, Status::Denied | Status::Restricted)
            && card.settings.is_some()
            && resource != Resource::Chrome
        {
            cx.open_url(card.settings.unwrap());
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
                match result {
                    Ok(changes) => flow.apply(changes),
                    Err(failure) => {
                        flow.apply(vec![(resource, failure.status)]);
                        crate::feedback::info("", tr(&failure.key).as_ref(), window, cx);
                    }
                }
                flow.complete(window, cx);
                if !flow.closed {
                    flow.check(window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
impl Render for Flow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .debug_selector(|| "permissions-modal".into())
            .child(
                DialogHeader::new().child(
                    v_flex()
                        .relative()
                        .overflow_hidden()
                        .p_6()
                        .gap_3()
                        .bg(cx.theme().sidebar)
                        .children(crate::theme::background("background.content", cx))
                        .child(
                            h_flex()
                                .relative()
                                .gap_3()
                                .child(crate::theme::brand(px(40.), cx).unwrap_or_else(|| {
                                    svg()
                                        .path("branding/sailry-mark.svg")
                                        .size_10()
                                        .into_any_element()
                                }))
                                .child(DialogTitle::new().child(tr("app"))),
                        )
                        .child(div().relative().text_sm().child(tr("permission_title"))),
                ),
            )
            .child(
                v_flex().p_6().gap_3().children(
                    self.cards
                        .iter()
                        .filter(|card| card.status != Status::NotNeeded)
                        .map(|card| {
                            let resource = card.resource;
                            let status = card.status;
                            let pending = self.pending == Some(resource);
                            let disabled = self.checking
                                || self.pending.is_some() && !pending
                                || matches!(
                                    card.status,
                                    Status::Granted | Status::Remote | Status::Unavailable
                                )
                                || card.status == Status::Restricted && card.settings.is_none()
                                || card.requires.is_some_and(|required| {
                                    !self.cards.iter().any(|card| {
                                        card.resource == required && card.status == Status::Granted
                                    })
                                });
                            GroupBox::new()
                                .fill()
                                .content_style(StyleRefinement::default().p_4().gap_2())
                                .child(
                                    h_flex()
                                        .gap_3()
                                        .justify_between()
                                        .child(
                                            v_flex()
                                                .gap_1()
                                                .flex_1()
                                                .min_w_0()
                                                .child(
                                                    div().font_medium().child(tr(resource.key())),
                                                )
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .debug_selector(move || {
                                                            format!(
                                                                "{}-{}",
                                                                resource.key(),
                                                                status.key()
                                                            )
                                                        })
                                                        .child(tr(card.status.key())),
                                                ),
                                        )
                                        .when(card.check.is_some(), |row| {
                                            row.child(
                                                Button::new(format!("{}-check", resource.key()))
                                                    .ghost()
                                                    .small()
                                                    .label(tr("permission_check"))
                                                    .disabled(
                                                        self.checking || self.pending.is_some(),
                                                    )
                                                    .debug_selector(move || {
                                                        format!("{}-check", resource.key())
                                                    })
                                                    .on_click(cx.listener(
                                                        |flow, _, window, cx| {
                                                            flow.check(window, cx)
                                                        },
                                                    )),
                                            )
                                        })
                                        .when(
                                            resource == Resource::Chrome
                                                && status == Status::Denied
                                                && card.settings.is_some(),
                                            |row| {
                                                let url = card.settings.unwrap();
                                                row.child(
                                                    Button::new("chrome-access-settings")
                                                        .ghost()
                                                        .small()
                                                        .label(tr("permission_settings"))
                                                        .on_click(move |_, _, cx| cx.open_url(url)),
                                                )
                                            },
                                        )
                                        .child(
                                            Button::new(resource.key())
                                                .small()
                                                .label(tr(
                                                    if matches!(
                                                        card.status,
                                                        Status::Denied | Status::Restricted
                                                    ) && card.settings.is_some()
                                                        && resource != Resource::Chrome
                                                    {
                                                        "permission_settings"
                                                    } else {
                                                        "permission_request"
                                                    },
                                                ))
                                                .when(pending, |button| {
                                                    button.icon(
                                                        gpui_kit::component::spinner::Spinner::new(
                                                        ),
                                                    )
                                                })
                                                .loading(pending)
                                                .disabled(disabled)
                                                .debug_selector(move || {
                                                    format!("{}-request", resource.key())
                                                })
                                                .on_click(cx.listener(
                                                    move |flow, _, window, cx| {
                                                        flow.request(resource, window, cx)
                                                    },
                                                )),
                                        ),
                                )
                        }),
                ),
            )
            .child(
                DialogFooter::new().px_6().pb_6().child(
                    Button::new("permissions-cancel")
                        .label(tr("settings_cancel"))
                        .debug_selector(|| "permissions-cancel".into())
                        .on_click(cx.listener(|flow, _, window, cx| {
                            flow.finish(false, window, cx);
                            window.close_dialog(cx);
                        })),
                ),
            )
    }
}
