use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Input, InputState},
};
use sailry_protocol::{ErrorCode, Request, Session};

pub(super) fn open(client: Arc<Client>, session: &Session, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(crate::activity::title(session))
            .placeholder(tr("form_name_hint"))
    });
    let form = cx.new(|_| Rename {
        client,
        session: session.clone(),
        input: input.clone(),
        request: None,
        pending: false,
        closed: false,
        error: None,
    });
    form.update(cx, |_, cx| {
        crate::feedback::observe(window, cx, |form, _| form.error.into_iter().collect())
    });
    window.open_dialog(cx, move |dialog, window, _| {
        let submit = form.clone();
        let close = form.clone();
        dialog
            .form_title(tr("workspace_rename"))
            .w(px(420.).min(window.viewport_size().width - px(48.)))
            .overlay_closable(false)
            .on_ok(move |_, window, cx| {
                submit.update(cx, |form, cx| form.submit(window, cx));
                false
            })
            .on_close(move |_, _, cx| close.update(cx, |form, _| form.closed = true))
            .child(form.clone())
    });
    window.defer(cx, move |window, cx| {
        input.read(cx).focus_handle(cx).focus(window, cx)
    });
}
struct Rename {
    client: Arc<Client>,
    session: Session,
    input: Entity<InputState>,
    request: Option<Request>,
    pending: bool,
    closed: bool,
    error: Option<&'static str>,
}
impl Rename {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        let title = self.input.read(cx).value().trim().to_owned();
        if title.is_empty() || title.chars().any(char::is_control) {
            self.error = Some("session_title_required");
            cx.notify();
            return;
        }
        let request = self
            .request
            .get_or_insert_with(|| {
                self.client
                    .prepare(sailry_protocol::Command::RenameSession {
                        session: self.session.id,
                        expected_revision: self.session.revision,
                        title,
                    })
            })
            .clone();
        let client = self.client.clone();
        let job = cx
            .global::<Services>()
            .runtime
            .spawn(async move { client.execute(request).await });
        self.pending = true;
        self.error = None;
        cx.spawn_in(window, async move |form, cx| {
            let result = job.await;
            let _ = form.update_in(cx, |form, window, cx| {
                form.pending = false;
                if form.closed {
                    return;
                }
                match result {
                    Ok(Ok(Output::Session(_))) => {
                        form.closed = true;
                        window.close_dialog(cx);
                    }
                    Ok(Err(error)) => {
                        form.error = Some(match error.code {
                            ErrorCode::RevisionConflict | ErrorCode::NotFound => "session_changed",
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable => {
                                "project_outcome_unknown"
                            }
                            _ => "live_request_failed",
                        });
                        if !matches!(
                            error.code,
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable
                        ) {
                            form.request = None;
                        }
                    }
                    _ => form.error = Some("project_outcome_unknown"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
impl Render for Rename {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                div().debug_selector(|| "session-name-input".into()).child(
                    Input::new(&self.input)
                        .aria_label(tr("workspace_name"))
                        .disabled(self.pending || self.request.is_some()),
                ),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("session-rename-cancel")
                            .label(tr("settings_cancel"))
                            .disabled(self.pending)
                            .on_click(cx.listener(|form, _, window, cx| {
                                form.closed = true;
                                window.close_dialog(cx);
                            })),
                    )
                    .child(
                        Button::new("session-rename-save")
                            .debug_selector(|| "session-rename-save".into())
                            .primary()
                            .label(tr(if self.request.is_some() {
                                "chat_retry"
                            } else {
                                "settings_save"
                            }))
                            .loading(self.pending)
                            .on_click(cx.listener(|form, _, window, cx| form.submit(window, cx))),
                    ),
            )
    }
}
