//! Public directory pages do not substitute for source inspection or installation.
use super::*;
use sailry_protocol::plugin::{
    skillcatalog::{Entry, InstallSource, Page, QueryScope, Source as DirectorySource},
    skills::RegistrySource,
};
use std::time::{Duration, Instant};

const PAGE_SIZE: usize = 24;
const SEARCH_SIZE: usize = 100;

#[derive(Default)]
pub(in crate::plugins) struct Directory(tokio::sync::Mutex<Option<Document>>);

struct Document {
    source: DirectorySource,
    query: String,
    page: u32,
    value: Page,
    attempted: Instant,
}

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

impl Host {
    pub(super) async fn external_skills(
        &self,
        source: DirectorySource,
        query: &str,
        page: u32,
        force: bool,
        stop: CancellationToken,
    ) -> Result<Page, Fault> {
        let mut cache = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(github::cancelled()),
            cache = self.external_skill_catalog.0.lock() => cache,
        };
        let matches =
            |entry: &Document| entry.source == source && entry.query == query && entry.page == page;
        if !force && let Some(entry) = cache.as_ref().filter(|entry| matches(entry)) {
            let ttl = if entry.value.unavailable { 30 } else { 15 * 60 };
            if entry.attempted.elapsed() < Duration::from_secs(ttl) {
                return Ok(entry.value.clone());
            }
        }
        let _permit = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "skill catalog is busy"))?;
        let result = match source {
            DirectorySource::SkillsSh => self.directory_page(query, page, &stop).await,
            DirectorySource::Skillhub => self.hub_page(query, page, &stop).await,
            DirectorySource::Official => unreachable!("official pages have their own cache"),
        };
        if stop.is_cancelled() {
            return Err(github::cancelled());
        }
        let value = match result {
            Ok(value) => value,
            Err(error) if official::unavailable(&error) => {
                let mut value = cache
                    .as_ref()
                    .filter(|entry| matches(entry))
                    .map(|entry| entry.value.clone())
                    .unwrap_or(Page {
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
                        unavailable: true,
                    });
                value.unavailable = true;
                value
            }
            Err(error) => return Err(error),
        };
        *cache = Some(Document {
            source,
            query: query.into(),
            page,
            value: value.clone(),
            attempted: Instant::now(),
        });
        Ok(value)
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
            let parts = skill.source.split('/').collect::<Vec<_>>();
            if parts.len() != 2
                || !parts.iter().all(|part| segment(part))
                || !segment(&skill.skill_id)
                || skill.name.trim().is_empty()
            {
                return Err(Fault::invalid("invalid skills.sh source"));
            }
            entries.push(Entry {
                id: format!("{}/{}", skill.source, skill.skill_id),
                package: None,
                name: skill.skill_id.clone(),
                display_name: Some(skill.name),
                display_locales: Default::default(),
                description: None,
                description_locales: Default::default(),
                source: InstallSource::Git(Source {
                    repository: format!("https://skills.sh/{}/{}", skill.source, skill.skill_id),
                    git_ref: None,
                    path: None,
                }),
                bundled: false,
            });
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
                display_locales: Default::default(),
                description: skill.description,
                description_locales,
                source: InstallSource::Registry(source),
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
