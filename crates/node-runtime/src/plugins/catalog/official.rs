//! The official index selects package directories in one authoritative repository.
use super::*;
mod cache;
mod history;
pub(in crate::plugins) use cache::Cache;
use history::{hydrate, package};

pub(super) const REPOSITORY: &str = "https://github.com/sailry/sailry-plugins";
const INDEX: &str = "https://raw.githubusercontent.com/sailry/sailry-plugins/main/catalog.json";

pub(crate) fn owns(info: &Info) -> bool {
    match &info.origin {
        Some(sailry_protocol::plugin::Origin::Bundled) => true,
        Some(sailry_protocol::plugin::Origin::Online { source, path }) => {
            info.skill.is_none()
                && owns_source(&info.summary.name, &source.repository, path)
                && (source.git_ref == "main" || github::source::commit(&source.git_ref))
        }
        _ => false,
    }
}

pub(crate) fn owns_source(package: &str, repository: &str, path: &str) -> bool {
    repository == format!("{REPOSITORY}.git") && path == package
}

#[derive(Deserialize)]
struct Index {
    version: u32,
    packages: Vec<serde_json::Value>,
    #[serde(default)]
    skills: Option<Vec<super::skills::Listing>>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    manifest: serde_json::Value,
    #[serde(default)]
    release_notes: Option<sailry_protocol::plugin::catalog::ReleaseNotes>,
    #[serde(default)]
    versions: Vec<sailry_protocol::plugin::catalog::Version>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Data {
    pub(super) entries: Vec<Entry>,
    pub(super) skills: Vec<sailry_protocol::plugin::skillcatalog::Entry>,
    commit: Option<String>,
}

pub(super) async fn entries(
    host: &Host,
    force: bool,
    stop: &CancellationToken,
) -> Result<Vec<Entry>, Fault> {
    match fetch(host, force, stop).await {
        Ok(entries) => Ok(preferred(entries, bundled::entries()?)),
        // Listings can use shipped packages without trusting invalid online manifests.
        Err(error)
            if !force && (unavailable(&error) || error.code == ErrorCode::InvalidRequest) =>
        {
            bundled::entries()
        }
        Err(error) => Err(error),
    }
}

fn preferred(mut entries: Vec<Entry>, bundled: Vec<Entry>) -> Vec<Entry> {
    for candidate in bundled {
        if let Some(current) = entries.iter_mut().find(|entry| entry.id == candidate.id) {
            let newer = candidate
                .version
                .as_deref()
                .zip(current.version.as_deref())
                .and_then(|(candidate, current)| {
                    semver::Version::parse(candidate)
                        .ok()
                        .zip(semver::Version::parse(current).ok())
                })
                .is_some_and(|(candidate, current)| candidate.cmp_precedence(&current).is_gt());
            if newer {
                *current = candidate;
            }
        } else {
            entries.push(candidate);
        }
    }
    entries
}

pub(super) async fn source(
    host: &Host,
    id: &str,
    stop: &CancellationToken,
) -> Result<Source, Fault> {
    let document = document(host, false, stop).await?;
    let entry = document
        .entries
        .iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "official plugin is unavailable"))?;
    compatibility::ensure(&entry.compatibility)?;
    Ok(Source {
        repository: REPOSITORY.into(),
        git_ref: entry.commit.clone().or(document.commit),
        path: Some(id.into()),
    })
}

pub(in crate::plugins) fn unavailable(error: &Fault) -> bool {
    matches!(error.code, ErrorCode::Unavailable | ErrorCode::NotFound)
}

pub(in crate::plugins) async fn fetch(
    host: &Host,
    force: bool,
    stop: &CancellationToken,
) -> Result<Vec<Entry>, Fault> {
    Ok(document(host, force, stop).await?.entries)
}

pub(super) async fn document(
    host: &Host,
    force: bool,
    stop: &CancellationToken,
) -> Result<Data, Fault> {
    let url = url(host);
    host.official_catalog
        .fetch(url.as_str(), force, stop, || async {
            let commit = resolve(host, stop).await?;
            let index: Index = host.catalog_json(fixed_url(host, &commit), stop).await?;
            let mut document = parse(index)?;
            document.commit = Some(commit.clone());
            let mut selected = Vec::with_capacity(document.entries.len());
            for entry in document.entries {
                match hydrate(host, entry, &commit, stop).await {
                    Ok(entry) => selected.push(entry),
                    Err(_) => tracing::warn!(target: "sailry", area = "plugins", phase = "catalog_entry_skipped"),
                }
            }
            document.entries = selected;
            document.skills.retain(|skill| {
                skill.package.as_ref().is_none_or(|package| document.entries.iter().any(|entry| entry.id == *package))
            });
            for skill in &mut document.skills {
                if !skill.bundled
                    && let Some(sailry_protocol::plugin::skillcatalog::InstallSource::Git(source)) =
                        &mut skill.source
                {
                    source.git_ref = Some(commit.clone());
                }
            }
            Ok(document)
        })
        .await
}

async fn resolve(host: &Host, stop: &CancellationToken) -> Result<String, Fault> {
    let url = commit_url(host);
    let mut file = host
        .github
        .download(url, "application/vnd.github.sha", 1024, stop)
        .await?;
    file.rewind().map_err(io_error)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(io_error)?;
    let commit = std::str::from_utf8(&bytes)
        .map_err(|_| Fault::invalid("invalid official catalog commit"))?
        .trim();
    if !github::source::commit(commit) {
        return Err(Fault::invalid("invalid official catalog commit"));
    }
    Ok(commit.into())
}

fn commit_url(_host: &Host) -> url::Url {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(endpoint) = _host
        .github
        .official_endpoint
        .as_ref()
        .or(_host.github.endpoint.as_ref())
    {
        return endpoint
            .join("repos/sailry/sailry-plugins/commits/main")
            .unwrap();
    }
    url::Url::parse("https://api.github.com/repos/sailry/sailry-plugins/commits/main").unwrap()
}

fn fixed_url(_host: &Host, commit: &str) -> url::Url {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(endpoint) = _host
        .github
        .official_endpoint
        .as_ref()
        .or(_host.github.endpoint.as_ref())
    {
        return endpoint.join(&format!("{commit}/catalog.json")).unwrap();
    }
    url::Url::parse(&format!(
        "https://raw.githubusercontent.com/sailry/sailry-plugins/{commit}/catalog.json"
    ))
    .unwrap()
}

fn url(_host: &Host) -> url::Url {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(endpoint) = _host
        .github
        .official_endpoint
        .as_ref()
        .or(_host.github.endpoint.as_ref())
    {
        return endpoint.join("catalog.json").unwrap();
    }
    url::Url::parse(INDEX).unwrap()
}

fn parse(index: Index) -> Result<Data, Fault> {
    if index.version != 1 || index.packages.len() > 512 {
        return Err(Fault::invalid("unsupported official plugin catalog"));
    }
    let icons = bundled::entries()?;
    let mut entries: Vec<Entry> = Vec::with_capacity(index.packages.len());
    let mut rejected = std::collections::BTreeSet::new();
    for (position, value) in index.packages.into_iter().enumerate() {
        let names: Vec<String> = [
            value.get("id"),
            value
                .get("manifest")
                .and_then(|manifest| manifest.get("name")),
        ]
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect();
        match package(value, &icons) {
            Ok(entry) if !entries.iter().any(|current| current.id == entry.id) => {
                entries.push(entry)
            }
            _ => {
                rejected.extend(names);
                tracing::warn!(target: "sailry", area = "plugins", phase = "catalog_entry_skipped", entry_index = position as u64);
            }
        }
    }
    let skills = match index.skills {
        Some(skills) => {
            let mut names = std::collections::BTreeSet::new();
            let mut selected = vec![];
            for listing in skills {
                if !entries.iter().any(|entry| entry.id == listing.package) {
                    if rejected.contains(&listing.package) {
                        continue;
                    }
                    return Err(Fault::invalid("official skill package is missing"));
                }
                let name = listing
                    .path
                    .strip_prefix("skills/")
                    .and_then(|path| path.strip_suffix("/SKILL.md"))
                    .filter(|name| !name.contains('/'))
                    .ok_or_else(|| Fault::invalid("invalid official skill path"))?;
                let skill = super::super::skills::parse(name, &listing.frontmatter)?;
                let entry = super::skills::entry(&listing.package, skill, false)?;
                if !names.insert(entry.id.clone()) {
                    return Err(Fault::invalid("duplicate official skill"));
                }
                selected.push(entry);
            }
            selected
        }
        None => bundled::skills()?
            .into_iter()
            .filter(|skill| {
                skill
                    .package
                    .as_ref()
                    .is_none_or(|package| entries.iter().any(|entry| entry.id == *package))
            })
            .collect(),
    };
    Ok(Data {
        entries,
        skills,
        commit: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Index {
        let index: Index =
            serde_json::from_str(include_str!("../../../../../plugins/catalog.json")).unwrap();
        for package in &index.packages {
            let manifest =
                manifest::parse(&serde_json::to_vec(&package["manifest"]).unwrap()).unwrap();
            assert!(
                manifest.issues.is_empty(),
                "{}: {:?}",
                package["id"],
                manifest.issues
            );
        }
        index
    }

    #[test]
    fn chooses_whole_entries_by_precedence() {
        let bundled = bundled::entries().unwrap().remove(0);
        let mut remote = bundled.clone();
        remote.bundled = false;
        remote.repository = Some(REPOSITORY.into());
        remote.category = None;
        remote.version = Some("0.0.1".into());
        assert_eq!(
            preferred(vec![remote.clone()], vec![bundled.clone()]).as_slice(),
            std::slice::from_ref(&bundled)
        );
        for version in [bundled.version.clone().unwrap(), "99.0.0".into()] {
            remote.version = Some(version);
            assert_eq!(
                preferred(vec![remote.clone()], vec![bundled.clone()]).as_slice(),
                std::slice::from_ref(&remote)
            );
        }
        assert_eq!(preferred(vec![], vec![bundled.clone()]), [bundled]);
    }

    #[test]
    fn preserves_metadata() {
        let entries = parse(index()).unwrap().entries;
        let bundled = bundled::entries().unwrap();
        assert_eq!(entries.len(), bundled.len());
        for (mut entry, expected) in entries.into_iter().zip(bundled) {
            assert!(!entry.bundled);
            assert_eq!(entry.repository.as_deref(), Some(REPOSITORY));
            entry.repository = None;
            entry.versions.clear();
            entry.bundled = true;
            assert_eq!(entry, expected);
        }
    }

    #[test]
    fn rejects_invalid_indexes() {
        let mut wrong = index();
        wrong.version = 2;
        assert!(parse(wrong).is_err());
        let mut duplicate = index();
        duplicate.packages.push(duplicate.packages[0].clone());
        assert_eq!(
            parse(duplicate).unwrap().entries.len(),
            index().packages.len()
        );
        for id in ["../private", "files/other", "different-name"] {
            let mut wrong = index();
            wrong.packages[0]["id"] = id.into();
            assert_eq!(
                parse(wrong).unwrap().entries.len(),
                index().packages.len() - 1
            );
        }
    }

    #[test]
    fn skips_invalid_package_metadata() {
        let expected = index().packages.len() - 1;
        for value in [
            serde_json::json!({"text":""}),
            serde_json::json!({"text":4}),
        ] {
            let mut invalid = index();
            invalid.packages[0]["release_notes"] = value;
            assert_eq!(parse(invalid).unwrap().entries.len(), expected);
        }
        let mut invalid = index();
        invalid.packages[0]["manifest"]["extensions"]["dev.sailry.platform"]["min_sailry_version"] =
            ">=0.1.0".into();
        assert_eq!(parse(invalid).unwrap().entries.len(), expected);
    }

    #[test]
    fn distinguishes_missing_and_empty_skill_inventories() {
        let mut old = index();
        old.skills = None;
        assert_eq!(parse(old).unwrap().skills, bundled::skills().unwrap());
        let mut empty = index();
        empty.skills = Some(vec![]);
        assert!(parse(empty).unwrap().skills.is_empty());
        let mut invalid = index();
        invalid.skills = Some(vec![super::super::skills::Listing {
            package: "../private".into(),
            path: "skills/research/SKILL.md".into(),
            frontmatter: "---\nname: research\ndescription: Research\n---\n".into(),
        }]);
        assert!(parse(invalid).is_err());
    }
}
