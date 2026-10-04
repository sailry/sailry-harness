//! Offline official packages and the public pluginsmp.com Agent Plugins directory.
use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{
    catalog::{Entry, Page, Source as CatalogSource},
    skills::Source,
};
use serde::Deserialize;
use std::io::{Read, Seek};

#[derive(Deserialize)]
struct Response<T> {
    data: T,
    #[serde(default)]
    meta: Option<Meta>,
}
#[derive(Deserialize)]
struct Meta {
    page: u32,
    total_pages: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listing {
    slug: String,
    name: String,
    description: Option<String>,
    repo_url: String,
    protocols: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Detail {
    repo_url: String,
    plugin_path: String,
    manifests: std::collections::BTreeMap<String, Manifest>,
}
#[derive(Deserialize)]
struct Manifest {
    path: String,
    raw: String,
}

impl Host {
    pub(crate) async fn catalog(
        &self,
        source: CatalogSource,
        query: &str,
        page: u32,
        stop: CancellationToken,
    ) -> Result<Page, Fault> {
        if query.len() > 200 || page == 0 {
            return Err(invalid("invalid catalog query"));
        }
        if source == CatalogSource::Official {
            let mut entries = bundled::entries()?;
            let query = query.to_lowercase();
            entries.retain(|entry| {
                entry.name.to_lowercase().contains(&query)
                    || entry
                        .description
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(&query)
                    || entry
                        .description_locales
                        .values()
                        .any(|description| description.to_lowercase().contains(&query))
                    || entry.display.as_ref().is_some_and(|display| {
                        std::iter::once(&display.label)
                            .chain(display.locales.values())
                            .any(|label| label.to_lowercase().contains(&query))
                    })
            });
            return Ok(Page {
                entries,
                page: 1,
                pages: 1,
                unavailable: false,
            });
        }
        let _permit = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "plugin catalog is busy"))?;
        let mut url = self.catalog_url("api/v1/plugins");
        url.query_pairs_mut()
            .append_pair("protocol", "agent-plugins")
            .append_pair("q", query)
            .append_pair("page", &page.to_string())
            .append_pair("per_page", "12");
        let response: Result<Response<Vec<Listing>>, Fault> = self.catalog_json(url, &stop).await;
        let mut entries = Vec::new();
        let response = response?;
        let meta = response
            .meta
            .ok_or_else(|| invalid("catalog pagination is missing"))?;
        for item in response.data {
            if !item
                .protocols
                .iter()
                .any(|protocol| protocol == "agent-plugins")
            {
                continue;
            }
            if !sailry_protocol::plugin::ui::identifier(&item.slug)
                || item.name.len() > 256
                || item
                    .description
                    .as_ref()
                    .is_some_and(|text| text.len() > 8192)
            {
                return Err(invalid("invalid catalog entry"));
            }
            github::source::Selection::parse(&Source {
                repository: item.repo_url.clone(),
                git_ref: None,
                path: None,
            })?;
            entries.push(Entry {
                id: item.slug,
                name: item.name,
                description: item.description,
                description_locales: Default::default(),
                display: None,
                icon: None,
                repository: Some(item.repo_url),
                bundled: false,
            });
        }
        Ok(Page {
            entries,
            page: meta.page,
            pages: meta.total_pages,
            unavailable: false,
        })
    }

    pub(crate) async fn catalog_source(
        &self,
        id: &str,
        stop: CancellationToken,
    ) -> Result<Source, Fault> {
        if !sailry_protocol::plugin::ui::identifier(id) {
            return Err(invalid("invalid catalog identifier"));
        }
        let response: Response<Detail> = self
            .catalog_json(self.catalog_url(&format!("api/v1/plugins/{id}")), &stop)
            .await?;
        source(response.data)
    }

    pub(crate) async fn catalog_info(
        &self,
        source: CatalogSource,
        id: &str,
        stop: CancellationToken,
    ) -> Result<Info, Fault> {
        if !sailry_protocol::plugin::ui::identifier(id) {
            return Err(invalid("invalid catalog identifier"));
        }
        match source {
            CatalogSource::Official => {
                let _permit = self
                    .reads
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| Fault::new(ErrorCode::Busy, "plugin inspection is busy"))?;
                let host = self.clone();
                let id = id.to_owned();
                tokio::task::spawn_blocking(move || host.inspect_bundled(&id, stop))
                    .await
                    .map_err(|_| github::unavailable())?
            }
            CatalogSource::ThirdParty => {
                let source = self.catalog_source(id, stop.clone()).await?;
                Ok(self.inspect_source(&source, stop).await?.info)
            }
        }
    }

    fn catalog_url(&self, path: &str) -> url::Url {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(endpoint) = &self.github.endpoint {
            return endpoint.join(path).unwrap();
        }
        url::Url::parse("https://pluginsmp.com/")
            .unwrap()
            .join(path)
            .unwrap()
    }

    async fn catalog_json<T: serde::de::DeserializeOwned>(
        &self,
        url: url::Url,
        stop: &CancellationToken,
    ) -> Result<T, Fault> {
        let mut file = self
            .github
            .download(url, "application/json", 2 * 1024 * 1024, stop)
            .await?;
        file.rewind().map_err(io_error)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(io_error)?;
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid plugin catalog response"))
    }
}

fn source(detail: Detail) -> Result<Source, Fault> {
    let manifest = detail
        .manifests
        .get("agent-plugins")
        .ok_or_else(|| invalid("Agent Plugins manifest is unavailable"))?;
    if manifest.path != "plugin.json"
        && manifest.path != format!("{}/plugin.json", detail.plugin_path.trim_end_matches('/'))
    {
        return Err(invalid(
            "portable plugin manifest must be at the package root",
        ));
    }
    manifest::parse(manifest.raw.as_bytes())?;
    let source = Source {
        repository: detail.repo_url,
        git_ref: None,
        path: Some(detail.plugin_path),
    };
    github::source::Selection::parse(&source)?;
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn official_is_offline_and_searches_localized_titles() {
        let host = Host::new(None);
        let page = host
            .catalog(CatalogSource::Official, "", 1, CancellationToken::new())
            .await
            .unwrap();
        assert!(!page.unavailable);
        assert!(
            page.entries
                .iter()
                .all(|entry| entry.bundled && entry.repository.is_none())
        );
        assert_eq!(page.entries, bundled::entries().unwrap());
        for (query, expected) in [("文件", vec!["files"]), ("筑梦大亨", vec!["city-trader"])]
        {
            let page = host
                .catalog(CatalogSource::Official, query, 1, CancellationToken::new())
                .await
                .unwrap();
            let mut names: Vec<_> = page
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            names.sort();
            assert_eq!(names, expected);
        }
        assert!(
            host.catalog(
                CatalogSource::Official,
                "no-such-plugin",
                1,
                CancellationToken::new()
            )
            .await
            .unwrap()
            .entries
            .is_empty()
        );
    }

    fn detail(path: &str, schema: &str) -> Detail {
        Detail {
            repo_url: "https://github.com/example/plugins".into(),
            plugin_path: "plugins/review".into(),
            manifests: [(
                "agent-plugins".into(),
                Manifest {
                    path: path.into(),
                    raw: serde_json::json!({"$schema":schema, "name":"review"}).to_string(),
                },
            )]
            .into(),
        }
    }
    #[test]
    fn accepts_only_portable_package_roots() {
        for path in ["plugin.json", "plugins/review/plugin.json"] {
            assert_eq!(
                source(detail(path, manifest::SCHEMA))
                    .unwrap()
                    .path
                    .as_deref(),
                Some("plugins/review")
            );
        }
        assert!(source(detail(".claude-plugin/plugin.json", manifest::SCHEMA)).is_err());
        assert!(source(detail("plugin.json", "unknown")).is_err());
        let mut unsafe_path = detail("plugin.json", manifest::SCHEMA);
        unsafe_path.plugin_path = "../private".into();
        assert!(source(unsafe_path).is_err());
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires the public plugin catalog and GitHub"]
    async fn context7_packages() {
        let profile = tempfile::tempdir().unwrap();
        let host = Host::new(Some(profile.path().canonicalize().unwrap()));
        let stop = CancellationToken::new();
        let page = host
            .catalog(CatalogSource::ThirdParty, "context7", 1, stop.clone())
            .await
            .unwrap();
        let entries: Vec<_> = page
            .entries
            .iter()
            .filter(|entry| entry.name == "context7")
            .collect();
        assert!(!entries.is_empty());
        for entry in entries {
            let source = host.catalog_source(&entry.id, stop.clone()).await.unwrap();
            let selected = host.inspect_source(&source, stop.clone()).await.unwrap();
            let info = selected.info;
            assert_eq!(info.summary.name, entry.name);
            assert!(!info.skills.is_empty() || !info.mcp.is_empty());
            assert!(info.issues.is_empty());
            let installed = host
                .install_source(&selected.source, &selected.path, &entry.name)
                .await
                .unwrap();
            assert_eq!(installed, info);
            println!(
                "{}: installed {} at {} ({} skills, {} MCP servers)",
                entry.id,
                entry.repository.as_deref().unwrap(),
                selected.source.commit,
                installed.skills.len(),
                installed.mcp.len()
            );
        }
    }
    #[tokio::test]
    #[ignore = "requires the public plugin catalog and GitHub"]
    async fn public_directory() {
        let host = Host::new(None);
        let stop = CancellationToken::new();
        let page = host
            .catalog(CatalogSource::ThirdParty, "standards", 1, stop.clone())
            .await
            .unwrap();
        assert!(!page.unavailable);
        let entry = page
            .entries
            .iter()
            .find(|entry| !entry.bundled)
            .expect("portable public catalog entry");
        let source = host.catalog_source(&entry.id, stop.clone()).await.unwrap();
        let selected = host.inspect_source(&source, stop).await.unwrap();
        assert_eq!(selected.info.summary.name, entry.name);
        assert!(!selected.info.summary.digest.is_empty());
    }
}
