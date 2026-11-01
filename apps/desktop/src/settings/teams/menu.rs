//! OS-native team row menus. Actions route to the Workspace that owns the Teams page and
//! act only while the same Node is bound.
use super::{
    editor::Target,
    requests::{self, Subject},
};
use crate::{settings::Workspace, tr};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::*;
use sailry_protocol::{Command, NodeId, TeamId, team};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Duplicate,
    Delete,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry_teams, no_json)]
pub(super) struct Dispatch {
    node: NodeId,
    /// None is the virtual Default team, which can be duplicated but not deleted.
    team: Option<TeamId>,
    operation: Operation,
}

pub(super) fn show(
    node: NodeId,
    team: Option<TeamId>,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    if !cfg!(any(target_os = "macos", target_os = "windows")) {
        crate::feedback::error(
            "workspace_native_menu_platform",
            "",
            &tr("workspace_native_menu_platform"),
            window,
            cx,
        );
        return;
    }
    let item = |operation| {
        Box::new(Dispatch {
            node,
            team,
            operation,
        })
    };
    let mut menu = NativeMenu::new().menu(tr("team_duplicate"), item(Operation::Duplicate));
    if team.is_some() {
        menu = menu.menu(tr("settings_delete"), item(Operation::Delete));
    }
    menu.show(position, window, cx);
}

/// Confirms, then removes the team at its displayed revision; roles stay in the catalog.
pub(super) fn delete(
    binding: super::Binding,
    team: team::Profile,
    window: &mut Window,
    cx: &mut App,
    after: impl FnOnce(&mut Window, &mut App) + 'static,
) {
    let prompt = rust_i18n::t!("team_delete_prompt", name = team.name.as_str()).to_string();
    crate::prompts::confirm(
        &tr("team_delete"),
        &prompt,
        tr("settings_delete"),
        window,
        cx,
        move |window, cx| {
            requests::remove(
                binding,
                Command::RemoveTeam {
                    team: team.id,
                    expected_revision: team.revision,
                },
                Subject::Team,
                "team_deleted",
                window,
                cx,
                after,
            )
        },
    );
}

impl Workspace {
    pub(super) fn team_action(
        &mut self,
        action: &Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(catalog) = self
            .team_catalog()
            .filter(|catalog| catalog.binding.client.target() == action.node)
        else {
            return;
        };
        let team = match action.team {
            Some(id) => match catalog.teams.iter().find(|team| team.id == id) {
                Some(team) => Some(team.clone()),
                None => return,
            },
            None => None,
        };
        match (action.operation, team) {
            (Operation::Duplicate, team) => self.open_team(Target::Copy(team), window, cx),
            (Operation::Delete, Some(team)) => {
                delete(catalog.binding, team, window, cx, |_, _| {});
            }
            (Operation::Delete, None) => {}
        }
    }
}
