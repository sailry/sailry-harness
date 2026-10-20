//! Public directory pages do not substitute for source inspection or installation.
use super::*;
use sailry_protocol::plugin::{
    skillcatalog::{Entry, InstallSource, Page, QueryScope, Source as DirectorySource},
    skills::RegistrySource,
};
use std::time::Duration;

const PAGE_SIZE: usize = 24;
const SEARCH_SIZE: usize = 100;

#[derive(Default)]
pub(in crate::plugins) struct Directory(super::super::super::query::Cache<[u8; 32], Page>);

#[derive(Deserialize)]
struct DirectoryPage {
    skills: Vec<DirectorySkill>,
    total: Option<usize>,
    #[serde(rename = "hasMore", default)]
    has_more: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectorySkill {
    source: String,
    skill_id: String,
    name: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct HubResponse {
    code: i64,
    data: HubPage,
}
#[derive(Deserialize)]
struct HubPage {
    skills: Vec<HubSkill>,
    total: usize,
}
#[derive(Deserialize)]
struct HubSkill {
    #[serde(rename = "iconUrl", default)]
    icon: Option<String>,
    slug: String,
    name: String,
    description: Option<String>,
    description_zh: Option<String>,
    namespace: HubNamespace,
    version: Option<String>,
}
#[derive(Deserialize)]
struct HubNamespace {
    handle: String,
}

fn segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn icon(value: Option<String>) -> Option<String> {
    value.filter(|value| {
        url::Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
    })
}

fn directory_entry(skill: DirectorySkill) -> Result<Entry, Fault> {
    if skill.source.trim().is_empty()
        || skill.source.chars().any(char::is_control)
        || !segment(&skill.skill_id)
        || skill.name.trim().is_empty()
    {
        return Err(Fault::invalid("invalid skills.sh entry"));
    }
    let parts = skill.source.split('/').collect::<Vec<_>>();
    let url = parts.iter().all(|part| segment(part)).then(|| {
        let mut url = url::Url::parse("https://skills.sh/").unwrap();
        url.path_segments_mut()
            .unwrap()
            .clear()
            .extend(&parts)
            .push(&skill.skill_id);
        url.to_string()
    });
    let source = (parts.len() == 2 && parts.iter().all(|part| segment(part))).then(|| {
        InstallSource::Git(Source {
            repository: url.clone().unwrap(),
            git_ref: None,
            path: None,
        })
    });
    Ok(Entry {
        id: format!("{}/{}", skill.source, skill.skill_id),
        package: None,
        name: skill.skill_id,
        display_name: Some(skill.name),
        icon: None,
        display_locales: Default::default(),
        description: skill.description,
        description_locales: Default::default(),
        origin: skill.source,
        url,
        source,
        bundled: false,
    })
}

impl Host {
    pub(super) async fn external_skills(
        &self,
        source: DirectorySource,
        query: &str,
        page: u32,
        force: bool,
        stop: CancellationToken,
    ) -> Result<Page, Fault> {
        let location = if source == DirectorySource::Skillhub {
            self.registry.url(&["api", "skills"]).to_string()
        } else {
            #[cfg(any(test, feature = "test-support"))]
            let endpoint = self
                .github
                .endpoint
                .as_ref()
                .map(|url| url.join("skills-sh/").unwrap().to_string());
            #[cfg(not(any(test, feature = "test-support")))]
            let endpoint: Option<String> = None;
            endpoint.unwrap_or_else(|| "https://skills.sh/".into())
        };
        let bytes = serde_json::to_vec(&(location, query, page))
            .map_err(|_| Fault::invalid("invalid skill query"))?;
        let key = *blake3::hash(&bytes).as_bytes();
        let result = self
            .external_skill_catalog
            .0
            .fetch(key, Duration::from_secs(15 * 60), force, &stop, || async {
                let _permit = self.read_permit(&stop).await?;
                match source {
                    DirectorySource::SkillsSh => self.directory_page(query, page, &stop).await,
                    DirectorySource::Skillhub => self.hub_page(query, page, &stop).await,
                    DirectorySource::Official => {
                        unreachable!("official pages have their own cache")
                    }
                }
            })
            .await;
        match result {
            Ok(value) => Ok(value),
            Err(error) if official::unavailable(&error) => {
                let mut value = self.external_skill_catalog.0.peek(&key).unwrap_or(Page {
                    query_scope: if source == DirectorySource::SkillsSh
                        && query.chars().count() == 1
                    {
                        QueryScope::Page
                    } else {
                        QueryScope::Catalog
                    },
                    entries: vec![],
                    page,
                    next: None,
                    total: None,
                    unavailable: false,
                });
                value.unavailable = true;
                Ok(value)
            }
            Err(error) => Err(error),
        }
    }

    async fn directory_page(
        &self,
        query: &str,
        page: u32,
        stop: &CancellationToken,
    ) -> Result<Page, Fault> {
        let searching = query.chars().count() >= 2;
        if searching && page != 1 {
            return Err(Fault::invalid("skill search does not provide another page"));
        }
        let mut url = url::Url::parse("https://skills.sh/").unwrap();
        #[cfg(any(test, feature = "test-support"))]
        if let Some(endpoint) = &self.github.endpoint {
            url = endpoint.join("skills-sh/").unwrap();
        }
        url = url
            .join(if searching {
                "api/search"
            } else {
                "api/skills/all-time/"
            })
            .unwrap();
        if searching {
            url.query_pairs_mut()
                .append_pair("q", query)
                .append_pair("limit", &SEARCH_SIZE.to_string());
        } else {
            url.path_segments_mut()
                .unwrap()
                .pop_if_empty()
                .push(&(page - 1).to_string());
        }
        let response: DirectoryPage = self.catalog_json(url, stop).await?;
        let mut entries = vec![];
        for skill in response.skills {
            entries.push(directory_entry(skill)?);
        }
        // A one-character filter applies only to this real leaderboard page, not a global search.
        if !searching && !query.is_empty() {
            let query = query.to_lowercase();
            entries.retain(|entry| {
                entry.id.to_lowercase().contains(&query)
                    || entry.title("en").to_lowercase().contains(&query)
            });
        }
        Ok(Page {
            query_scope: if !searching && !query.is_empty() {
                QueryScope::Page
            } else {
                QueryScope::Catalog
            },
            entries,
            page,
            next: (!searching && response.has_more)
                .then(|| page.checked_add(1))
                .flatten(),
            total: (!searching && query.is_empty())
                .then_some(response.total)
                .flatten(),
            unavailable: false,
        })
    }

    async fn hub_page(
        &self,
        query: &str,
        page: u32,
        stop: &CancellationToken,
    ) -> Result<Page, Fault> {
        let mut url = self.registry.url(&["api", "skills"]);
        url.query_pairs_mut()
            .append_pair("page", &page.to_string())
            .append_pair("pageSize", &PAGE_SIZE.to_string());
        if !query.is_empty() {
            url.query_pairs_mut().append_pair("keyword", query);
        }
        let response: HubResponse = self.registry.json(url, stop).await?;
        if response.code != 0 {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "Skillhub catalog is unavailable",
            ));
        }
        let mut entries = vec![];
        for skill in response.data.skills {
            let source = RegistrySource {
                namespace: Some(skill.namespace.handle),
                slug: skill.slug,
                version: skill.version,
            };
            super::super::super::registry::validate(&source)?;
            if skill.name.trim().is_empty() {
                return Err(Fault::invalid("invalid Skillhub name"));
            }
            let description_locales = skill
                .description_zh
                .into_iter()
                .map(|value| ("zh-CN".into(), value))
                .collect();
            entries.push(Entry {
                id: format!("{}/{}", source.namespace.as_deref().unwrap(), source.slug),
                package: None,
                name: source.slug.clone(),
                display_name: Some(skill.name),
                icon: icon(skill.icon),
                display_locales: Default::default(),
                description: skill.description,
                description_locales,
                origin: source.namespace.as_ref().unwrap().clone(),
                url: Some(format!(
                    "https://skillhub.cn/skills/{}/{}",
                    source.namespace.as_ref().unwrap(),
                    source.slug
                )),
                source: Some(InstallSource::Registry(source)),
                bundled: false,
            });
        }
        let more = u64::from(page).saturating_mul(PAGE_SIZE as u64) < response.data.total as u64;
        Ok(Page {
            query_scope: QueryScope::Catalog,
            entries,
            page,
            next: more.then(|| page.checked_add(1)).flatten(),
            total: Some(response.data.total),
            unavailable: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_non_git_directory_sources() {
        // Public leaderboards contain web-hosted skills alongside GitHub coordinates.
        let entry = directory_entry(DirectorySkill {
            source: "open.feishu.cn".into(),
            skill_id: "lark-doc".into(),
            name: "lark-doc".into(),
            description: Some("Read documents".into()),
        })
        .unwrap();
        assert_eq!(entry.id, "open.feishu.cn/lark-doc");
        assert_eq!(entry.origin, "open.feishu.cn");
        assert_eq!(entry.source, None);
        assert_eq!(
            entry.url.as_deref(),
            Some("https://skills.sh/open.feishu.cn/lark-doc")
        );
        let entry = directory_entry(DirectorySkill {
            source: "vercel-labs/skills".into(),
            skill_id: "find-skills".into(),
            name: "find-skills".into(),
            description: None,
        })
        .unwrap();
        assert!(matches!(entry.source, Some(InstallSource::Git(_))));
    }

    #[test]
    fn preserves_public_icons() {
        let url = "https://images.example.test/analysis.png";
        assert_eq!(icon(Some(url.into())).as_deref(), Some(url));
        assert_eq!(icon(None), None);
        for value in [
            "",
            "images/analysis.png",
            "javascript:alert(1)",
            "http://images.example.test/icon.png",
            "https://user:secret@images.example.test/icon.png",
        ] {
            assert_eq!(icon(Some(value.into())), None);
        }
    }

    #[tokio::test]
    #[ignore = "requires the public skills.sh and Skillhub directories"]
    async fn public_directories() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let host = Host::new(None);
        for source in [DirectorySource::SkillsSh, DirectorySource::Skillhub] {
            for (query, page) in [("", 1), ("", 2), ("react", 1)] {
                let value = host
                    .skill_catalog(source, query, page, true, CancellationToken::new())
                    .await
                    .unwrap();
                assert!(!value.unavailable, "{source:?} {query:?} page {page}");
                assert!(
                    !value.entries.is_empty(),
                    "{source:?} {query:?} page {page}"
                );
                assert_eq!(value.page, page);
                println!(
                    "{source:?} {query:?} page {page}: {} entries, next {:?}, {} supported sources",
                    value.entries.len(),
                    value.next,
                    value
                        .entries
                        .iter()
                        .filter(|entry| entry.source.is_some())
                        .count()
                );
            }
        }
    }
}
