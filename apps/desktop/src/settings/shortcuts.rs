use super::group::{Group, Row};
use crate::{shortcuts, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    kbd::Kbd,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

// Kit has no shortcut recorder. Reuse its buttons and focus dispatch, with only
// the application binding capture and validation kept here.
pub(super) struct Panel {
    focus: FocusHandle,
    active: Option<&'static str>,
    draft: String,
    error: Option<&'static str>,
}

impl Panel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            active: None,
            draft: String::new(),
            error: None,
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.active = None;
        self.draft.clear();
        self.error = None;
        cx.notify();
    }

    fn record(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.active = Some(id);
        self.draft.clear();
        self.error = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn capture(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.active.is_none() {
            return;
        }
        cx.stop_propagation();
        window.prevent_default();
        if event.keystroke.key == "escape" {
            self.cancel(cx);
            return;
        }
        if event.is_held {
            return;
        }
        self.draft = event.keystroke.unparse();
        self.error = None;
        self.save(window, cx);
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.active else { return };
        if self.draft.is_empty() {
            return;
        }
        match shortcuts::save(id, Some(&self.draft), cx) {
            Ok(()) => self.cancel(cx),
            Err(error) => {
                self.error = Some(error);
                crate::feedback::error("", &tr(error), window, cx);
                cx.notify();
            }
        }
    }

    fn control(&self, entry: &shortcuts::Shortcut, cx: &mut Context<Self>) -> AnyElement {
        let id = entry.id;
        let recording = self.active == Some(id);
        let key = if recording { &self.draft } else { &entry.key };
        let label = if key.is_empty() {
            if recording {
                tr("shortcut_record").to_string()
            } else {
                tr("shortcut_disabled").to_string()
            }
        } else {
            shortcuts::label(key)
        };
        let strokes = key
            .split_whitespace()
            .filter_map(|key| Keystroke::parse(key).ok())
            .collect::<Vec<_>>();
        v_flex()
            .gap_1()
            .items_end()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(format!("shortcut-{id}"))
                            .accessibility_label(label.clone())
                            .child(
                                h_flex()
                                    .debug_selector(move || format!("shortcut-label-{id}"))
                                    .gap_1()
                                    .text_size(px(12.))
                                    .line_height(relative(1.))
                                    .when(strokes.is_empty(), |row| row.child(label))
                                    .children(strokes.into_iter().map(Kbd::new)),
                            )
                            .debug_selector(move || format!("shortcut-{id}"))
                            .when(recording, |button| button.primary())
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel.record(id, window, cx)
                            })),
                    )
                    .when(recording, |row| {
                        row.child(
                            Button::new(format!("shortcut-reset-{id}"))
                                .debug_selector(move || format!("shortcut-reset-{id}"))
                                .ghost()
                                .icon(IconName::RotateCw)
                                .tooltip(tr("shortcut_reset"))
                                .accessibility_label(tr("shortcut_reset"))
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    if let Err(error) = shortcuts::save(id, None, cx) {
                                        crate::feedback::error("", &tr(error), window, cx);
                                    }
                                    panel.cancel(cx);
                                })),
                        )
                        .child(
                            Button::new(format!("shortcut-clear-{id}"))
                                .debug_selector(move || format!("shortcut-clear-{id}"))
                                .ghost()
                                .icon(IconName::Close)
                                .tooltip(tr("shortcut_clear"))
                                .accessibility_label(tr("shortcut_clear"))
                                .disabled(entry.key.is_empty())
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    if let Err(error) = shortcuts::save(id, Some(""), cx) {
                                        crate::feedback::error("", &tr(error), window, cx);
                                    }
                                    panel.cancel(cx);
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = shortcuts::entries(cx);
        let rows = entries
            .iter()
            .map(|entry| Row::new(entry.label, self.control(entry, cx)))
            .collect::<Vec<_>>();
        v_flex()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::capture))
            .text_sm()
            .gap_6()
            .child(Group::new("settings_global_commands").children(rows))
    }
}

#[cfg(test)]
mod tests;
