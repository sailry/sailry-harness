//! Model-advertised choices are independent of reasoning and use captured configuration.
use super::{Model, ModelApi};
use crate::{Fault, plugin::desktop::Icon};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feature {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    pub options: Vec<FeatureOption>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOption {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

pub fn validate(features: &[Feature]) -> Result<(), Fault> {
    let mut ids = BTreeSet::new();
    if features.len() > 16 {
        return Err(invalid());
    }
    for feature in features {
        let mut options = BTreeSet::new();
        if !identifier(&feature.id)
            || !ids.insert(&feature.id)
            || !name(feature.name.as_deref())
            || feature.icon.as_ref().is_some_and(|icon| !icon.valid())
            || feature.options.is_empty()
            || feature.options.len() > 16
        {
            return Err(invalid());
        }
        for option in &feature.options {
            if !identifier(&option.id)
                || !options.insert(&option.id)
                || !name(option.name.as_deref())
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

impl Model {
    pub fn validate_features(
        &self,
        api: ModelApi,
        selected: &BTreeMap<String, String>,
    ) -> Result<(), Fault> {
        let features = self
            .capabilities
            .as_ref()
            .map_or(&[][..], |value| value.features.as_slice());
        validate(features)?;
        for (id, value) in selected {
            if !advertised(features, id, value) {
                return Err(Fault::invalid("model feature choice is not advertised"));
            }
            if !supported(api, id, value) {
                return Err(Fault::invalid(
                    "model feature choice is unsupported by the API",
                ));
            }
        }
        Ok(())
    }

    pub fn retain_features(&self, api: ModelApi, selected: &mut BTreeMap<String, String>) {
        let features = self
            .capabilities
            .as_ref()
            .map_or(&[][..], |value| value.features.as_slice());
        selected.retain(|id, value| advertised(features, id, value) && supported(api, id, value));
    }
}

fn advertised(features: &[Feature], id: &str, value: &str) -> bool {
    features
        .iter()
        .any(|feature| feature.id == id && feature.options.iter().any(|option| option.id == value))
}

fn supported(api: ModelApi, id: &str, value: &str) -> bool {
    api == ModelApi::Plugin || (api == ModelApi::Responses && id == "speed" && value == "fast")
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

fn name(value: Option<&str>) -> bool {
    value.is_none_or(|value| {
        !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
    })
}

fn invalid() -> Fault {
    Fault::invalid("model feature declarations must be unique and bounded")
}

#[cfg(test)]
mod tests;
