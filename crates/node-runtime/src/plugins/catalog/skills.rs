//! Official skill entries are projections of real package resources.
use super::*;
use sailry_protocol::plugin::{Skill, skillcatalog};

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
        package: package.into(),
        source: Source {
            repository: "https://github.com/sailry/sailry-plugins".into(),
            git_ref: Some("main".into()),
            path: Some(format!("{package}/skills/{}", skill.name)),
        },
        skill,
        bundled,
    })
}

impl Host {
    pub(crate) async fn skill_catalog(
        &self,
        query: &str,
        force: bool,
        stop: CancellationToken,
    ) -> Result<skillcatalog::Page, Fault> {
        let (mut entries, unavailable) = match official::document(self, force, &stop).await {
            Ok(data) => (data.skills, false),
            Err(error) if official::unavailable(&error) => (bundled::skills()?, true),
            Err(error) => return Err(error),
        };
        let query = query.trim().to_lowercase();
        entries.retain(|entry| {
            std::iter::once(&entry.id)
                .chain(std::iter::once(&entry.skill.name))
                .chain(std::iter::once(&entry.skill.description))
                .chain(entry.skill.display_name.iter())
                .chain(entry.skill.display_locales.values())
                .any(|text| text.to_lowercase().contains(&query))
        });
        Ok(skillcatalog::Page {
            entries,
            unavailable,
        })
    }
}
