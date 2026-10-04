//! Provider-executed capabilities, separate from local function tools.
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    WebSearch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub capability: Capability,
    pub display: super::desktop::Navigation,
}

pub fn valid(declarations: &[Declaration]) -> bool {
    declarations.len() <= 16
        && declarations.iter().enumerate().all(|(index, declaration)| {
            declaration.display.valid()
                && !declarations[..index]
                    .iter()
                    .any(|other| other.capability == declaration.capability)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_localized_unique_capabilities() {
        let declaration: Declaration = serde_json::from_value(serde_json::json!({
            "capability": "web_search",
            "display": {"label": "Search", "locales": {"zh-CN": "搜索"}, "icon": "reicon:ui/magnifier"}
        }))
        .unwrap();
        assert!(valid(std::slice::from_ref(&declaration)));
        assert!(!valid(&[declaration.clone(), declaration.clone()]));
        let mut invalid = declaration;
        invalid.display.label.clear();
        assert!(!valid(&[invalid]));
        assert!(serde_json::from_value::<Capability>(serde_json::json!("unknown")).is_err());
    }
}
