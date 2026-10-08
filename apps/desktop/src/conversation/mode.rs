use crate::tr;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        menu::{DropdownMenu, PopupMenuItem},
        *,
    },
    *,
};
use sailry_protocol::WorkMode;

pub(super) const MODES: [WorkMode; 2] = [WorkMode::Code, WorkMode::Plan];

pub(super) fn label(mode: WorkMode) -> &'static str {
    match mode {
        WorkMode::Code => "composer_mode_code",
        WorkMode::Plan => "composer_mode_plan",
    }
}

pub(super) fn icon(mode: WorkMode) -> IconName {
    match mode {
        WorkMode::Code => IconName::SquareTerminal,
        WorkMode::Plan => IconName::Map,
    }
}

pub(super) fn description(mode: WorkMode) -> &'static str {
    match mode {
        WorkMode::Code => "composer_mode_code_description",
        WorkMode::Plan => "composer_mode_plan_description",
    }
}

pub(super) fn item(mode: WorkMode, selected: bool) -> PopupMenuItem {
    super::choice::item(label(mode), description(mode), icon(mode), selected)
}

impl crate::shell::Shell {
    pub(super) fn composer_mode(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let key = (self.host, self.session);
        let selected = MODES[self.conversations[&key].options.choices[0]];
        let owner = cx.entity().downgrade();
        Button::new("composer-mode")
            .custom(crate::theme::subtle_button(cx))
            .icon(icon(selected))
            .rounded_full()
            .label(tr(label(selected)))
            .dropdown_caret(false)
            .debug_selector(|| "composer-mode".into())
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                MODES.into_iter().enumerate().fold(
                    menu.check_side(Side::Right),
                    |menu, (index, mode)| {
                        let owner = owner.clone();
                        menu.item(item(mode, selected == mode).on_click(move |_, _, cx| {
                            let _ = owner.update(cx, |shell, cx| {
                                if let Some(conversation) = shell.conversations.get_mut(&key) {
                                    conversation.options.choices[0] = index;
                                    cx.notify();
                                }
                            });
                        }))
                    },
                )
            })
    }
}
