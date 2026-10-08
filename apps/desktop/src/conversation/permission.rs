use crate::tr;
use gpui_kit::{
    component::{menu::PopupMenuItem, *},
    *,
};
use sailry_protocol::Permission;

pub(super) const MODES: [Permission; 3] = [Permission::Ask, Permission::Project, Permission::Full];

pub(super) fn label(mode: Permission) -> &'static str {
    match mode {
        Permission::Ask => "composer_permission_ask",
        Permission::Project => "composer_permission_project",
        Permission::Full => "composer_permission_full",
    }
}

pub(super) fn icon(mode: Permission) -> IconName {
    match mode {
        Permission::Ask => IconName::CircleUser,
        Permission::Project => IconName::CircleCheck,
        Permission::Full => IconName::TriangleAlert,
    }
}

pub(super) fn description(mode: Permission) -> &'static str {
    match mode {
        Permission::Ask => "composer_permission_ask_description",
        Permission::Project => "composer_permission_project_description",
        Permission::Full => "composer_permission_full_description",
    }
}

pub(super) fn item(mode: Permission, selected: bool) -> PopupMenuItem {
    super::choice::item(label(mode), description(mode), icon(mode), selected)
}

impl crate::shell::Shell {
    pub(super) fn composer_permission(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::{
            button::{Button, ButtonVariants},
            menu::DropdownMenu,
        };

        let key = (self.host, self.session);
        let selected = MODES[self.conversations[&key].options.choices[3]];
        let owner = cx.entity().downgrade();
        Button::new("composer-permission")
            .custom(crate::theme::subtle_button(cx))
            .icon(icon(selected))
            .rounded_full()
            .label(tr(label(selected)))
            .debug_selector(|| "composer-permission".into())
            .tooltip(tr("composer_permission_preview"))
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                MODES.into_iter().enumerate().fold(
                    menu.check_side(Side::Right),
                    |menu, (index, mode)| {
                        let owner = owner.clone();
                        menu.item(item(mode, selected == mode).on_click(move |_, _, cx| {
                            let _ = owner.update(cx, |shell, cx| {
                                if let Some(conversation) = shell.conversations.get_mut(&key) {
                                    conversation.options.choices[3] = index;
                                    cx.notify();
                                }
                            });
                        }))
                    },
                )
            })
    }
}
