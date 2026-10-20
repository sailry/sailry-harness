//! The official index selects package directories in one authoritative repository.
use super::*;
mod cache;
pub(in crate::plugins) use cache::Cache;

pub(super) const REPOSITORY: &str = "https://github.com/sailry/sailry-plugins";
const INDEX: &str = "https://raw.githubusercontent.com/sailry/sailry-plugins/main/catalog.json";

pub(in crate::plugins) fn owns(info: &Info) -> bool {
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
    packages: Vec<Package>,
    #[serde(default)]
    skills: Option<Vec<super::skills::Listing>>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    manifest: serde_json::Value,
    #[serde(default)]
    release_notes: Option<sailry_protocol::plugin::catalog::ReleaseNotes>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Data {
    entries: Vec<Entry>,
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
    if !document.entries.iter().any(|entry| entry.id == id) {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "official plugin is unavailable",
        ));
    }
    Ok(Source {
        repository: REPOSITORY.into(),
        git_ref: document.commit,
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
            let _permit = host
                .reads
                .clone()
                .try_acquire_owned()
                .map_err(|_| Fault::new(ErrorCode::Busy, "plugin catalog is busy"))?;
            let commit = resolve(host, stop).await?;
            let index: Index = host.catalog_json(fixed_url(host, &commit), stop).await?;
            let mut document = parse(index)?;
            document.commit = Some(commit.clone());
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
        return Err(sailry_protocol::Fault::invalid(
            "unsupported official plugin catalog",
        ));
    }
    let icons = bundled::entries()?;
    let mut entries: Vec<Entry> = Vec::with_capacity(index.packages.len());
    for package in index.packages {
        validate_name(&package.id)?;
        let bytes = serde_json::to_vec(&package.manifest)
            .map_err(|_| sailry_protocol::Fault::invalid("invalid official plugin manifest"))?;
        if bytes.len() > 64 * 1024 {
            return Err(sailry_protocol::Fault::invalid(
                "official plugin manifest exceeds its size limit",
            ));
        }
        let manifest = manifest::parse(&bytes)?;
        if manifest.name != package.id
            || !manifest.issues.is_empty()
            || entries.iter().any(|entry| entry.id == package.id)
        {
            return Err(sailry_protocol::Fault::invalid(
                "invalid official plugin identity or manifest",
            ));
        }
        let mut entry = super::entry(&package.id, manifest);
        if package
            .release_notes
            .as_ref()
            .is_some_and(|notes| !notes.valid())
        {
            return Err(Fault::invalid("invalid official release notes"));
        }
        entry.release_notes = package.release_notes;
        entry.icon = icons
            .iter()
            .find(|icon| icon.id == entry.id)
            .and_then(|icon| icon.icon.clone());
        entry.repository = Some(REPOSITORY.into());
        entries.push(entry);
    }
    let skills = match index.skills {
        Some(skills) => {
            let mut names = std::collections::BTreeSet::new();
            let mut selected = vec![];
            for listing in skills {
                if !entries.iter().any(|entry| entry.id == listing.package) {
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
        None => bundled::skills()?,
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
                manifest::parse(&serde_json::to_vec(&package.manifest).unwrap()).unwrap();
            assert!(
                manifest.issues.is_empty(),
                "{}: {:?}",
                package.id,
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
        duplicate.packages[1].id = duplicate.packages[0].id.clone();
        duplicate.packages[1].manifest = duplicate.packages[0].manifest.clone();
        assert!(parse(duplicate).is_err());
        for id in ["../private", "files/other", "different-name"] {
            let mut wrong = index();
            wrong.packages[0].id = id.into();
            assert!(parse(wrong).is_err());
        }
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
