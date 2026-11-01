//! Core Settings › Teams: teams built from the shared role catalog, and the catalog itself.
//! The Default team is virtual: it always contains every role and follows the conversation
//! model, so the page draws it and never stores it.
mod draft;
mod editor;
mod form;
mod list;
mod menu;
mod models;
mod requests;
mod roles;

use super::{Workspace, providers::Binding};
use crate::tr;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    tab::{Tab, TabBar},
    *,
};
use gpui_kit::*;
use sailry_protocol::{role, team};

const TEAMS: usize = 0;

pub(super) struct State {
    tab: usize,
    /// Native row menus dispatch their actions through this focus.
    focus: FocusHandle,
}

impl State {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            tab: TEAMS,
            focus: cx.focus_handle(),
        }
    }
}

/// Node catalog values captured when an editor opens; saves still send exact revisions.
#[derive(Clone)]
struct Catalog {
    binding: Binding,
    roles: Vec<role::Profile>,
    teams: Vec<team::Profile>,
    models: models::Models,
}

impl Workspace {
    /// Editing requires a connected Node; preview and offline pages stay read-only.
    fn team_catalog(&self) -> Option<Catalog> {
        let live = self.provider_link.as_ref().filter(|live| live.connected)?;
        let snapshot = live.snapshot.as_ref()?;
        Some(Catalog {
            binding: live.binding.clone(),
            roles: snapshot.roles.clone(),
            teams: snapshot.teams.clone(),
            models: models::Models::new(
                self.providers.channels.clone(),
                live.providers
                    .iter()
                    .map(|(&key, provider)| (key, provider.id)),
            ),
        })
    }

    fn open_team(&mut self, target: editor::Target, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(catalog) = self.team_catalog() {
            editor::open(catalog, target, window, cx);
        }
    }

    pub(super) fn team_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let catalog = self.team_catalog();
        let locked = catalog.is_none();
        let teams = self.teams.tab == TEAMS;
        let tabs = TabBar::new("team-tabs")
            .segmented()
            .menu(false)
            .selected_index(self.teams.tab)
            .child(Tab::new().label(tr("settings_teams")))
            .child(Tab::new().label(tr("teams_roles")))
            .on_click(cx.listener(|workspace, index: &usize, _, cx| {
                workspace.teams.tab = *index;
                cx.notify();
            }));
        let create = if teams {
            Button::new("team-new")
                .label(tr("team_new"))
                .on_click(cx.listener(|workspace, _, window, cx| {
                    workspace.open_team(editor::Target::New, window, cx)
                }))
        } else {
            Button::new("role-new").label(tr("role_new")).on_click(
                cx.listener(|workspace, _, window, cx| workspace.open_role(None, window, cx)),
            )
        };
        let body = if teams {
            self.team_rows(catalog.as_ref(), cx)
        } else {
            self.role_rows(catalog.as_ref(), cx)
        };
        v_flex()
            .track_focus(&self.teams.focus)
            .on_action(cx.listener(Self::team_action))
            .w_full()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_3()
                    .child(tabs)
                    .child(div().flex_1())
                    .child(
                        create
                            .primary()
                            .small()
                            .icon(IconName::Plus)
                            .disabled(locked),
                    ),
            )
            .child(body)
            // Child concurrency: stage 2 moves the delegation plugin's per-conversation
            // subagent limit into a core Node setting. Its control belongs here, at the
            // bottom of the page below both tabs, once that setting exists.
            .into_any_element()
    }
}
