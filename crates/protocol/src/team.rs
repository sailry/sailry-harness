//! Node-owned teams select catalog roles and an optional lead configuration.
//! The Default team is virtual: it is never stored and always selects the whole catalog.
use crate::{Fault, RoleId, TeamId};
use serde::{Deserialize, Serialize};

/// The execution-Node catalog revision explicitly selected by a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub id: TeamId,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: TeamId,
    pub revision: u64,
    /// Editable name used to select a team; identity remains `id` across renames.
    pub key: String,
    pub name: String,
    /// Optional visual identity; absent uses the controller's standard team icon.
    pub appearance: Option<crate::projects::Appearance>,
    pub description: String,
    /// Lead model and effort; None follows the conversation's model.
    pub model: Option<crate::role::Model>,
    /// Added to the lead's system instructions; empty adds nothing.
    pub instructions: String,
    /// Role references in display order; roles stay in the shared catalog.
    pub members: Vec<RoleId>,
}

impl Profile {
    pub fn reference(&self) -> Reference {
        Reference {
            id: self.id,
            revision: self.revision,
        }
    }

    pub fn validate(&self) -> Result<(), Fault> {
        if !crate::role::key(&self.key) {
            return Err(Fault::invalid(
                "team key must use lowercase letters, digits, underscores or hyphens",
            ));
        }
        if self.name.trim().is_empty() {
            return Err(Fault::invalid("team name is invalid"));
        }
        if !crate::role::appearance(self.appearance.as_ref()) {
            return Err(Fault::invalid("team appearance is invalid"));
        }
        if self
            .model
            .as_ref()
            .is_some_and(|model| model.model.trim().is_empty())
        {
            return Err(Fault::invalid("team model identifier is invalid"));
        }
        let mut members = std::collections::BTreeSet::new();
        if !self.members.iter().all(|role| members.insert(role)) {
            return Err(Fault::invalid("team members must be distinct"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
