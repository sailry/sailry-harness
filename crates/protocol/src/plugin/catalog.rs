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
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Details {
    Plugin(Box<Info>),
    Skills(Box<SkillCollection>),
    Unsupported(Box<External>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub author: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Codex,
    ClaudeCode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCollection {
    pub source: skills::Resolved,
    pub metadata: Metadata,
    pub skills: Vec<skills::Candidate>,
    pub unsupported: Vec<Format>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct External {
    pub source: skills::Resolved,
    pub metadata: Metadata,
    pub formats: Vec<Format>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<super::Category>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<super::Tint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<ReleaseNotes>,
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub description_locales: std::collections::BTreeMap<String, String>,
    pub display: Option<super::desktop::Navigation>,
    pub icon: Option<String>,
    pub repository: Option<String>,
    pub bundled: bool,
    #[serde(default)]
    pub provides_models: bool,
    #[serde(default)]
    pub permissions: Vec<super::Action>,
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

/// Public text for the package version being offered, never an executable document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseNotes {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub locales: std::collections::BTreeMap<String, String>,
}

impl ReleaseNotes {
    pub fn text(&self, locale: &str) -> &str {
        self.locales
            .get(locale)
            .map(String::as_str)
            .unwrap_or(&self.text)
    }
    pub fn valid(&self) -> bool {
        std::iter::once(&self.text)
            .chain(self.locales.values())
            .all(|value| {
                !value.trim().is_empty()
                    && !value.chars().any(|character| {
                        character.is_control() && !matches!(character, '\r' | '\n' | '\t')
                    })
            })
            && self.locales.keys().all(|locale| {
                !locale.is_empty()
                    && locale.split('-').all(|part| {
                        !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    })
            })
    }
}
