use super::*;
use gpui_kit::component::{
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    native_menu::NativeMenu,
    notification::Notification,
};

#[derive(Clone, PartialEq, gpui_kit::Action)]
#[action(namespace = preview_cli, no_json)]
pub(crate) struct Start {
    pub(crate) owner: Owner,
    pub(crate) profile: &'static str,
}

impl Shell {
    pub(crate) fn start_preview_cli(
        &mut self,
        action: &Start,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.contains(action.owner) {
            return;
        }
        if !self.workspace.projects[&action.owner.project].trusted {
            self.project_trust(
                action.owner.project,
                Some(crate::workspace::Pending::CliProfile(
                    action.owner,
                    action.profile,
                )),
                window,
                cx,
            );
            return;
        }
        let Some(profile) = self
            .workspace
            .cli_profiles
            .get(&action.owner.host)
            .and_then(|profiles| profiles.iter().find(|profile| profile.id == action.profile))
            .cloned()
        else {
            return;
        };
        let path = self.workspace.worktrees[&action.owner.worktree]
            .path
            .clone();
        match self
            .workspace
            .create_cli_terminal(action.owner, &path, &profile)
        {
            Ok(key) => {
                self.sidebar.open_project(action.owner.project);
                self.select_terminal(key, window, cx);
            }
            Err(error) => {
                crate::feedback::toast(window, tr(error), Notification::error(tr(error)), cx)
            }
        }
    }

    pub(crate) fn preview_cli_button(&self, owner: Owner, button: Button) -> impl IntoElement {
        let profiles = self
            .workspace
            .cli_profiles
            .get(&owner.host)
            .cloned()
            .unwrap_or_default();
        button.dropdown_menu(move |menu, _, _| popup(menu, owner, &profiles))
    }

    pub(crate) fn preview_cli_menu(&self, owner: Owner) -> NativeMenu {
        self.workspace
            .cli_profiles
            .get(&owner.host)
            .into_iter()
            .flatten()
            .fold(NativeMenu::new(), |menu, profile| {
                menu.menu_with_icon_disabled(
                    profile.name.clone(),
                    IconName::SquareTerminal,
                    !profile.available,
                    Box::new(Start {
                        owner,
                        profile: profile.id,
                    }),
                )
            })
    }
}

fn popup(menu: PopupMenu, owner: Owner, profiles: &[Profile]) -> PopupMenu {
    profiles.iter().fold(menu, |menu, profile| {
        let action = Start {
            owner,
            profile: profile.id,
        };
        let id = profile.id;
        let label = profile.name.clone();
        menu.item(
            PopupMenuItem::element(move |_, _| {
                div()
                    .debug_selector(move || format!("cli-profile-{id}"))
                    .child(label.clone())
            })
            .icon(IconName::SquareTerminal)
            .disabled(!profile.available)
            .on_click(move |_, window, cx| window.dispatch_action(Box::new(action.clone()), cx)),
        )
    })
}
