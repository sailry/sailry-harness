use super::*;
use crate::conversation::permission;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuLayout},
};
use gpui_kit::prelude::FluentBuilder as _;

impl View {
    pub(super) fn select_permission(
        &mut self,
        mode: sailry_protocol::Permission,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy() || (self.session.is_some() && !self.connected()) {
            return;
        }
        if self.session.is_none() {
            self.draft_permission = Some(mode);
        }
        if let Some(mut config) = self.config.clone() {
            config.permission = mode;
            self.configure(config, window, cx);
        } else {
            self.remember_options(cx);
        }
        cx.notify();
    }

    pub(super) fn permission_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self
            .config
            .as_ref()
            .map(|config| config.permission)
            .or(self.draft_permission);
        let mode = selected.unwrap_or(sailry_protocol::Permission::Ask);
        let owner = cx.entity().downgrade();
        let modes = self.composer_options.permissions;
        let sidebar = self.sidebar;
        let label = if sidebar {
            sidebar_label(mode)
        } else {
            permission::label(mode)
        };
        let button = Button::new("live-permission")
            .custom(crate::theme::subtle_button(cx).foreground(match mode {
                sailry_protocol::Permission::Full => cx.theme().danger,
                sailry_protocol::Permission::Project => cx.theme().warning,
                _ => cx.theme().foreground,
            }))
            .icon(if sidebar {
                sidebar_icon(mode)
            } else {
                Icon::new(permission::icon(mode))
            })
            .rounded_full()
            .when(!sidebar, |button| button.label(tr(label)))
            .when(sidebar, |button| button.flex_shrink_0())
            .tooltip(tr(label))
            .accessibility_label(tr(label))
            .debug_selector(|| "live-chat-permission".into())
            .disabled(self.busy() || (self.session.is_some() && !self.connected()));
        button
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                modes
                    .iter()
                    .copied()
                    .fold(menu.check_side(Side::Right), |menu, mode| {
                        let owner = owner.clone();
                        menu.item(
                            (if sidebar {
                                sidebar_item(
                                    mode,
                                    selected.unwrap_or(sailry_protocol::Permission::Ask) == mode,
                                )
                            } else {
                                permission::item(
                                    mode,
                                    selected.unwrap_or(sailry_protocol::Permission::Ask) == mode,
                                )
                            })
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |view, cx| {
                                    view.select_permission(mode, window, cx);
                                });
                            }),
                        )
                    })
            })
            .into_any_element()
    }
}

fn sidebar_label(mode: sailry_protocol::Permission) -> &'static str {
    match mode {
        sailry_protocol::Permission::Full => "composer_permission_full",
        _ => "composer_permission_readonly",
    }
}

fn sidebar_item(
    mode: sailry_protocol::Permission,
    selected: bool,
) -> gpui_kit::component::menu::PopupMenuItem {
    let label = sidebar_label(mode);
    gpui_kit::component::menu::PopupMenuItem::element(move |_, cx| {
        let layout = cx.global::<PopupMenuLayout>();
        h_flex()
            .id(label)
            .debug_selector(move || format!("{label}-option"))
            .flex_1()
            .mx(-layout.row_padding.width)
            .my(-layout.row_padding.height)
            .px(layout.row_padding.width)
            .py(layout.row_padding.height)
            .rounded(layout.item_radius(cx.theme()))
            .aria_selected(selected)
            .when(selected, |row| row.bg(cx.theme().secondary_active))
            .gap_2()
            .child(sidebar_icon(mode).size_4())
            .child(tr(label))
    })
}

fn sidebar_icon(mode: sailry_protocol::Permission) -> Icon {
    Icon::default().path(match mode {
        sailry_protocol::Permission::Full => "icons/reicon/shield-off.svg",
        _ => "icons/reicon/shield.svg",
    })
}
