use super::*;
use crate::{
    settings::{
        entry::Entry as RowEntry,
        group::{Group, Row},
    },
    theme::DialogStyle as _,
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    scroll::ScrollableElement,
    switch::Switch,
};
use gpui_kit::prelude::FluentBuilder as _;

pub(super) fn key(phase: Phase) -> &'static str {
    match phase {
        Phase::BeforeTool => "hooks_before_tool",
        Phase::AfterTool => "hooks_after_tool",
        Phase::TurnEnd => "hooks_turn_end",
    }
}

impl Panel {
    fn consent(&mut self, settings: Settings, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let baseline = self.current().cloned();
        let root = self
            .root
            .clone()
            .unwrap_or_else(|| self.worktree.to_string());
        let worktree = self.worktree;
        let node = self.binding.label.clone();
        let sandboxed = self.sandboxed;
        let owner = cx.entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let submit = owner.clone();
            let captured = settings.clone();
            let baseline = baseline.clone();
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
                .children(
                    settings
                        .entries()
                        .filter(|(_, _, script)| script.enabled)
                        .map(|(id, phase, script)| {
                            let id = id.to_owned();
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
                                        .debug_selector(move || format!("hooks-consent-{id}"))
                                        .child(script.command.clone()),
                                )
                                .child(div().text_sm().child(format!(
                                    "{}: {}",
                                    tr("hooks_timeout"),
                                    script.timeout_ms
                                )))
                        }),
                );
            dialog
                .form_title(tr("hooks_enable"))
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
                            Button::new("hooks-consent-enable")
                                .debug_selector(|| "hooks-consent-enable".into())
                                .primary()
                                .label(tr("hooks_enable"))
                                .on_click(move |_, window, cx| {
                                    submit.update(cx, |panel, cx| {
                                        if !panel.active || panel.current() != baseline.as_ref() {
                                            panel.error = Some("hooks_target_changed");
                                            cx.notify();
                                        } else {
                                            panel.submit(captured.clone(), window, cx);
                                        }
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
        let settings = self.current().cloned().unwrap_or_default();
        let locked = self.locked();
        let owner = cx.entity();
        let mut list = Group::new("hooks_scripts")
            .empty(IconName::Settings2, "hooks_empty")
            .action(
                Button::new("hooks-add")
                    .debug_selector(|| "hooks-add".into())
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .tooltip(tr("hooks_add"))
                    .accessibility_label(tr("hooks_add"))
                    .disabled(locked)
                    .on_click(move |_, window, cx| {
                        editor::open(owner.clone(), None, window, cx);
                    }),
            );
        for (id, phase, script) in settings.entries() {
            let id = id.to_owned();
            let toggle_id = id.clone();
            let edit_id = id.clone();
            let remove_id = id.clone();
            let owner = cx.entity();
            list = list.child(
                RowEntry::new(
                    format!("hooks-script-{id}"),
                    v_flex()
                        .gap_1()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .font_medium()
                                .child(if script.name.is_empty() {
                                    tr(key(phase))
                                } else {
                                    script.name.clone().into()
                                }),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_color(cx.theme().muted_foreground)
                                .child(if script.name.is_empty() {
                                    script.command.clone().into()
                                } else {
                                    tr(key(phase))
                                }),
                        ),
                )
                .control(
                    div()
                        .debug_selector({
                            let id = id.clone();
                            move || format!("hooks-enabled-{id}")
                        })
                        .child(
                            Switch::new(SharedString::from(format!("hooks-enabled-{id}")))
                                .checked(script.enabled)
                                .disabled(locked)
                                .accessibility_label(tr("hooks_enabled"))
                                .on_click(cx.listener(move |panel, value, window, cx| {
                                    let Some(mut settings) = panel.current().cloned() else {
                                        return;
                                    };
                                    let Some((phase, mut script)) = panel.entry(&toggle_id) else {
                                        return;
                                    };
                                    script.enabled = *value;
                                    Self::replace(&mut settings, &toggle_id, phase, Some(script));
                                    if *value {
                                        panel.consent(settings, window, cx);
                                    } else {
                                        panel.submit(settings, window, cx);
                                    }
                                })),
                        ),
                )
                .action(
                    Button::new(SharedString::from(format!("hooks-edit-{id}")))
                        .debug_selector({
                            let id = id.clone();
                            move || format!("hooks-edit-{id}")
                        })
                        .icon(IconName::Settings2)
                        .tooltip(tr("hooks_configure"))
                        .accessibility_label(tr("hooks_configure"))
                        .disabled(locked)
                        .on_click(move |_, window, cx| {
                            editor::open(owner.clone(), Some(edit_id.clone()), window, cx);
                        }),
                )
                .action(
                    Button::new(SharedString::from(format!("hooks-remove-{id}")))
                        .debug_selector(move || format!("hooks-remove-{id}"))
                        .icon(IconName::CircleX)
                        .tooltip(tr("hooks_remove"))
                        .accessibility_label(tr("hooks_remove"))
                        .disabled(locked)
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            let Some(mut settings) = panel.current().cloned() else {
                                return;
                            };
                            Self::replace(&mut settings, &remove_id, phase, None);
                            if settings.entries().next().is_none() {
                                settings.enabled = false;
                            }
                            panel.submit(settings, window, cx);
                        })),
                ),
            );
        }
        let toggle = div().debug_selector(|| "hooks-enabled".into()).child(
            Switch::new("hooks-enabled")
                .checked(settings.enabled)
                .disabled(locked || settings.entries().next().is_none())
                .accessibility_label(tr("hooks_enabled"))
                .on_click(cx.listener(|panel, value, window, cx| {
                    let Some(mut settings) = panel.current().cloned() else {
                        return;
                    };
                    settings.enabled = *value;
                    if *value && settings.entries().any(|(_, _, script)| script.enabled) {
                        panel.consent(settings, window, cx);
                    } else {
                        panel.submit(settings, window, cx);
                    }
                })),
        );
        v_flex()
            .gap_6()
            .child(Group::new("hooks_settings").child(Row::new("hooks_enabled", toggle)))
            .child(list)
            .when(self.loading, |view| view.child(spinner::Spinner::new()))
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
