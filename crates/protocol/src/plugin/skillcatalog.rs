//! Public source coordinates for real skills in the official package inventory.
use super::skills;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Official,
    SkillsSh,
    Skillhub,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum InstallSource {
    Git(skills::Source),
    Registry(skills::RegistrySource),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub package: Option<String>,
    pub name: String,
    pub display_name: Option<String>,
    pub display_locales: BTreeMap<String, String>,
    pub description: Option<String>,
    pub description_locales: BTreeMap<String, String>,
    pub origin: String,
    pub url: Option<String>,
    pub source: Option<InstallSource>,
    pub bundled: bool,
}

impl Entry {
    pub fn title(&self, locale: &str) -> &str {
        self.display_locales
            .get(locale)
            .map(String::as_str)
            .or(self.display_name.as_deref())
            .unwrap_or(&self.name)
    }

    pub fn description(&self, locale: &str) -> Option<&str> {
        self.description_locales
            .get(locale)
            .map(String::as_str)
            .or(self.description.as_deref())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryScope {
    Catalog,
    Page,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub query_scope: QueryScope,
    pub entries: Vec<Entry>,
    pub page: u32,
    pub next: Option<u32>,
    pub total: Option<usize>,
    pub unavailable: bool,
}
