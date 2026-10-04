use super::*;
use crate::conversation::mode;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::DropdownMenu,
};
use gpui_kit::prelude::FluentBuilder as _;

#[cfg(test)]
mod tests;

impl View {
    pub(super) fn select_mode(
        &mut self,
        mode: sailry_protocol::WorkMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly() || self.busy() || (self.session.is_some() && !self.connected()) {
            return;
        }
        self.replace_mode_marker(mode, window, cx);
        if self.session.is_none() {
            self.draft_mode = Some(mode);
        }
        if let Some(mut config) = self.config.clone() {
            config.mode = mode;
            self.configure(config, window, cx);
        } else {
            self.remember_options(cx);
        }
        cx.notify();
    }

    pub(super) fn confirm_mode_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        let text = self.input.read(cx).value();
        let Some((mode, range)) = references::commands::leading_mode(&text) else {
            return;
        };
        if range.end == text.len() || self.current_mode() == mode {
            return;
        }
        self.select_mode(mode, window, cx);
    }

    pub(super) fn current_mode(&self) -> sailry_protocol::WorkMode {
        self.config
            .as_ref()
            .map(|config| config.mode)
            .or(self.draft_mode)
            .unwrap_or(sailry_protocol::WorkMode::Code)
    }

    pub(super) fn replace_mode_marker(
        &mut self,
        mode: sailry_protocol::WorkMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.input.read(cx).value();
        let Some((current, range)) = references::commands::leading_mode(&text) else {
            return;
        };
        if current == mode {
            return;
        }
        self.input.update(cx, |input, cx| {
            let selected = input.selected_range();
            let length = references::commands::mode_token(mode).len();
            let end = range.end;
            let start = range.start;
            let shift = |offset: usize| {
                if offset >= end {
                    offset - (end - start) + length
                } else if offset > start {
                    start + length
                } else {
                    offset
                }
            };
            input
                .replace_range_with_token(
                    range,
                    references::commands::token(references::commands::mode_token(mode)),
                    window,
                    cx,
                )
                .expect("leading mode is a valid input range");
            input.set_selected_range(shift(selected.start)..shift(selected.end), cx);
        });
    }

    pub(super) fn mode_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self
            .config
            .as_ref()
            .map(|config| config.mode)
            .or(self.draft_mode);
        let current = selected.unwrap_or(sailry_protocol::WorkMode::Code);
        let owner = cx.entity().downgrade();
        let button = Button::new("live-mode")
            .custom(crate::theme::subtle_button(cx))
            .icon(mode::icon(current))
            .rounded_full()
            .label(tr(mode::label(current)))
            .dropdown_caret(false)
            .debug_selector(|| "live-chat-mode".into())
            .when(self.saving_config() && self.connected(), |button| {
                button.text_color(cx.theme().foreground)
            })
            .disabled(self.busy() || (self.session.is_some() && !self.connected()));
        button
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                mode::MODES
                    .into_iter()
                    .fold(menu.check_side(Side::Right), |menu, mode| {
                        let owner = owner.clone();
                        menu.item(mode::item(mode, current == mode).on_click(
                            move |_, window, cx| {
                                let _ = owner.update(cx, |view, cx| {
                                    view.select_mode(mode, window, cx);
                                });
                            },
                        ))
                    })
            })
            .into_any_element()
    }
}
