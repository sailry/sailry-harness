//! The official index selects package directories in one authoritative repository.
use super::*;
mod cache;
pub(in crate::plugins) use cache::Cache;

const REPOSITORY: &str = "https://github.com/sailry/sailry-plugins";
const INDEX: &str = "https://raw.githubusercontent.com/sailry/sailry-plugins/main/catalog.json";

pub(in crate::plugins) fn owns(info: &Info) -> bool {
    match &info.origin {
        Some(sailry_protocol::plugin::Origin::Bundled) => true,
        Some(sailry_protocol::plugin::Origin::Online { source, path }) => {
            info.skill.is_none()
                && source.repository == format!("{REPOSITORY}.git")
                && source.git_ref == "main"
                && path == &info.summary.name
        }
        _ => false,
    }
}

#[derive(Deserialize)]
struct Index {
    version: u32,
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    manifest: serde_json::Value,
}

pub(super) async fn entries(
    host: &Host,
    force: bool,
    stop: &CancellationToken,
) -> Result<Vec<Entry>, Fault> {
    match fetch(host, force, stop).await {
        Ok(entries) => Ok(entries),
        // Listings can use shipped packages without trusting invalid online manifests.
        Err(error)
            if !force && (unavailable(&error) || error.code == ErrorCode::InvalidRequest) =>
        {
            bundled::entries()
        }
        Err(error) => Err(error),
    }
}

pub(super) async fn source(
    host: &Host,
    id: &str,
    stop: &CancellationToken,
) -> Result<Source, Fault> {
    let entries = fetch(host, false, stop).await?;
    if !entries.iter().any(|entry| entry.id == id) {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "official plugin is unavailable",
        ));
    }
    Ok(Source {
        repository: REPOSITORY.into(),
        git_ref: Some("main".into()),
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
    let url = url(host);
    host.official_catalog
        .fetch(url.as_str(), force, stop, || async {
            let _permit = host
                .reads
                .clone()
                .try_acquire_owned()
                .map_err(|_| Fault::new(ErrorCode::Busy, "plugin catalog is busy"))?;
            let index: Index = host.catalog_json(url.clone(), stop).await?;
            parse(index)
        })
        .await
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

fn parse(index: Index) -> Result<Vec<Entry>, Fault> {
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
        entry.icon = icons
            .iter()
            .find(|icon| icon.id == entry.id)
            .and_then(|icon| icon.icon.clone());
        entry.repository = Some(REPOSITORY.into());
        entries.push(entry);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Index {
        serde_json::from_str(include_str!("../../../../../plugins/catalog.json")).unwrap()
    }

    #[test]
    fn preserves_metadata() {
        let entries = parse(index()).unwrap();
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
}
