//! Work modes contribute instructions while Node policy owns their authority.
use super::desktop::Navigation;
use crate::{SessionConfig, WorkMode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConversationMode")]
pub struct Declaration {
    pub id: String,
    /// A string option in this package's conversation schema; empty selects no mode.
    pub option: String,
    pub display: Navigation,
    pub permissions: Permissions,
    /// Coordinator instructions; child roles retain their own task instructions.
    pub instructions: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ModePermissions")]
pub struct Permissions {
    /// Writing still requires the captured session's ordinary tool permission.
    pub write: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub package: super::Reference,
    pub id: String,
}

impl Declaration {
    pub fn valid(&self) -> bool {
        super::ui::command_name(&self.id)
            && !self.option.is_empty()
            && self.option.len() <= 64
            && self
                .option
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            && self.display.valid()
            && !self.instructions.trim().is_empty()
            && self.instructions.len() <= 16 * 1024
            && !self.instructions.contains('\0')
    }

    pub fn selected(&self, options: Option<&BTreeMap<String, serde_json::Value>>) -> bool {
        options
            .and_then(|values| values.get(&self.option))
            .and_then(serde_json::Value::as_str)
            == Some(self.id.as_str())
    }

    pub fn base(&self) -> WorkMode {
        if self.permissions.write {
            WorkMode::Code
        } else {
            WorkMode::Plan
        }
    }

    pub fn allows(&self, config: &SessionConfig) -> bool {
        self.permissions.write || config.mode == WorkMode::Plan
    }
}

pub fn valid(declarations: &[Declaration]) -> bool {
    declarations.len() <= 16
        && declarations.iter().enumerate().all(|(index, declaration)| {
            declaration.valid()
                && !declarations[..index]
                    .iter()
                    .any(|other| other.id == declaration.id)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_identity_and_authority() {
        let mut declaration: Declaration = serde_json::from_value(json!({
            "id":"review", "option":"mode", "display":{"label":"Review"},
            "permissions":{"write":false}, "instructions":"Inspect the requested changes"
        }))
        .unwrap();
        assert!(valid(&[declaration.clone()]));
        assert_eq!(declaration.base(), WorkMode::Plan);
        assert!(declaration.selected(Some(&BTreeMap::from([("mode".into(), json!("review"))]))));
        assert!(!declaration.selected(None));
        assert!(!valid(&[declaration.clone(), declaration.clone()]));
        declaration.id = "../review".into();
        assert!(!declaration.valid());
    }
}
