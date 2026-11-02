//! Teams tab rows: the virtual Default team first, then stored teams in Node key order.
use super::{Catalog, editor::Target, menu};
use crate::{settings::Workspace, tr, workspace::appearance as identity};
use gpui_kit::component::{avatar::Avatar, list::ListItem, tag::Tag, *};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sailry_protocol::{projects::Appearance, role, team};

/// Kit Avatar tinted with the identity color.
pub(super) fn avatar(appearance: Option<&Appearance>, cx: &App) -> Avatar {
    let fallback = super::draft::fallback();
    filled(Some(appearance.unwrap_or(&fallback)), Icon::empty(), cx)
}

/// Leading row icon, sized and shaped like the model provider tiles.
pub(super) fn tile(appearance: Option<&Appearance>, fallback: Icon, cx: &App) -> Div {
    const SIZE: f32 = 36.;
    let (icon, fill) = identity::tinted(appearance, fallback, cx);
    h_flex()
        .size(px(SIZE))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(px(11.))
        .bg(fill)
        .child(icon.size(px(SIZE * 5. / 9.)))
}

/// A role's row tile; roles without an appearance use the standard agent glyph.
pub(super) fn role_tile(appearance: Option<&Appearance>, cx: &App) -> Div {
    let fallback = super::draft::fallback();
    tile(Some(appearance.unwrap_or(&fallback)), Icon::empty(), cx)
}

fn filled(appearance: Option<&Appearance>, fallback: Icon, cx: &App) -> Avatar {
    let (icon, fill) = identity::tinted(appearance, fallback, cx);
    Avatar::new().bg(fill).border_0().placeholder(icon)
}

pub(super) fn count(roles: usize) -> SharedString {
    if roles == 1 {
        tr("team_role_one")
    } else {
        rust_i18n::t!("team_role_count", count = roles)
            .to_string()
            .into()
    }
}

fn members<'a>(
    team: &[sailry_protocol::RoleId],
    roles: &'a [role::Profile],
) -> Vec<&'a role::Profile> {
    team.iter()
        .filter_map(|id| roles.iter().find(|role| role.id == *id))
        .collect()
}

impl Workspace {
    pub(super) fn team_rows(
        &self,
        catalog: Option<&Catalog>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut rows = vec![self.team_row(None, catalog, cx)];
        if let Some(catalog) = catalog {
            rows.extend(
                catalog
                    .teams
                    .iter()
                    .map(|team| self.team_row(Some(team), Some(catalog), cx)),
            );
        }
        crate::settings::group::Group::new("settings_teams")
            .heading(false)
            .children(rows)
            .into_any_element()
    }

    fn team_row(
        &self,
        team: Option<&team::Profile>,
        catalog: Option<&Catalog>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let roles = catalog.map_or(&[][..], |catalog| catalog.roles.as_slice());
        let selected = match team {
            Some(team) => members(&team.members, roles),
            None => roles.iter().collect(),
        };
        let (name, description) = match team {
            Some(team) => (
                SharedString::from(team.name.clone()),
                team.description.clone(),
            ),
            None => (
                tr("team_default"),
                tr("team_default_description").to_string(),
            ),
        };
        let mut meta = count(selected.len()).to_string();
        if let (Some(team), Some(catalog)) = (team, catalog) {
            meta.push_str(" · ");
            meta.push_str(
                &catalog
                    .models
                    .summary(team.model.as_ref(), "team_follow_conversation"),
            );
        }
        let id = team.map(|team| team.id);
        let node = catalog.map(|catalog| catalog.binding.client.target());
        let open = team.cloned();
        let owner = cx.entity().downgrade();
        let context = owner.clone();
        let key = id.map_or_else(|| "default".to_owned(), |id| id.to_string());
        // Bleed through the card inset so the hover fills the card; its mask rounds the ends.
        ListItem::new(SharedString::from(format!("team-{key}")))
            .mx_neg_4()
            .px_4()
            .py_3()
            .disabled(catalog.is_none())
            .accessibility_label(name.clone())
            .on_click(move |_, window, cx| {
                let target = open.clone().map_or(Target::Default, Target::Edit);
                _ = owner.update(cx, |workspace, cx| workspace.open_team(target, window, cx));
            })
            .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                let Some(node) = node else {
                    return;
                };
                cx.stop_propagation();
                _ = context.update(cx, |workspace, cx| {
                    workspace.teams.focus.focus(window, cx);
                });
                menu::show(node, id, event.position, window, cx);
            })
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .child(tile(
                        team.and_then(|team| team.appearance.as_ref()),
                        Icon::from(gpui_kit::assets::IconName::Users),
                        cx,
                    ))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap_2()
                                    .child(div().truncate().child(name))
                                    .when(team.is_none(), |row| {
                                        row.child(
                                            Tag::secondary().small().child(tr("team_builtin")),
                                        )
                                    }),
                            )
                            .when(!description.is_empty(), |column| {
                                column.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(description),
                                )
                            }),
                    )
                    .child(
                        // Side by side rather than stacked; the count beside them gives the total.
                        h_flex().flex_shrink_0().gap_1().children(
                            selected.iter().take(5).map(|role| {
                                avatar(role.appearance.as_ref(), cx).with_size(px(20.))
                            }),
                        ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(meta),
                    )
                    .child(
                        Icon::new(IconName::ChevronRight)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    ),
            )
            .into_any_element()
    }
}
