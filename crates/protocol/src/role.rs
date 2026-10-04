//! Node-owned named roles. Session execution freezes the selected definitions separately.
use crate::{Effort, ErrorCode, Fault, ProviderId, RoleId};
use serde::{Deserialize, Serialize};

pub const MAX_PROFILES: usize = 32;

/// The execution-Node catalog revision explicitly selected by a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub id: RoleId,
    pub revision: u64,
}

/// Immutable definitions and fixed-model providers owned by a session revision.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub profiles: Vec<Profile>,
    pub providers: Vec<crate::conversation::Provider>,
}

impl Snapshot {
    pub fn validate(&self) -> Result<(), Fault> {
        use std::collections::BTreeSet;
        if self.profiles.len() > MAX_PROFILES {
            return Err(invalid("session role limit exceeded"));
        }
        let mut ids = BTreeSet::new();
        let mut keys = BTreeSet::new();
        let mut required = BTreeSet::new();
        for profile in &self.profiles {
            profile.validate()?;
            if !ids.insert(profile.id) || !keys.insert(&profile.key) {
                return Err(invalid(
                    "session roles must have distinct identities and keys",
                ));
            }
            if let Some(model) = &profile.model {
                required.insert(model.provider);
            }
        }
        let providers: BTreeSet<_> = self.providers.iter().map(|provider| provider.id).collect();
        if providers.len() != self.providers.len() || providers != required {
            return Err(invalid(
                "session role providers do not match the selected models",
            ));
        }
        Ok(())
    }
}

/// Write-only credential material for a fixed-model provider in a session import.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSecret {
    pub provider: ProviderId,
    pub secret: crate::Secret,
    pub expires_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub provider: ProviderId,
    pub model: String,
    /// None uses the selected model's default effort.
    pub effort: Option<Effort>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: RoleId,
    pub revision: u64,
    /// Editable name used to select a role; identity remains `id` across renames.
    pub key: String,
    pub name: String,
    /// Optional visual identity; absent uses the controller's standard agent icon.
    pub appearance: Option<crate::projects::Appearance>,
    pub description: String,
    /// None inherits the parent turn's model and reasoning configuration.
    pub model: Option<Model>,
    pub max_turns: Option<u32>,
    pub skills: Vec<String>,
    pub instructions: String,
}

impl Profile {
    pub fn reference(&self) -> Reference {
        Reference {
            id: self.id,
            revision: self.revision,
        }
    }

    pub fn validate(&self) -> Result<(), Fault> {
        let key = self.key.as_bytes();
        if key.len() > 128
            || !key.first().is_some_and(u8::is_ascii_lowercase)
            || !key.last().is_some_and(u8::is_ascii_alphanumeric)
            || !key.iter().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(byte)
            })
        {
            return Err(invalid(
                "role key must use lowercase letters, digits, underscores or hyphens",
            ));
        }
        if self.name.trim().is_empty()
            || self.name.len() > 128
            || self.description.len() > 1024
            || self.instructions.len() > 16 * 1024
            || self.max_turns == Some(0)
        {
            return Err(invalid("role text or turn limit is invalid"));
        }
        if self.appearance.as_ref().is_some_and(|appearance| {
            !crate::projects::ICONS.contains(&appearance.icon.as_str())
                || !crate::projects::COLORS.contains(&appearance.color.as_str())
        }) {
            return Err(invalid("role appearance is invalid"));
        }
        if self
            .model
            .as_ref()
            .is_some_and(|model| model.model.trim().is_empty() || model.model.len() > 256)
        {
            return Err(invalid("role model identifier is invalid"));
        }
        let mut skills = std::collections::BTreeSet::new();
        if self.skills.len() > 3
            || self
                .skills
                .iter()
                .any(|skill| skill.trim().is_empty() || skill.len() > 256 || !skills.insert(skill))
        {
            return Err(invalid("role must reference at most three distinct skills"));
        }
        Ok(())
    }
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
mod tests;
