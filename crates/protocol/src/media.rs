//! Execution-owned companion models, following Code Pi d9b56405.
use crate::{
    Authentication, ProviderId,
    conversation::{Model, ModelApi, Provider},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Vision,
    Image,
    Video,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Generation {
    Image,
    Video,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub provider: ProviderId,
    pub model: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub revision: u64,
    pub bindings: BTreeMap<Kind, Binding>,
}

impl Kind {
    pub fn supports(self, provider: &Provider, model: &Model) -> bool {
        provider.enabled
            && provider.authentication == Authentication::ApiKey
            && match self {
                Self::Vision => model.vision,
                Self::Image | Self::Video => {
                    matches!(
                        provider.api,
                        ModelApi::ChatCompletions | ModelApi::Responses
                    ) && model.generates.contains(&match self {
                        Self::Image => Generation::Image,
                        _ => Generation::Video,
                    })
                }
            }
    }
}

/// An image source confined to the captured worktree or admitted turn attachments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Source {
    Path(String),
    Attachment(crate::AttachmentId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Inspect { prompt: String, source: Source },
    Image { prompt: String, path: String },
    Video { prompt: String, path: String },
}

impl Action {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Inspect { .. } => Kind::Vision,
            Self::Image { .. } => Kind::Image,
            Self::Video { .. } => Kind::Video,
        }
    }
}

/// Safe settings choices; provider endpoints and credentials remain on the Node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub provider: ProviderId,
    pub provider_name: String,
    pub model: String,
    pub kinds: Vec<Kind>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn package_operations_match_their_resource_grants() {
        use crate::{
            plugin,
            tool::{Declaration, Operation},
        };
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../plugins/media/plugin.json")).unwrap();
        let extension: plugin::Extension =
            serde_json::from_value(manifest["extensions"]["dev.sailry.platform"].clone()).unwrap();
        assert_eq!(extension.tools.len(), 3);
        assert!(extension.tools.iter().all(Declaration::valid));
        for (name, operation, read_only) in [
            ("inspect_image", Operation::InspectMedia, true),
            ("generate_image", Operation::GenerateImage, false),
            ("generate_video", Operation::GenerateVideo, false),
        ] {
            let declaration = extension
                .tools
                .iter()
                .find(|tool| tool.name == name)
                .unwrap();
            assert_eq!(
                declaration.handler.as_ref().unwrap().operation,
                Some(operation)
            );
            assert!(extension.actions.contains(&operation.action().unwrap()));
            assert_eq!(operation.read_only(), read_only);
        }
    }

    #[test]
    fn sources_are_exclusive_and_actions_are_typed() {
        let value = json!({"action":"inspect","prompt":"Describe","source":{"kind":"path","value":"image.png"}});
        let action: Action = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(action.kind(), Kind::Vision);
        assert_eq!(serde_json::to_value(action).unwrap(), value);
        for value in [
            json!({"action":"inspect","prompt":"Describe","source":{"kind":"path","value":"image.png","attachment":"extra"}}),
            json!({"action":"image","prompt":"Create","path":"image.png","model":"override"}),
            json!({"action":"video","prompt":"Create","path":"video.mp4","source":{"kind":"path","value":"input.png"}}),
        ] {
            assert!(serde_json::from_value::<Action>(value).is_err());
        }
    }
}
