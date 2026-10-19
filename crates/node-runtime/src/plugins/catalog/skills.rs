//! Official skill entries are projections of real package resources.
use super::*;
use sailry_protocol::plugin::{Skill, skillcatalog};
mod external;
pub(in crate::plugins) use external::Directory;

#[derive(Deserialize)]
pub(super) struct Listing {
    pub package: String,
    pub path: String,
    pub frontmatter: String,
}

pub(in crate::plugins) fn entry(
    package: &str,
    skill: Skill,
    bundled: bool,
) -> Result<skillcatalog::Entry, Fault> {
    validate_name(package)?;
    if skill.path != format!("skills/{}/SKILL.md", skill.name)
        || super::super::github::source::directory(&format!("{package}/skills/{}", skill.name))
            .is_err()
        || skill.name.is_empty()
        || skill.description.trim().is_empty()
    {
        return Err(Fault::invalid("invalid official skill entry"));
    }
    Ok(skillcatalog::Entry {
        id: format!("{package}/{}", skill.name),
        package: Some(package.into()),
        name: skill.name.clone(),
        display_name: skill.display_name,
        icon: None,
        display_locales: skill.display_locales,
        description: Some(skill.description),
        description_locales: Default::default(),
        origin: "sailry/sailry-plugins".into(),
        url: None,
        source: Some(skillcatalog::InstallSource::Git(Source {
            repository: "https://github.com/sailry/sailry-plugins".into(),
            git_ref: Some("main".into()),
            path: Some(format!("{package}/skills/{}", skill.name)),
        })),
        bundled,
    })
}

impl Host {
    pub(crate) async fn skill_catalog(
        &self,
        source: skillcatalog::Source,
        query: &str,
        page: u32,
        force: bool,
        stop: CancellationToken,
    ) -> Result<skillcatalog::Page, Fault> {
        if page == 0 {
            return Err(Fault::invalid("invalid skill catalog page"));
        }
        if source != skillcatalog::Source::Official {
            return self.external_skills(source, query, page, force, stop).await;
        }
        let (mut entries, unavailable) = match official::document(self, force, &stop).await {
            Ok(data) => (data.skills, false),
            Err(error) if official::unavailable(&error) => (bundled::skills()?, true),
            Err(error) => return Err(error),
        };
        let query = query.trim().to_lowercase();
        entries.retain(|entry| {
            std::iter::once(&entry.id)
                .chain(std::iter::once(&entry.name))
                .chain(entry.description.iter())
                .chain(entry.display_name.iter())
                .chain(entry.display_locales.values())
                .any(|text| text.to_lowercase().contains(&query))
        });
        Ok(skillcatalog::Page {
            query_scope: skillcatalog::QueryScope::Catalog,
            total: Some(entries.len()),
            entries,
            page: 1,
            next: None,
            unavailable,
        })
    }
}
