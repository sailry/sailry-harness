use super::{METADATA_BYTES, commit, http::Http};
use sailry_link::CancellationToken;
use sailry_protocol::{Fault, models::Version};
use std::collections::BTreeMap;

pub(super) struct Refs {
    heads: BTreeMap<String, String>,
    tags: BTreeMap<String, String>,
}

impl Refs {
    pub async fn read(
        http: &Http,
        repository: &str,
        cancel: &CancellationToken,
    ) -> Result<Self, Fault> {
        let mut url = http.repository(&[], &format!("{repository}.git"), &["info", "refs"]);
        url.query_pairs_mut()
            .append_pair("service", "git-upload-pack");
        let response = http.get(url, METADATA_BYTES, cancel).await?;
        Self::parse(&response.body)
    }

    pub(super) fn parse(mut body: &[u8]) -> Result<Self, Fault> {
        let mut heads = BTreeMap::new();
        let mut tags = BTreeMap::new();
        let mut peeled = BTreeMap::new();
        let mut service = false;
        while !body.is_empty() {
            let length = body
                .get(..4)
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .and_then(|value| usize::from_str_radix(value, 16).ok())
                .ok_or_else(invalid)?;
            body = body.get(4..).ok_or_else(invalid)?;
            if length == 0 {
                continue;
            }
            let length = length.checked_sub(4).ok_or_else(invalid)?;
            let line = body.get(..length).ok_or_else(invalid)?;
            body = body.get(length..).ok_or_else(invalid)?;
            let line = std::str::from_utf8(line).map_err(|_| invalid())?;
            if !service {
                if line != "# service=git-upload-pack\n" {
                    return Err(invalid());
                }
                service = true;
                continue;
            }
            if line == "version 1\n" {
                continue;
            }
            let line = line.split('\0').next().unwrap_or_default().trim_end();
            let (revision, name) = line.split_once(' ').ok_or_else(invalid)?;
            if !commit(revision) {
                return Err(invalid());
            }
            let revision = revision.to_ascii_lowercase();
            if let Some(name) = name.strip_prefix("refs/heads/") {
                insert(&mut heads, name, revision)?;
            } else if let Some(name) = name.strip_prefix("refs/tags/") {
                if let Some(name) = name.strip_suffix("^{}") {
                    insert(&mut peeled, name, revision)?;
                } else {
                    insert(&mut tags, name, revision)?;
                }
            } else if name != "HEAD" {
                continue;
            }
        }
        if !service || heads.is_empty() && tags.is_empty() {
            return Err(invalid());
        }
        for (name, revision) in peeled {
            if !tags.contains_key(&name) {
                return Err(invalid());
            }
            tags.insert(name, revision);
        }
        Ok(Self { heads, tags })
    }

    pub fn resolve(&self, name: &str) -> Option<&str> {
        if let Some(name) = name.strip_prefix("refs/heads/") {
            return self.heads.get(name).map(String::as_str);
        }
        if let Some(name) = name.strip_prefix("refs/tags/") {
            return self.tags.get(name).map(String::as_str);
        }
        match (self.heads.get(name), self.tags.get(name)) {
            (Some(head), Some(tag)) if head != tag => None,
            (Some(revision), _) | (_, Some(revision)) => Some(revision),
            _ => None,
        }
    }

    pub fn versions(&self) -> Vec<Version> {
        self.heads
            .iter()
            .chain(&self.tags)
            .map(|(name, revision)| Version {
                name: name.clone(),
                revision: revision.clone(),
            })
            .collect()
    }
}

fn insert(map: &mut BTreeMap<String, String>, name: &str, revision: String) -> Result<(), Fault> {
    if name.is_empty()
        || name.len() > 256
        || name.contains('\0')
        || map.insert(name.into(), revision).is_some()
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> Fault {
    Fault::invalid("ModelScope did not return a Git ref advertisement")
}
