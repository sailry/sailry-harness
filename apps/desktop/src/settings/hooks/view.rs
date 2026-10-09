use super::*;
use crate::{
    settings::group::{Group, Row},
    theme::DialogStyle as _,
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    input::Input,
    scroll::ScrollableElement,
    switch::Switch,
};
use gpui_kit::prelude::FluentBuilder;

fn key(phase: Phase) -> &'static str {
    match phase {
        Phase::BeforeTool => "hooks_before_tool",
        Phase::AfterTool => "hooks_after_tool",
        Phase::TurnEnd => "hooks_turn_end",
    }
}

impl Panel {
    fn consent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let Ok(settings) = self.draft(cx) else {
            self.error = Some("hooks_invalid");
            cx.notify();
            return;
        };
        let worktree = self.worktree;
        let root = self.root.clone().unwrap_or_else(|| worktree.to_string());
        let node = self.binding.label.clone();
        let sandboxed = self.sandboxed;
        let owner = cx.entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let submit = owner.clone();
            let settings = settings.clone();
            let captured = settings.clone();
            let details = v_flex()
                .id("hooks-consent-details")
                .gap_3()
                .max_h((window.viewport_size().height - px(250.)).max(px(160.)))
                .overflow_y_scrollbar()
                .child(div().child(tr("hooks_consent")))
                .child(div().child(tr(if sandboxed {
                    "hooks_scope_workspace"
                } else {
                    "hooks_scope_windows"
                })))
                .child(
                    div()
                        .debug_selector(|| "hooks-consent-scope".into())
                        .child(format!("{node} · {root} · {worktree}")),
                )
                .child(div().child(tr(if settings.enabled {
                    "hooks_enabled"
                } else {
                    "hooks_disabled"
                })))
                .children(
                    [Phase::BeforeTool, Phase::AfterTool, Phase::TurnEnd]
                        .into_iter()
                        .filter_map(|phase| {
                            let script = match phase {
                                Phase::BeforeTool => settings.before_tool.as_ref(),
                                Phase::AfterTool => settings.after_tool.as_ref(),
                                Phase::TurnEnd => settings.turn_end.as_ref(),
                            }?;
                            Some(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(tr(key(phase))),
                                    )
                                    .child(
                                        div()
                                            .whitespace_normal()
                                            .debug_selector(move || {
                                                format!("hooks-consent-{}", phase.name())
                                            })
                                            .child(script.command.clone()),
                                    )
                                    .child(div().text_sm().child(format!(
                                        "{}: {}",
                                        tr("hooks_timeout"),
                                        script.timeout_ms
                                    ))),
                            )
                        }),
                );
            dialog
                .form_title(tr("hooks_save"))
                .w((window.viewport_size().width - px(48.)).min(px(600.)))
                .overlay_closable(false)
                .child(details)
                .footer(
                    DialogFooter::new()
                        .w_full()
                        .gap_2()
                        .child(
                            Button::new("hooks-consent-cancel")
                                .debug_selector(|| "hooks-consent-cancel".into())
                                .label(tr("settings_cancel"))
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("hooks-consent-save")
                                .debug_selector(|| "hooks-consent-save".into())
                                .primary()
                                .label(tr("settings_save"))
                                .on_click(move |_, window, cx| {
                                    submit.update(cx, |panel, cx| {
                                        panel.submit(captured.clone(), window, cx)
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.locked();
        let mut group = Group::new("hooks_settings").child(Row::new(
            "hooks_enabled",
            div().debug_selector(|| "hooks-enabled".into()).child(
                Switch::new("hooks-enabled")
                    .checked(self.enabled)
                    .disabled(locked)
                    .accessibility_label(tr("hooks_enabled"))
                    .on_click(cx.listener(|panel, value, _, cx| {
                        panel.enabled = *value;
                        cx.notify();
                    })),
            ),
        ));
        for (index, (phase, command, timeout)) in self.fields.iter().enumerate() {
            group = group.child(
                v_flex()
                    .w_full()
                    .py_3()
                    .gap_2()
                    .child(div().child(tr(key(*phase))))
                    .child(
                        div()
                            .debug_selector(move || format!("hooks-command-{index}"))
                            .child(
                                Input::new(command)
                                    .disabled(locked)
                                    .aria_label(tr(key(*phase))),
                            ),
                    )
                    .child(Row::new(
                        "hooks_timeout",
                        div()
                            .w(px(120.))
                            .debug_selector(move || format!("hooks-timeout-{index}"))
                            .child(
                                Input::new(timeout)
                                    .disabled(locked)
                                    .aria_label(tr("hooks_timeout")),
                            ),
                    )),
            );
        }
        group = group
            .action(
                Button::new("hooks-save")
                    .debug_selector(|| "hooks-save".into())
                    .label(tr("settings_save"))
                    .primary()
                    .disabled(locked)
                    .on_click(cx.listener(|panel, _, window, cx| panel.consent(window, cx))),
            )
            .action(
                Button::new("hooks-reload")
                    .debug_selector(|| "hooks-reload".into())
                    .label(tr("hooks_reload"))
                    .disabled(self.pending || self.request.is_some())
                    .on_click(cx.listener(|panel, _, window, cx| panel.load(window, cx))),
            );
        v_flex()
            .gap_4()
            .child(group)
            .when(self.loading, |view| {
                view.child(gpui_kit::component::spinner::Spinner::new())
            })
            .when(self.request.is_some() && !self.pending, |view| {
                view.child(
                    Button::new("hooks-retry")
                        .debug_selector(|| "hooks-retry".into())
                        .label(tr("hooks_retry"))
                        .on_click(cx.listener(|panel, _, window, cx| panel.retry(window, cx))),
                )
            })
    }
}
