//! Configuration and installation snapshots for Node-owned local model kinds.
use super::{Definition, Kind, valid_id};
use crate::Fault;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub enabled: bool,
    pub model: Option<String>,
}

impl Choice {
    fn initial(kind: Kind) -> Self {
        Self {
            enabled: true,
            model: (kind == Kind::Embedding).then(|| "granite-multi".into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub mirror: Option<String>,
    #[serde(default)]
    pub choices: BTreeMap<Kind, Choice>,
}

impl Default for Settings {
    fn default() -> Self {
        let mut settings = Self {
            revision: 0,
            mirror: None,
            choices: BTreeMap::new(),
        };
        settings.fill_defaults();
        settings
    }
}

impl Settings {
    pub fn fill_defaults(&mut self) {
        for kind in [Kind::Embedding, Kind::Ocr] {
            self.choices
                .entry(kind)
                .or_insert_with(|| Choice::initial(kind));
        }
    }

    pub fn choice(&self, kind: Kind) -> Choice {
        self.choices
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| Choice::initial(kind))
    }

    pub fn selected(&self, kind: Kind) -> Option<&str> {
        match self.choices.get(&kind) {
            Some(choice) => choice.enabled.then_some(choice.model.as_deref()).flatten(),
            None if kind == Kind::Embedding => Some("granite-multi"),
            None => None,
        }
    }

    pub fn validate(&self) -> Result<(), Fault> {
        if self.revision > i64::MAX as u64
            || self.choices.iter().any(|(kind, choice)| {
                *kind == Kind::Speech || choice.model.as_ref().is_some_and(|id| !valid_id(id))
            })
        {
            return Err(Fault::invalid("invalid local model settings"));
        }
        if let Some(mirror) = &self.mirror {
            let url =
                url::Url::parse(mirror).map_err(|_| Fault::invalid("invalid model mirror"))?;
            if mirror.len() > 4096
                || url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(Fault::invalid(
                    "model mirror requires an HTTPS URL without credentials",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub definition: Definition,
    pub state: State,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum State {
    Missing,
    Downloading { percent: u8, bytes_per_second: u64 },
    Installed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_missing_choices_and_preserves_explicit_disable() {
        let mut settings:Settings = serde_json::from_str(r#"{"revision":4,"mirror":null,"choices":{"embedding":{"enabled":false,"model":null}}}"#).unwrap();
        settings.fill_defaults();
        assert_eq!(settings.selected(Kind::Embedding), None);
        assert_eq!(
            settings.choice(Kind::Ocr),
            Choice {
                enabled: true,
                model: None
            }
        );
        assert_eq!(
            Settings::default().selected(Kind::Embedding),
            Some("granite-multi")
        );
        settings.validate().unwrap();
    }

    #[test]
    fn validates_node_kinds_ids_and_https_mirror() {
        for mirror in [
            "http://example.invalid",
            "https://user:secret@example.invalid",
            "https://example.invalid?query=1",
        ] {
            assert!(
                Settings {
                    mirror: Some(mirror.into()),
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
        Settings {
            mirror: Some("https://models.example.invalid/cache".into()),
            ..Settings::default()
        }
        .validate()
        .unwrap();
        let mut settings = Settings::default();
        settings.choices.insert(
            Kind::Ocr,
            Choice {
                enabled: true,
                model: Some("../outside".into()),
            },
        );
        assert!(settings.validate().is_err());
        settings.choices.remove(&Kind::Ocr);
        settings.choices.insert(
            Kind::Speech,
            Choice {
                enabled: true,
                model: None,
            },
        );
        assert!(settings.validate().is_err());
    }
}
