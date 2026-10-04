//! Catalog entries and immutable repository selections for plugin installation.
use super::{Info, skills};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Official,
    ThirdParty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub source: skills::Resolved,
    pub path: String,
    pub info: Info,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub description_locales: std::collections::BTreeMap<String, String>,
    pub display: Option<super::desktop::Navigation>,
    pub icon: Option<String>,
    pub repository: Option<String>,
    pub bundled: bool,
}

impl Entry {
    pub fn description(&self, locale: &str) -> Option<&str> {
        self.description_locales
            .get(locale)
            .map(String::as_str)
            .or(self.description.as_deref())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub page: u32,
    pub pages: u32,
    pub unavailable: bool,
}
