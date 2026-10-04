use crate::theme::DialogStyle as _;
use crate::{
    shell::Shell,
    tr,
    workspace::model::updates::{Phase, State},
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    progress::Progress,
    scroll::ScrollableElement,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

struct Update {
    host: usize,
    shell: WeakEntity<Shell>,
    _observer: Subscription,
}

impl Shell {
    pub(crate) fn host_update(&self, host: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.workspace.updates.contains_key(&host) {
            return;
        }
        let shell = cx.entity();
        let view = cx.new(|cx| Update {
            host,
            shell: shell.downgrade(),
            _observer: cx.observe(&shell, |_, _, cx| cx.notify()),
        });
        view.update(cx, |_, cx| {
            crate::feedback::observe_with(
                window,
                cx,
                |view: &Update, cx| {
                    view.shell
                        .read_with(cx, |shell, _| {
                            shell
                                .workspace
                                .updates
                                .get(&view.host)
                                .and_then(|state| state.error.as_ref())
                                .map(|_| "update_failed")
                        })
                        .ok()
                        .flatten()
                        .into_iter()
                        .collect()
                },
                |view, key, cx| {
                    let detail = view
                        .shell
                        .read_with(cx, |shell, _| {
                            shell
                                .workspace
                                .updates
                                .get(&view.host)
                                .and_then(|state| state.error.clone())
                        })
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| tr(key));
                    crate::feedback::diagnostic(detail).title(tr("host_update"))
                },
            );
        });
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .form_title(tr("host_update"))
                .w((window.viewport_size().width - px(48.)).min(px(480.)))
                .overlay_closable(false)
                .on_ok(|_, _, _| false)
                .child(view.clone())
        });
    }
}

fn status(state: &State, cx: &App) -> AnyElement {
    v_flex()
        .gap_3()
        .child(
            div()
                .font_semibold()
                .debug_selector(|| "host-update-headline".into())
                .child(tr(if state.phase == Phase::Idle && state.note.is_some() {
                    "update_unavailable"
                } else {
                    state.phase.label()
                })),
        )
        .child(
            div()
                .truncate()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .debug_selector(|| "host-update-versions".into())
                .child(
                    match state
                        .target
                        .as_ref()
                        .filter(|target| *target != &state.current)
                    {
                        Some(target) => rust_i18n::t!(
                            "update_versions",
                            current = &state.current,
                            target = target
                        )
                        .to_string(),
                        None => {
                            rust_i18n::t!("update_version", current = &state.current).to_string()
                        }
                    },
                ),
        )
        .when_some(
            state.note.as_ref().filter(|_| state.phase == Phase::Idle),
            |body, note| {
                body.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(note.clone()),
                )
            },
        )
        .when_some(state.progress, |body, value| {
            body.child(
                div()
                    .debug_selector(|| "host-update-progress".into())
                    .child(
                        Progress::new("host-update-progress")
                            .value(value.min(100) as f32)
                            .xsmall()
                            .accessibility_label(tr("host_update")),
                    ),
            )
        })
        .into_any_element()
}

impl Render for Update {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self
            .shell
            .upgrade()
            .and_then(|shell| shell.read(cx).workspace.updates.get(&self.host).cloned());
        let action = snapshot.as_ref().and_then(State::primary);
        v_flex()
            .gap_4()
            .debug_selector(|| "host-update-dialog".into())
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("update_preview")),
            )
            .child(
                div()
                    .max_h((window.viewport_size().height - px(300.)).clamp(px(80.), px(300.)))
                    .child(
                        v_flex()
                            .id("host-update-content")
                            .overflow_y_scrollbar()
                            .child(
                                snapshot
                                    .as_ref()
                                    .map(|value| status(value, cx))
                                    .unwrap_or_else(|| {
                                        div().child(tr("update_target_changed")).into_any_element()
                                    }),
                            ),
                    ),
            )
            .child(
                DialogFooter::new()
                    .w_full()
                    .mt_3()
                    .child(
                        Button::new("host-update-close")
                            .label(tr("close"))
                            .debug_selector(|| "host-update-close".into())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .when_some(action, |footer, (_, label)| {
                        footer.child(
                            Button::new("host-update-action")
                                .primary()
                                .label(tr(label))
                                .debug_selector(|| "host-update-action".into())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(expected) = &snapshot {
                                        _ = this.shell.update(cx, |shell, cx| {
                                            if let Some(current) =
                                                shell.workspace.updates.get_mut(&this.host)
                                            {
                                                current.advance(expected);
                                            }
                                            cx.notify();
                                        });
                                    }
                                })),
                        )
                    }),
            )
    }
}
