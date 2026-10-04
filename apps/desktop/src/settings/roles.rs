//! Core composer references retain the execution Node role catalog independently of its editor.
use super::Workspace;
use crate::tr;
use sailry_protocol::{RoleId, conversation::Provider, role};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Target {
    key: usize,
    revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Role {
    target: Target,
    pub id: String,
    pub name: String,
    pub appearance: Option<sailry_protocol::projects::Appearance>,
    pub description: String,
    pub model: Option<(usize, String)>,
    pub effort: Option<sailry_protocol::Effort>,
    pub max_turns: Option<u32>,
    pub skills: Vec<String>,
    pub instructions: String,
}

impl Role {
    pub fn example() -> Self {
        Self {
            target: Target {
                key: 0,
                revision: 0,
            },
            id: "preview-reviewer".into(),
            name: tr("role_example").to_string(),
            appearance: None,
            description: tr("role_example_description").to_string(),
            model: None,
            effort: None,
            max_turns: None,
            skills: Vec::new(),
            instructions: String::new(),
        }
    }
}

impl Workspace {
    pub(crate) fn role_profiles(&self) -> &[Role] {
        &self.roles
    }

    fn validate_role_model(&self, role: &Role) -> Result<(), &'static str> {
        if let Some((channel, model)) = &role.model {
            let model = self
                .providers
                .channels
                .iter()
                .find(|entry| entry.id == *channel && entry.enabled)
                .and_then(|entry| entry.models.iter().find(|entry| entry.id == *model))
                .ok_or("role_model_unavailable")?;
            let valid = if model.reasoning {
                role.effort
                    .is_none_or(|effort| model.efforts.contains(&effort))
            } else {
                role.effort.is_none()
            };
            if !valid {
                return Err("role_effort_invalid");
            }
        }
        Ok(())
    }

    pub(crate) fn validate_role_options(&self, role: &Role) -> Result<(), &'static str> {
        self.validate_role_model(role)?;
        if role.skills.len() > 3 {
            return Err("role_skills_limit");
        }
        if role.skills.iter().any(|id| {
            !self
                .skills
                .iter()
                .any(|skill| skill.id() == *id && skill.enabled)
        }) {
            return Err("role_skill_unavailable");
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Catalog {
    ids: BTreeMap<RoleId, usize>,
}

impl Catalog {
    pub(super) fn accept(
        &mut self,
        profiles: &[role::Profile],
        providers: &BTreeMap<usize, Provider>,
    ) -> Vec<Role> {
        profiles
            .iter()
            .map(|profile| {
                let next = self.ids.len();
                let key = *self.ids.entry(profile.id).or_insert(next);
                Role {
                    target: Target {
                        key,
                        revision: profile.revision,
                    },
                    id: profile.key.clone(),
                    name: profile.name.clone(),
                    appearance: profile.appearance.clone(),
                    description: profile.description.clone(),
                    model: profile.model.as_ref().map(|model| {
                        (
                            providers
                                .iter()
                                .find(|(_, provider)| provider.id == model.provider)
                                .map_or(usize::MAX, |(&key, _)| key),
                            model.model.clone(),
                        )
                    }),
                    effort: profile.model.as_ref().and_then(|model| model.effort),
                    max_turns: profile.max_turns,
                    skills: profile.skills.clone(),
                    instructions: profile.instructions.clone(),
                }
            })
            .collect()
    }
}
