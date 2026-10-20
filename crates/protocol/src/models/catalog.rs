//! Installed-model metadata includes the selected Node's execution availability.
use super::{Kind, Model};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub models: Vec<Model>,
    pub availability: BTreeMap<Kind, Availability>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Availability {
    Available,
    Unavailable { reason: String },
}

impl Availability {
    pub fn available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_execution_availability() {
        let catalog = Catalog {
            models: Vec::new(),
            availability: BTreeMap::from([
                (Kind::Embedding, Availability::Available),
                (
                    Kind::Decision,
                    Availability::Unavailable {
                        reason: "runtime is unavailable".into(),
                    },
                ),
            ]),
        };
        let bytes = serde_json::to_vec(&catalog).unwrap();
        assert_eq!(serde_json::from_slice::<Catalog>(&bytes).unwrap(), catalog);
        assert!(catalog.availability[&Kind::Embedding].available());
        assert!(!catalog.availability[&Kind::Decision].available());
        let value = crate::plugin::transaction::sdk::output(crate::Output::LocalModels(catalog));
        assert_eq!(value["data"], serde_json::json!([]));
    }
}
