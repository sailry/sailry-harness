use crate::tr;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        menu::{PopupMenuItem, PopupMenuLayout},
        *,
    },
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
    let label = label(mode);
    let description = description(mode);
    PopupMenuItem::element(move |_, cx| {
        let layout = cx.global::<PopupMenuLayout>();
        h_flex()
            .id(label)
            .aria_label(format!("{}: {}", tr(label), tr(description)))
            .debug_selector(move || format!("{label}-option"))
            .flex_1()
            .max_w_96()
            .min_w_0()
            .whitespace_normal()
            .mx(-layout.row_padding.width)
            .my(-layout.row_padding.height)
            .px(layout.row_padding.width)
            .py(layout.row_padding.height)
            .rounded(layout.item_radius(cx.theme()))
            .aria_selected(selected)
            .when(selected, |row| row.bg(cx.theme().secondary_active))
            .gap_2()
            .child(Icon::new(icon(mode)).size_4())
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_sm().line_height(relative(1.25)).child(tr(label)))
                    .child(
                        div()
                            .text_xs()
                            .line_height(relative(1.25))
                            .text_color(cx.theme().muted_foreground)
                            .child(tr(description)),
                    ),
            )
    })
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
