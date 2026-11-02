//! The popover's team row and team list. Team and member choices are per conversation:
//! sessions send revision-checked commands, drafts keep them until the session starts.
use super::*;
use gpui_kit::component::{
    avatar::Avatar, badge::Badge, scroll::ScrollableElement as _, separator::Separator,
};
use sailry_protocol::{RoleId, TeamId, projects::Appearance, role, team};

/// Height of the team row, including the gap above the effort controls.
pub(super) const ROW_HEIGHT: f32 = 40.;

#[derive(Clone, Debug, PartialEq)]
pub(in crate::conversation::live) struct Team {
    pub reference: team::Reference,
    pub name: SharedString,
    pub appearance: Option<Appearance>,
    pub members: Vec<RoleId>,
    /// Lead model fixed by the team; None follows the conversation.
    pub model: Option<role::Model>,
}

impl From<&team::Profile> for Team {
    fn from(profile: &team::Profile) -> Self {
        Self {
            reference: profile.reference(),
            name: profile.name.clone().into(),
            appearance: profile.appearance.clone(),
            members: profile.members.clone(),
            model: profile.model.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::conversation::live) struct Member {
    pub role: RoleId,
    pub revision: u64,
    pub name: SharedString,
    pub appearance: Option<Appearance>,
    /// The conversation overrides this member's model.
    pub overridden: bool,
}

/// The conversation's team as the popover shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::conversation::live) struct Roster {
    /// The selected stored team; None is the Default team.
    pub team: Option<Team>,
    pub members: Vec<Member>,
    /// Stored teams offered by the team list.
    pub teams: Vec<Team>,
    /// Current role revisions available for explicit team selection.
    pub catalog: Vec<role::Reference>,
}

/// Content of the composer model chip.
#[derive(Debug, PartialEq)]
pub(super) enum Chip<'a> {
    /// Provider badge, model and effort.
    Model,
    /// Team icon and name; the dot marks member overrides.
    Team { team: &'a Team, overridden: bool },
}

impl Roster {
    pub(in crate::conversation::live) fn members<'a>(
        profiles: impl IntoIterator<Item = &'a role::Profile>,
        models: &BTreeMap<RoleId, role::Model>,
    ) -> Vec<Member> {
        profiles
            .into_iter()
            .map(|profile| Member {
                role: profile.id,
                revision: profile.revision,
                name: crate::settings::role_label(profile).0,
                appearance: profile.appearance.clone(),
                overridden: models.contains_key(&profile.id),
            })
            .collect()
    }

    pub(super) fn chip(&self) -> Chip<'_> {
        match &self.team {
            None => Chip::Model,
            Some(team) => Chip::Team {
                team,
                overridden: self.members.iter().any(|member| member.overridden),
            },
        }
    }

    pub(super) fn contains(&self, role: RoleId) -> bool {
        self.members.iter().any(|member| member.role == role)
    }

    fn selected(&self, team: Option<TeamId>) -> bool {
        self.team.as_ref().map(|team| team.reference.id) == team
    }

    /// Explicit selection recaptures changed catalog members, even when the team
    /// revision is unchanged. Unchanged selection keeps conversation overrides.
    pub(super) fn changes(&self, team: Option<&Team>) -> bool {
        if self.team.as_ref().map(|team| team.reference) != team.map(|team| team.reference) {
            return true;
        }
        let selected: BTreeMap<_, _> = self
            .catalog
            .iter()
            .filter(|role| team.is_none_or(|team| team.members.contains(&role.id)))
            .map(|role| (role.id, role.revision))
            .collect();
        let captured: BTreeMap<_, _> = self
            .members
            .iter()
            .map(|role| (role.role, role.revision))
            .collect();
        selected != captured
    }
}

/// Override, else the role model; None follows the lead, as in `role::Snapshot::model`.
pub(in crate::conversation::live) fn effective<'a>(
    models: &'a BTreeMap<RoleId, role::Model>,
    profile: &'a role::Profile,
) -> Option<&'a role::Model> {
    models.get(&profile.id).or(profile.model.as_ref())
}

/// Team choice of a conversation without a session, applied once the session exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::conversation::live) struct Draft {
    /// None is the Default team; the current catalog revision is sent at start.
    pub team: Option<TeamId>,
    pub models: BTreeMap<RoleId, role::Model>,
}

/// One revision-checked change that brings a new session to its draft team.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::conversation::live) enum Step {
    Team(Option<team::Reference>),
    Member(RoleId, role::Model),
}

impl Draft {
    pub(in crate::conversation::live) fn steps(&self, teams: &[team::Profile]) -> Vec<Step> {
        let team = self
            .team
            .and_then(|id| teams.iter().find(|team| team.id == id))
            .map(team::Profile::reference);
        team.map(|team| Step::Team(Some(team)))
            .into_iter()
            .chain(
                self.models
                    .iter()
                    .map(|(role, model)| Step::Member(*role, model.clone())),
            )
            .collect()
    }
}

impl Step {
    /// None when the session already matches or no longer lists the member.
    pub(in crate::conversation::live) fn command(
        &self,
        session: SessionId,
        revision: u64,
        roles: &role::Snapshot,
    ) -> Option<Command> {
        match self {
            Self::Team(team) => (roles.team != *team).then_some(Command::SetSessionTeam {
                session,
                expected_revision: revision,
                team: *team,
            }),
            Self::Member(role, model) => (roles.profiles.iter().any(|item| item.id == *role)
                && roles.models.get(role) != Some(model))
            .then(|| Command::SetSessionMemberModel {
                session,
                expected_revision: revision,
                role: *role,
                model: Some(model.clone()),
            }),
        }
    }
}

/// An identity glyph and the color it sits on.
type Glyph = (Icon, Hsla);

/// A role or team identity: a white glyph on its color; absent appearances stay neutral.
fn identity(
    appearance: Option<&Appearance>,
    fallback: gpui_kit::assets::IconName,
    cx: &App,
) -> Glyph {
    crate::workspace::appearance::filled(appearance, Icon::from(fallback), cx)
}

fn icon(team: Option<&Team>, cx: &App) -> Glyph {
    identity(
        team.and_then(|team| team.appearance.as_ref()),
        gpui_kit::assets::IconName::Users,
        cx,
    )
}

/// Member avatars fill their button so no second circle shows around them.
const MEMBER: Pixels = px(28.);

fn name(team: Option<&Team>) -> SharedString {
    team.map_or_else(|| tr("composer_team_default"), |team| team.name.clone())
}

fn avatar(icon: Glyph, overridden: bool, cx: &App) -> Badge {
    marked(icon, overridden, false, cx)
}

/// The selected member is ringed in the theme accent instead of sitting on a button fill.
fn marked((icon, fill): Glyph, overridden: bool, selected: bool, cx: &App) -> Badge {
    Badge::new()
        .when(overridden, |badge| badge.dot().color(cx.theme().primary))
        .child(
            Avatar::new()
                .with_size(MEMBER)
                .bg(fill)
                .border_color(fill)
                .placeholder(icon.size_4())
                .when(selected, |avatar| {
                    avatar.border_1().border_color(cx.theme().primary)
                }),
        )
}

/// The composer chip's team identity; the dot marks member overrides.
pub(super) fn chip_avatar(team: Option<&Team>, overridden: bool, cx: &App) -> Badge {
    let (glyph, fill) = icon(team, cx);
    Badge::new()
        .when(overridden, |badge| badge.dot().color(cx.theme().primary))
        .child(
            Avatar::new()
                .with_size(px(20.))
                .bg(fill)
                .border_color(fill)
                .placeholder(glyph.size_3()),
        )
}

impl Panel {
    /// Current team, a divider, then Lead and each member; the selected avatar is edited.
    pub(super) fn team_row(
        &self,
        settings: &Settings,
        locked: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let roster = &settings.roster;
        let team = roster.team.as_ref();
        let lead = tr("composer_team_lead");
        h_flex()
            .w_full()
            .min_w_0()
            .h_8()
            .flex_shrink_0()
            .gap_2()
            .items_center()
            .child(
                Button::new("model-team-choose")
                    .text()
                    .small()
                    .h_8()
                    .flex_shrink_0()
                    .max_w(px(140.))
                    .rounded_full()
                    .accessibility_label(name(team))
                    .debug_selector(|| "model-team-choose".into())
                    .disabled(locked)
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_1p5()
                            .items_center()
                            .child(avatar(icon(team, cx), false, cx))
                            .child(div().min_w_0().truncate().text_sm().child(name(team)))
                            .child(Icon::new(IconName::ChevronRight).size_3()),
                    )
                    .on_click(cx.listener(|panel, _, _, cx| {
                        panel.layer = Layer::Teams;
                        cx.notify();
                    })),
            )
            .child(Separator::vertical().h_4().flex_shrink_0())
            .child(
                h_flex()
                    .id("model-team-members")
                    .debug_selector(|| "model-team-members".into())
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .items_center()
                    .child(self.member_button(
                        "model-team-lead",
                        None,
                        lead.clone(),
                        identity(None, gpui_kit::assets::IconName::Crown, cx),
                        false,
                        cx,
                    ))
                    .children(roster.members.iter().enumerate().map(|(index, member)| {
                        self.member_button(
                            SharedString::from(format!("model-team-member-{index}")),
                            Some(member.role),
                            member.name.clone(),
                            identity(
                                member.appearance.as_ref(),
                                gpui_kit::assets::IconName::Bot,
                                cx,
                            ),
                            member.overridden,
                            cx,
                        )
                    }))
                    .overflow_x_scrollbar(),
            )
            .into_any_element()
    }

    fn member_button(
        &self,
        id: impl Into<SharedString>,
        role: Option<RoleId>,
        name: SharedString,
        icon: Glyph,
        overridden: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.into();
        let selector = id.clone();
        let selected = self.member == role;
        Button::new(id)
            .ghost()
            .rounded_full()
            .w(MEMBER)
            .h(MEMBER)
            .p_0()
            .flex_shrink_0()
            .tooltip(name.clone())
            .accessibility_label(name)
            .debug_selector(move || selector.to_string())
            .child(marked(icon, overridden, selected, cx))
            .on_click(cx.listener(move |panel, _, _, cx| panel.select_member(role, cx)))
            .into_any_element()
    }

    /// Team choices under a header with back and title.
    pub(super) fn team_list(
        &self,
        settings: &Settings,
        size: gpui_kit::Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let roster = &settings.roster;
        let locked = settings.busy || settings.pending || !settings.connected;
        let choices = std::iter::once((None, None, roster.catalog.len()))
            .chain(
                roster
                    .teams
                    .iter()
                    .map(|team| (Some(team.reference.id), Some(team), team.members.len())),
            )
            .enumerate()
            .map(|(index, (id, team, members))| {
                let selected = roster.selected(id);
                Button::new(("model-team-option", index))
                    .text()
                    .w_full()
                    .h_8()
                    .px_2()
                    .disabled(locked)
                    .selected(selected)
                    .accessibility_label(name(team))
                    .debug_selector(move || format!("model-team-option-{index}"))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .items_center()
                            .child(avatar(icon(team, cx), false, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .line_height(px(20.))
                                    .child(name(team)),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(members.to_string()),
                            )
                            .child(div().size_4().flex_shrink_0().when(selected, |check| {
                                check.child(Icon::new(IconName::Check).size_4())
                            })),
                    )
                    .on_click(
                        cx.listener(move |panel, _, window, cx| panel.choose_team(id, window, cx)),
                    )
            })
            .collect::<Vec<_>>();
        let owner = self.owner.clone();
        v_flex()
            .debug_selector(|| "model-team-list".into())
            .flex_shrink_0()
            .w(size.width)
            .h(size.height)
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .h_8()
                    .flex_shrink_0()
                    .items_center()
                    .child(
                        Button::new("model-team-back")
                            .ghost()
                            .small()
                            .icon(IconName::ArrowLeft)
                            .label(tr("composer_back"))
                            .on_click(cx.listener(|panel, _, window, cx| panel.back(window, cx))),
                    ),
            )
            .child(
                v_flex()
                    .id("model-team-scroll")
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .overflow_y_scrollbar()
                    .children(choices)
                    .child(Separator::horizontal().my_1())
                    .child(
                        Button::new("model-team-manage")
                            .text()
                            .w_full()
                            .h_8()
                            .px_2()
                            .accessibility_label(tr("composer_team_manage"))
                            .debug_selector(|| "model-team-manage".into())
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .gap_2()
                                    .items_center()
                                    .text_color(cx.theme().muted_foreground)
                                    // Same slot as the team avatars so the labels line up.
                                    .child(
                                        div()
                                            .w(MEMBER)
                                            .flex_shrink_0()
                                            .flex()
                                            .justify_center()
                                            .child(Icon::new(IconName::Settings).size_4()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_sm()
                                            .child(tr("composer_team_manage")),
                                    ),
                            )
                            .on_click(move |_, _, cx| {
                                let _ = owner.update(cx, |view, cx| view.open_team_settings(cx));
                            }),
                    ),
            )
            .into_any_element()
    }

    fn select_member(&mut self, role: Option<RoleId>, cx: &mut Context<Self>) {
        if self.member == role {
            return;
        }
        self.member = role;
        self.dragging = false;
        // The owner derives the selected avatar's settings on its next render.
        let _ = self.owner.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    fn choose_team(&mut self, team: Option<TeamId>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = &self.settings else {
            return;
        };
        let chosen = team.and_then(|id| {
            settings
                .roster
                .teams
                .iter()
                .find(|team| team.reference.id == id)
                .cloned()
        });
        if team.is_some() && chosen.is_none() {
            return;
        }
        if settings.roster.changes(chosen.as_ref()) {
            let _ = self
                .owner
                .update(cx, |view, cx| view.select_team(chosen, window, cx));
            self.member = None;
        }
        self.back(window, cx);
    }
}
