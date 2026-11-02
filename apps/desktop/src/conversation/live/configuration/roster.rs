//! Per-conversation team and member models. Sessions change them through
//! revision-checked commands; drafts keep them until the session starts.
use super::super::model_controls::team::{Draft, Roster, Team, effective};
use super::*;
use sailry_protocol::{ProviderId, RoleId, conversation::Execution, role, team};

/// The selected member's model as the popover shows it.
pub(in crate::conversation::live) struct MemberModel {
    pub label: SharedString,
    pub choices: Vec<Effort>,
    pub effort: Effort,
    pub selection: Option<crate::model_picker::Selection>,
    pub overridden: bool,
}

impl View {
    fn team_catalog(&self) -> (&[team::Profile], &[role::Profile]) {
        match &self.node.snapshot {
            Some(snapshot) => (snapshot.teams.as_slice(), snapshot.roles.as_slice()),
            None => (&[][..], &[][..]),
        }
    }

    /// The session roster, else the draft team's members from the execution Node catalog.
    fn member_profiles(&self) -> Vec<&role::Profile> {
        if self.execution() == Execution::Model {
            return Vec::new();
        }
        if let Some(session) = &self.session {
            return session.roles.profiles.iter().collect();
        }
        let (teams, roles) = self.team_catalog();
        match self
            .team_draft
            .team
            .and_then(|id| teams.iter().find(|team| team.id == id))
        {
            None => roles.iter().collect(),
            Some(team) => team
                .members
                .iter()
                .filter_map(|id| roles.iter().find(|role| role.id == *id))
                .collect(),
        }
    }

    fn member_overrides(&self) -> &BTreeMap<RoleId, role::Model> {
        self.session
            .as_ref()
            .map_or(&self.team_draft.models, |session| &session.roles.models)
    }

    fn execution(&self) -> Execution {
        self.session
            .as_ref()
            .map_or(Execution::Team, |session| session.roles.execution)
    }

    pub(in crate::conversation::live) fn team_roster(&self) -> Roster {
        let (teams, roles) = self.team_catalog();
        let team = match &self.session {
            Some(session) => session.roles.team.map(|reference| {
                teams
                    .iter()
                    .find(|team| team.id == reference.id)
                    .map_or_else(
                        || Team {
                            reference,
                            name: tr("composer_team_unknown"),
                            appearance: None,
                            members: session.roles.profiles.iter().map(|role| role.id).collect(),
                            model: None,
                        },
                        // The session keeps its captured revision.
                        |profile| Team {
                            reference,
                            ..Team::from(profile)
                        },
                    )
            }),
            None => self
                .team_draft
                .team
                .and_then(|id| teams.iter().find(|team| team.id == id))
                .map(Team::from),
        };
        Roster {
            execution: self.execution(),
            team,
            members: Roster::members(self.member_profiles(), self.member_overrides()),
            teams: teams.iter().map(Team::from).collect(),
            catalog: roles.iter().map(role::Profile::reference).collect(),
        }
    }

    /// The member's effective model; None follows the lead.
    fn member_model(&self, role: RoleId) -> Option<role::Model> {
        self.member_profiles()
            .into_iter()
            .find(|profile| profile.id == role)
            .and_then(|profile| effective(self.member_overrides(), profile).cloned())
    }

    fn member_provider(&self, id: ProviderId) -> Option<&Provider> {
        self.session
            .iter()
            .flat_map(|session| &session.roles.providers)
            .chain(
                self.node
                    .snapshot
                    .iter()
                    .flat_map(|snapshot| &snapshot.providers),
            )
            .find(|provider| provider.id == id)
    }

    pub(in crate::conversation::live) fn member_settings(&self, role: RoleId) -> MemberModel {
        let target = self.binding.client.target();
        let (provider, node, model, effort) = match self.member_model(role) {
            Some(model) => (
                self.member_provider(model.provider),
                target,
                model.model,
                model.effort,
            ),
            None => match &self.config {
                Some(config) => (
                    self.providers()
                        .find(|provider| provider.id == config.provider),
                    self.config_owner,
                    config.model.clone(),
                    Some(config.effort),
                ),
                None => (None, target, String::new(), None),
            },
        };
        let entry = provider.and_then(|provider| {
            provider
                .models
                .iter()
                .find(|candidate| candidate.id == model)
        });
        let reasoning = entry.filter(|entry| entry.reasoning);
        let default = reasoning.map_or(Effort::Default, |entry| {
            Effort::initial(&entry.efforts, entry.default_effort)
        });
        MemberModel {
            label: entry
                .and_then(|entry| entry.display_name.clone())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| model.clone())
                .into(),
            choices: reasoning
                .map(|entry| crate::reasoning::choices(&entry.efforts))
                .unwrap_or_default(),
            effort: effort.unwrap_or(default),
            selection: provider
                .and_then(|provider| self.provider_ids.get(&(node, provider.id)))
                .map(|channel| crate::model_picker::Selection {
                    channel: *channel,
                    model,
                }),
            overridden: self.member_overrides().contains_key(&role),
        }
    }

    fn team_blocked(&self) -> bool {
        self.readonly()
            || self.pending
            || self.configuration_blocked()
            || (self.session.is_some() && !self.connected())
    }

    /// Chooses a team; a team with a fixed lead model also switches the lead.
    pub(in crate::conversation::live) fn select_team(
        &mut self,
        team: Option<Team>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.team_blocked() {
            return;
        }
        let lead = team.as_ref().and_then(|team| team.model.clone());
        let reference = team.map(|team| team.reference);
        if let Some(session) = &self.session {
            let command = Command::SetSessionExecution {
                session: session.id,
                expected_revision: session.revision,
                execution: Execution::Team,
                team: reference.map(Into::into),
                models: BTreeMap::new(),
            };
            self.execute(command, actions::Action::Team, window, cx);
            return;
        }
        // Node clears member models with a new team; drafts follow the same rule.
        self.team_draft = Draft {
            team: reference.map(|reference| reference.id),
            models: BTreeMap::new(),
        };
        if let Some(model) = lead {
            self.apply_lead(&model, window, cx);
        }
        cx.notify();
    }

    /// Previews a draft team's lead model; Node admission applies its configuration.
    fn apply_lead(&mut self, model: &role::Model, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.binding.client.target();
        let Some(channel) = self.provider_ids.get(&(target, model.provider)).copied() else {
            return;
        };
        let effort = model.effort.unwrap_or_else(|| {
            self.model_sources()
                .find(|(node, provider)| *node == target && provider.id == model.provider)
                .and_then(|(_, provider)| {
                    provider.models.iter().find(|entry| entry.id == model.model)
                })
                .filter(|entry| entry.reasoning)
                .map_or(Effort::Default, |entry| {
                    Effort::initial(&entry.efforts, entry.default_effort)
                })
        });
        self.select_model(
            &crate::model_picker::Selection {
                channel,
                model: model.model.clone(),
            },
            effort,
            window,
            cx,
        );
    }

    /// Sets or clears one member's model for this conversation only.
    pub(in crate::conversation::live) fn select_member_model(
        &mut self,
        role: RoleId,
        model: Option<role::Model>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.team_blocked()
            || !self
                .member_profiles()
                .iter()
                .any(|profile| profile.id == role)
            || self.member_overrides().get(&role) == model.as_ref()
        {
            return;
        }
        if let Some(session) = &self.session {
            let command = Command::SetSessionMemberModel {
                session: session.id,
                expected_revision: session.revision,
                role,
                model,
            };
            self.execute(command, actions::Action::Member, window, cx);
            return;
        }
        match model {
            Some(model) => {
                self.team_draft.models.insert(role, model);
            }
            None => {
                self.team_draft.models.remove(&role);
            }
        }
        cx.notify();
    }

    pub(in crate::conversation::live) fn pick_member_model(
        &mut self,
        role: RoleId,
        selection: &crate::model_picker::Selection,
        effort: Effort,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.binding.client.target();
        let model = self
            .model_sources()
            .find(|(node, provider)| {
                *node == target
                    && provider.enabled
                    && self.provider_ids.get(&(*node, provider.id)) == Some(&selection.channel)
            })
            .and_then(|(_, provider)| {
                let entry = provider
                    .models
                    .iter()
                    .find(|entry| entry.id == selection.model)?;
                Some(role::Model {
                    provider: provider.id,
                    model: entry.id.clone(),
                    effort: entry.reasoning.then_some(effort),
                })
            });
        if let Some(model) = model {
            self.select_member_model(role, Some(model), window, cx);
        }
    }

    /// A member that follows the lead takes the lead's model with the chosen effort.
    pub(in crate::conversation::live) fn select_member_effort(
        &mut self,
        role: RoleId,
        effort: Effort,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.member_settings(role).choices.contains(&effort) {
            return;
        }
        let base = match self.member_model(role) {
            Some(model) => model,
            None => match &self.config {
                Some(config) if self.config_owner == self.binding.client.target() => role::Model {
                    provider: config.provider,
                    model: config.model.clone(),
                    effort: None,
                },
                _ => return,
            },
        };
        self.select_member_model(
            role,
            Some(role::Model {
                effort: Some(effort),
                ..base
            }),
            window,
            cx,
        );
    }

    pub(in crate::conversation::live) fn open_team_settings(&self, cx: &mut Context<Self>) {
        cx.emit(Event::TeamSettings(self.binding.client.target()));
    }
}
