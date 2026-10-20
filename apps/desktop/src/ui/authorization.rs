//! One account authorization dialog body shared by device-code and browser sign-in flows.
//! Callers own the sign-in workflow and map its state into [`Flow`]; this module only renders
//! Kit controls, copies codes and links, and emits the caller's actions.
use crate::theme::DialogStyle as _;
use crate::tr;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::{component::*, prelude::FluentBuilder as _, *};
use std::rc::Rc;
use std::time::Duration;

#[cfg(test)]
mod tests;

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;
type LabeledHandler = (&'static str, Handler);

const WIDTH: f32 = 400.;
const CELL_WIDTH: f32 = 28.;
const CELL_HEIGHT: f32 = 38.;
const GAP_WIDTH: f32 = 14.;
/// Icon tile edge shared by every caller of this dialog.
pub(crate) const TILE: f32 = 48.;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    /// No code or link yet, or the provider is exchanging the result.
    Starting,
    Device {
        code: SharedString,
        url: SharedString,
        expires_at_ms: Option<u64>,
    },
    Browser {
        url: SharedString,
    },
    /// The code or link can no longer complete; refreshing restarts the flow.
    Expired {
        code: Option<SharedString>,
        url: Option<SharedString>,
    },
    /// The attempt stopped before completion; the outcome was already reported by toast.
    Failed {
        code: Option<SharedString>,
        url: Option<SharedString>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Cell {
    Char(char),
    Gap,
}

/// Splits a user code into display cells; separators become a narrow gap.
pub(crate) fn cells(code: &str) -> Vec<Cell> {
    code.chars()
        .filter(|ch| !ch.is_whitespace())
        .map(|ch| if ch == '-' { Cell::Gap } else { Cell::Char(ch) })
        .collect()
}

/// Host shown in the device-code hint, without a `www.` prefix.
pub(crate) fn host(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    Some(host.strip_prefix("www.").unwrap_or(host).to_owned())
}

/// Remaining time as `mm:ss` (or `h:mm:ss`); `None` once expired or when unknown.
pub(crate) fn countdown(expires_at_ms: u64, now_ms: u64) -> Option<String> {
    let seconds = expires_at_ms.checked_sub(now_ms)?.div_ceil(1000);
    if seconds == 0 {
        return None;
    }
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    Some(if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    })
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

/// Applies the shared surface, width and padding to the hosting Kit dialog.
pub(crate) fn dialog(dialog: dialog::Dialog, window: &Window, cx: &App) -> dialog::Dialog {
    dialog
        .floating_surface(cx)
        .w((window.viewport_size().width - px(48.)).min(px(WIDTH)))
        .pt(px(32.))
        .px(px(28.))
        .pb(px(24.))
}

/// Success feedback after the caller closes the dialog.
pub(crate) fn connected(title: &str, account: Option<&str>, window: &mut Window, cx: &mut App) {
    let detail = match account {
        Some(account) => format!("{title} · {account}"),
        None => title.to_owned(),
    };
    crate::feedback::toast(
        window,
        tr("authorization_connected"),
        notification::Notification::success(detail).title(tr("authorization_connected")),
        cx,
    );
}

fn copy(text: &str, window: &mut Window, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
    crate::feedback::toast(
        window,
        tr("content_copied"),
        notification::Notification::success(tr("content_copied")),
        cx,
    );
}

/// Re-renders the countdown once per second while it is displayed.
struct Clock {
    _task: Task<()>,
}

impl Clock {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            _task: cx.spawn(async move |clock, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if clock.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }),
        }
    }
}

#[derive(IntoElement)]
pub(crate) struct Authorization {
    icon: AnyElement,
    title: SharedString,
    flow: Flow,
    cancelling: bool,
    busy: bool,
    extra: Option<AnyElement>,
    on_refresh: Option<Handler>,
}

impl Authorization {
    pub(crate) fn new(icon: impl IntoElement, title: impl Into<SharedString>, flow: Flow) -> Self {
        Self {
            icon: icon.into_any_element(),
            title: title.into(),
            flow,
            cancelling: false,
            busy: false,
            extra: None,
            on_refresh: None,
        }
    }

    /// Shows cancellation progress on the cancel button.
    pub(crate) fn cancelling(mut self, cancelling: bool) -> Self {
        self.cancelling = cancelling;
        self
    }

    /// Disables refresh while a caller request is running.
    pub(crate) fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// Extra caller content between the status and the actions.
    pub(crate) fn extra(mut self, extra: impl IntoElement) -> Self {
        self.extra = Some(extra.into_any_element());
        self
    }

    /// Restarts the flow; without it, an expired or failed flow offers only Cancel.
    pub(crate) fn on_refresh(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_refresh = Some(Rc::new(handler));
        self
    }

    fn hint(&self) -> Option<SharedString> {
        match &self.flow {
            Flow::Device { url, .. }
            | Flow::Expired {
                code: Some(_),
                url: Some(url),
            }
            | Flow::Failed {
                code: Some(_),
                url: Some(url),
            } => host(url).map(|host| {
                rust_i18n::t!("authorization_enter_code", host = host)
                    .to_string()
                    .into()
            }),
            Flow::Browser { .. }
            | Flow::Expired {
                code: None,
                url: Some(_),
            }
            | Flow::Failed {
                code: None,
                url: Some(_),
            } => Some(tr("authorization_browser")),
            _ => None,
        }
    }

    fn code(&self) -> Option<(&SharedString, bool)> {
        match &self.flow {
            Flow::Device { code, .. } => Some((code, false)),
            Flow::Expired {
                code: Some(code), ..
            }
            | Flow::Failed {
                code: Some(code), ..
            } => Some((code, true)),
            _ => None,
        }
    }

    fn render_code(code: &SharedString, dim: bool, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let row = h_flex()
            .gap_1()
            .children(cells(code).into_iter().map(|cell| {
                match cell {
                    Cell::Gap => div()
                        .w(px(GAP_WIDTH))
                        .flex()
                        .justify_center()
                        .text_lg()
                        .text_color(theme.muted_foreground)
                        .child("–")
                        .into_any_element(),
                    Cell::Char(ch) => div()
                        .w(px(CELL_WIDTH))
                        .h(px(CELL_HEIGHT))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(theme.radius)
                        .bg(theme.muted)
                        .font_family(theme.mono_font_family.clone())
                        .text_lg()
                        .font_semibold()
                        .when(dim, |cell| {
                            cell.text_color(theme.muted_foreground).line_through()
                        })
                        .child(ch.to_string())
                        .into_any_element(),
                }
            }));
        if dim {
            return div()
                .debug_selector(|| "authorization-code".into())
                .p(px(6.))
                .child(row)
                .into_any_element();
        }
        let text = code.clone();
        Button::new("authorization-code")
            .debug_selector(|| "authorization-code".into())
            .accessibility_label(tr("authorization_copy_code"))
            .custom(ButtonCustomVariant::new(cx).hover(theme.secondary_hover))
            .h_auto()
            .p(px(6.))
            .rounded(theme.radius_lg)
            .child(row)
            .on_click(move |_, window, cx| copy(&text, window, cx))
            .into_any_element()
    }

    fn status(&self, window: &mut Window, cx: &mut App) -> Option<(SharedString, bool, bool)> {
        // (text, spinning, warning)
        Some(match &self.flow {
            Flow::Starting => (tr("authorization_starting"), true, false),
            Flow::Device { expires_at_ms, .. } => {
                let time = expires_at_ms.filter(|at| *at > 0).and_then(|at| {
                    window.use_keyed_state("authorization-clock", cx, |_, cx| Clock::new(cx));
                    countdown(at, now_ms())
                });
                let text = match time {
                    Some(time) => rust_i18n::t!("authorization_waiting_time", time = time)
                        .to_string()
                        .into(),
                    None => tr("authorization_waiting"),
                };
                (text, true, false)
            }
            Flow::Browser { .. } => (tr("authorization_waiting"), true, false),
            Flow::Expired { code, .. } => (
                tr(if code.is_some() {
                    "authorization_code_expired"
                } else {
                    "authorization_link_expired"
                }),
                false,
                true,
            ),
            Flow::Failed { .. } => return None,
        })
    }

    fn actions(&self) -> (Option<LabeledHandler>, Option<LabeledHandler>) {
        let open = |url: &SharedString| -> Handler {
            let url = url.clone();
            Rc::new(move |_: &mut Window, cx: &mut App| cx.open_url(&url))
        };
        match &self.flow {
            Flow::Device { url, .. } => (None, Some(("authorization_open", open(url)))),
            Flow::Browser { url } => {
                let link = url.clone();
                (
                    Some((
                        "authorization_copy_link",
                        Rc::new(move |window: &mut Window, cx: &mut App| copy(&link, window, cx))
                            as Handler,
                    )),
                    Some(("authorization_reopen", open(url))),
                )
            }
            Flow::Expired { .. } => (
                None,
                self.on_refresh
                    .clone()
                    .map(|handler| ("authorization_refresh", handler)),
            ),
            Flow::Failed { .. } => (
                None,
                self.on_refresh
                    .clone()
                    .map(|handler| ("authorization_retry", handler)),
            ),
            Flow::Starting => (None, None),
        }
    }
}

impl RenderOnce for Authorization {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let hint = self.hint();
        let code = self
            .code()
            .map(|(code, dim)| Self::render_code(code, dim, cx));
        let status = self.status(window, cx);
        let (secondary, primary) = self.actions();
        let theme = cx.theme();
        let busy = self.busy;
        let cancelling = self.cancelling;
        // Browser flows replace the cancel button with Copy link; dismissal stays on the close button.
        let secondary = secondary.map_or_else(
            || {
                Button::new("authorization-cancel")
                    .debug_selector(|| "authorization-cancel".into())
                    .flex_1()
                    .loading(cancelling)
                    .disabled(cancelling)
                    .label(tr("authorization_cancel"))
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(dialog::Cancel), cx);
                    })
            },
            |(key, handler)| {
                Button::new("authorization-secondary")
                    .debug_selector(|| "authorization-secondary".into())
                    .flex_1()
                    .label(tr(key))
                    .on_click(move |_, window, cx| handler(window, cx))
            },
        );
        v_flex()
            .id("authorization")
            .debug_selector(|| "authorization".into())
            .w_full()
            .items_center()
            .child(self.icon)
            .child(
                div()
                    .mt(px(14.))
                    .max_w_full()
                    .min_w_0()
                    .truncate()
                    .text_base()
                    .font_semibold()
                    .child(self.title),
            )
            .when_some(hint, |body, hint| {
                body.child(
                    div()
                        .mt_1()
                        .max_w_full()
                        .text_center()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(hint),
                )
            })
            .when_some(code, |body, code| body.child(div().mt(px(22.)).child(code)))
            .when_some(status, |body, (text, spinning, warning)| {
                body.child(
                    h_flex()
                        .mt_4()
                        .h(px(18.))
                        .gap_1p5()
                        .items_center()
                        .text_xs()
                        .text_color(if warning {
                            theme.warning
                        } else {
                            theme.muted_foreground
                        })
                        .when(spinning, |row| row.child(spinner::Spinner::new().small()))
                        .child(text),
                )
            })
            .when_some(self.extra.take(), |body, extra| {
                body.child(div().mt_4().w_full().child(extra))
            })
            .child(
                h_flex()
                    .mt(px(22.))
                    .w_full()
                    .gap_2()
                    .child(secondary)
                    .when_some(primary, |row, (key, handler)| {
                        row.child(
                            Button::new("authorization-primary")
                                .debug_selector(|| "authorization-primary".into())
                                .primary()
                                .flex_1()
                                .disabled(busy || cancelling)
                                .label(tr(key))
                                .on_click(move |_, window, cx| handler(window, cx)),
                        )
                    }),
            )
    }
}
