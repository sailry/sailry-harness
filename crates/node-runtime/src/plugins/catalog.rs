//! Official repository packages and the public pluginsmp.com Agent Plugins directory.
use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{
    catalog::{Details, Entry, Page, Source as CatalogSource},
    skills::Source,
};
use serde::Deserialize;
use std::io::{Read, Seek};
pub(crate) mod official;
pub(super) mod skills;

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
}

impl Host {
    pub(crate) async fn catalog(
        &self,
        source: CatalogSource,
        query: &str,
        page: u32,
        force: bool,
        stop: CancellationToken,
    ) -> Result<Page, Fault> {
        if page == 0 {
            return Err(sailry_protocol::Fault::invalid("invalid catalog query"));
        }
        if source == CatalogSource::Official {
            let mut entries = official::entries(self, force, &stop).await?;
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
            .ok_or_else(|| sailry_protocol::Fault::invalid("catalog pagination is missing"))?;
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
                return Err(sailry_protocol::Fault::invalid("invalid catalog entry"));
            }
            github::source::Selection::parse(&Source {
                repository: item.repo_url.clone(),
                git_ref: None,
                path: None,
            })?;
            entries.push(Entry {
                compatibility: Default::default(),
                commit: None,
                versions: Vec::new(),
                category: None,
                tint: None,
                release_notes: None,
                id: item.slug,
                name: item.name,
                version: None,
                description: item.description,
                description_locales: Default::default(),
                display: None,
                icon: None,
                repository: Some(item.repo_url),
                bundled: false,
                provides_models: false,
                permissions: Vec::new(),
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
        catalog: CatalogSource,
        id: &str,
        stop: CancellationToken,
    ) -> Result<Source, Fault> {
        if !sailry_protocol::plugin::ui::identifier(id) {
            return Err(sailry_protocol::Fault::invalid(
                "invalid catalog identifier",
            ));
        }
        if catalog == CatalogSource::Official {
            return official::source(self, id, &stop).await;
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
        bundled: bool,
        stop: CancellationToken,
    ) -> Result<Details, Fault> {
        if !sailry_protocol::plugin::ui::identifier(id) {
            return Err(sailry_protocol::Fault::invalid(
                "invalid catalog identifier",
            ));
        }
        if bundled {
            if source != CatalogSource::Official {
                return Err(sailry_protocol::Fault::invalid(
                    "third-party catalog cannot select bundled packages",
                ));
            }
            let host = self.clone();
            let id = id.to_owned();
            return tokio::task::spawn_blocking(move || host.inspect_bundled(&id, stop))
                .await
                .map_err(|_| github::unavailable())?
                .map(|info| Details::Plugin(Box::new(info)));
        }
        if source == CatalogSource::ThirdParty {
            let selected = self.catalog_source(source, id, stop.clone()).await?;
            self.inspect_catalog_source(&selected, stop).await
        } else {
            let document = official::document(self, false, &stop).await?;
            let entry = document
                .entries
                .iter()
                .find(|entry| entry.id == id)
                .ok_or_else(|| Fault::new(ErrorCode::NotFound, "official plugin is unavailable"))?;
            // Details remain readable when every published version requires a newer Node.
            let selected = Source {
                repository: official::REPOSITORY.into(),
                git_ref: entry.commit.clone(),
                path: Some(id.into()),
            };
            let mut info = self.inspect_source(&selected, stop).await?.info;
            if info.summary.version != entry.version {
                return Err(Fault::invalid(
                    "official package version does not match its catalog",
                ));
            }
            info.versions = entry.versions.clone();
            Ok(Details::Plugin(Box::new(info)))
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
        serde_json::from_slice(&bytes)
            .map_err(|_| sailry_protocol::Fault::invalid("invalid plugin catalog response"))
    }
}

pub(super) fn entry(id: &str, manifest: manifest::Manifest) -> Entry {
    let provides_models = manifest
        .extension
        .as_ref()
        .is_some_and(|extension| !extension.model_providers.is_empty());
    let permissions = manifest
        .extension
        .as_ref()
        .map_or_else(Vec::new, |extension| extension.actions.clone());
    Entry {
        compatibility: compatibility::extension(manifest.extension.as_ref()),
        commit: None,
        versions: Vec::new(),
        category: manifest
            .extension
            .as_ref()
            .and_then(|extension| extension.category),
        tint: manifest
            .extension
            .as_ref()
            .and_then(|extension| extension.tint),
        release_notes: None,
        provides_models,
        permissions,
        id: id.to_owned(),
        name: manifest.name,
        version: manifest.version,
        description: manifest
            .extension
            .as_ref()
            .and_then(|extension| extension.description.as_ref())
            .map(|description| description.label.clone())
            .or(manifest.description),
        description_locales: manifest
            .extension
            .as_ref()
            .and_then(|extension| extension.description.as_ref())
            .map(|description| description.locales.clone())
            .unwrap_or_default(),
        display: manifest.extension.and_then(|extension| {
            extension
                .display
                .or_else(|| extension.desktop.and_then(|desktop| desktop.navigation))
        }),
        icon: None,
        repository: None,
        bundled: false,
    }
}

fn source(detail: Detail) -> Result<Source, Fault> {
    let root_manifest = detail.manifests.get("agent-plugins");
    if let Some(manifest) = root_manifest {
        if manifest.path != "plugin.json"
            && manifest.path != format!("{}/plugin.json", detail.plugin_path.trim_end_matches('/'))
        {
            return Err(Fault::invalid(
                "portable plugin manifest must be at the package root",
            ));
        }
    } else if ![
        ("codex", ".codex-plugin/plugin.json"),
        ("claude-code", ".claude-plugin/plugin.json"),
    ]
    .iter()
    .any(|(format, path)| {
        detail.manifests.get(*format).is_some_and(|manifest| {
            manifest.path == *path
                || manifest.path == format!("{}/{path}", detail.plugin_path.trim_end_matches('/'))
        })
    }) {
        return Err(Fault::invalid(
            "supported repository plugin metadata is unavailable",
        ));
    }
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
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut host = Host::new(None);
        host.github = github::Github::catalog_fixture("http://127.0.0.1:9/");
        let page = host
            .catalog(
                CatalogSource::Official,
                "",
                1,
                false,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!page.unavailable);
        assert!(
            page.entries
                .iter()
                .all(|entry| entry.bundled && entry.repository.is_none())
        );
        assert_eq!(page.entries, bundled::entries().unwrap());
        for (query, expected) in [
            ("读取、编辑与管理文件", vec!["files"]),
            ("筑梦大亨", vec!["city-trader"]),
        ] {
            let page = host
                .catalog(
                    CatalogSource::Official,
                    query,
                    1,
                    false,
                    CancellationToken::new(),
                )
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
                false,
                CancellationToken::new()
            )
            .await
            .unwrap()
            .entries
            .is_empty()
        );
    }

    fn detail(path: &str) -> Detail {
        Detail {
            repo_url: "https://github.com/example/plugins".into(),
            plugin_path: "plugins/review".into(),
            manifests: [("agent-plugins".into(), Manifest { path: path.into() })].into(),
        }
    }
    #[test]
    fn accepts_only_portable_package_roots() {
        for path in ["plugin.json", "plugins/review/plugin.json"] {
            assert_eq!(
                source(detail(path)).unwrap().path.as_deref(),
                Some("plugins/review")
            );
        }
        assert!(source(detail(".claude-plugin/plugin.json")).is_err());
        let mut unsafe_path = detail("plugin.json");
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
            .catalog(
                CatalogSource::ThirdParty,
                "context7",
                1,
                false,
                stop.clone(),
            )
            .await
            .unwrap();
        let entries: Vec<_> = page
            .entries
            .iter()
            .filter(|entry| entry.name == "context7")
            .collect();
        assert!(!entries.is_empty());
        for entry in entries {
            let source = host
                .catalog_source(CatalogSource::ThirdParty, &entry.id, stop.clone())
                .await
                .unwrap();
            let selected = host.inspect_source(&source, stop.clone()).await.unwrap();
            let info = selected.info;
            assert_eq!(info.summary.name, entry.name);
            assert!(!info.skills.is_empty() || !info.mcp.is_empty());
            assert!(info.issues.is_empty());
            let installed = host
                .install_source(
                    &selected.source,
                    &selected.path,
                    &entry.name,
                    stop.clone(),
                    None,
                )
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
            .catalog(
                CatalogSource::ThirdParty,
                "standards",
                1,
                false,
                stop.clone(),
            )
            .await
            .unwrap();
        assert!(!page.unavailable);
        let entry = page
            .entries
            .iter()
            .find(|entry| !entry.bundled)
            .expect("portable public catalog entry");
        let source = host
            .catalog_source(CatalogSource::ThirdParty, &entry.id, stop.clone())
            .await
            .unwrap();
        let selected = host.inspect_source(&source, stop).await.unwrap();
        assert_eq!(selected.info.summary.name, entry.name);
        assert!(!selected.info.summary.digest.is_empty());
    }
}
