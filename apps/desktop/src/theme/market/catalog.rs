use super::{FILE_BYTES, Failure, MANIFEST_BYTES, PACKAGE_BYTES, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Catalog {
    pub version: u32,
    pub themes: Vec<Entry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub id: String,
    pub path: String,
    pub name: String,
    pub package_version: String,
    pub author: String,
    pub license: String,
    pub description: Option<String>,
    #[serde(default)]
    pub description_locales: BTreeMap<String, String>,
    pub homepage: Option<String>,
    pub digest: String,
    #[serde(default)]
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub modes: Vec<String>,
    #[serde(default)]
    pub previews: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Resource {
    pub path: String,
    pub bytes: usize,
    pub digest: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Selection {
    pub commit: String,
    pub entry: Entry,
}

impl Catalog {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self> {
        let value: Self = serde_json::from_slice(bytes).map_err(|_| Failure::Invalid)?;
        value.validate()?;
        Ok(value)
    }

    pub(super) fn validate(&self) -> Result<()> {
        let mut names = BTreeSet::new();
        if self.version != 1 {
            return Err(Failure::Invalid);
        }
        for entry in &self.themes {
            entry.validate()?;
            if !names.insert(&entry.id) {
                return Err(Failure::Invalid);
            }
        }
        Ok(())
    }
}

impl Entry {
    pub fn description(&self, locale: &str) -> Option<&str> {
        self.description_locales
            .get(locale)
            .map(String::as_str)
            .or(self.description.as_deref())
    }

    pub fn downloadable(&self) -> bool {
        self.validate().is_ok()
    }

    pub(super) fn validate(&self) -> Result<()> {
        if !super::super::package::valid_id(&self.id)
            || self.id == "builtin"
            || self.path != self.id
            || self.name.trim().is_empty()
            || self.author.trim().is_empty()
            || self.license.trim().is_empty()
            || Version::parse(&self.package_version).is_err()
            || !digest(&self.digest)
        {
            return Err(Failure::Invalid);
        }
        if self.homepage.as_ref().is_some_and(|homepage| {
            url::Url::parse(homepage).map_or(true, |url| {
                !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
            })
        }) {
            return Err(Failure::Invalid);
        }
        let mut paths = BTreeSet::new();
        let mut total = 0usize;
        for resource in &self.resources {
            let limit = if resource.path == "theme.json" {
                MANIFEST_BYTES
            } else {
                resource_path(&resource.path)?;
                FILE_BYTES
            };
            total = total.checked_add(resource.bytes).ok_or(Failure::Invalid)?;
            if resource.bytes == 0
                || resource.bytes > limit
                || total > PACKAGE_BYTES
                || !digest(&resource.digest)
                || !paths.insert(&resource.path)
            {
                return Err(Failure::Invalid);
            }
        }
        let modes: BTreeSet<_> = self.modes.iter().map(String::as_str).collect();
        if modes.len() != self.modes.len()
            || modes.iter().any(|mode| !matches!(*mode, "light" | "dark"))
        {
            return Err(Failure::Invalid);
        }
        for (mode, path) in &self.previews {
            if !modes.contains(mode.as_str()) || resource_path(path).is_err() {
                return Err(Failure::Invalid);
            }
        }
        Ok(())
    }
}

pub(super) fn resource_path(path: &str) -> Result<()> {
    super::super::package::market::resource_path(path).map_err(|_| Failure::Invalid)
}

pub(super) fn digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

pub(super) fn commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn bundled() -> Catalog {
    Catalog::parse(include_bytes!("../../../../../themes/catalog.json"))
        .expect("bundled theme index")
}
