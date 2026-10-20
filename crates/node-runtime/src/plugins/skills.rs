//! Standard skill metadata; package declarations never grant tool permission.
use cap_std::fs::Dir;
use sailry_protocol::{
    Fault,
    plugin::{Issue, IssueKind, Skill},
};
use serde_json::Value;

use super::{manifest::string, package::read};

const MAX_SKILLS: usize = 64;
pub(super) const MAX_BYTES: usize = 128 * 1024;

pub(super) fn discover(root: &Dir, issues: &mut Vec<Issue>) -> Vec<Skill> {
    use cap_fs_ext::DirExt;
    let directory = match root.open_dir_nofollow("skills") {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(_) => {
            issues.push(Issue {
                path: "skills".into(),
                kind: IssueKind::InvalidSkills,
            });
            return Vec::new();
        }
    };
    let entries = match super::package::names(&directory) {
        Ok(entries) => entries,
        Err(_) => {
            issues.push(Issue {
                path: "skills".into(),
                kind: IssueKind::InvalidSkills,
            });
            return Vec::new();
        }
    };
    let mut skills = Vec::new();
    for name in entries {
        let Ok(child) = directory.open_dir_nofollow(&name) else {
            continue;
        };
        let path = format!("skills/{name}/SKILL.md");
        match read(&child, "SKILL.md", MAX_BYTES) {
            Ok(None) => {}
            Ok(Some(bytes)) => match std::str::from_utf8(&bytes)
                .ok()
                .and_then(|text| parse(&name, text).ok())
            {
                Some(mut skill) if skills.len() < MAX_SKILLS => {
                    if let Ok(agents) = child.open_dir_nofollow("agents")
                        && let Ok(Some(bytes)) = read(&agents, "openai.yaml", 16 * 1024)
                        && let Ok(value) = serde_saphyr::from_slice::<Value>(&bytes)
                        && let Some(name) = value["interface"]["display_name"].as_str()
                        && !name.trim().is_empty()
                        && name.len() <= 256
                    {
                        skill.display_name = Some(name.trim().to_owned());
                    }
                    skills.push(skill);
                }
                _ => issues.push(Issue {
                    path,
                    kind: IssueKind::InvalidSkill,
                }),
            },
            Err(_) => issues.push(Issue {
                path,
                kind: IssueKind::InvalidSkill,
            }),
        }
    }
    skills
}

pub(super) fn parse(directory: &str, text: &str) -> Result<Skill, Fault> {
    let skill = parse_document(text)?;
    if skill.name != directory {
        return Err(sailry_protocol::Fault::invalid(
            "skill name must match its directory",
        ));
    }
    Ok(skill)
}

/// Splits a leading YAML frontmatter block into its header and Markdown body.
///
/// The opening `---` may follow a byte-order mark and blank lines; `---` or `...` closes it.
/// Returns `None` when the text has no complete frontmatter block.
pub(super) fn frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut offset = 0;
    let mut opened = false;
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let end = offset + line.len();
        let trimmed = line.trim_end();
        if !opened {
            if trimmed.is_empty() {
                offset = end;
                continue;
            }
            if trimmed != "---" {
                return None;
            }
            opened = true;
            start = end;
        } else if trimmed == "---" || trimmed == "..." {
            return Some((&text[start..offset], &text[end..]));
        }
        offset = end;
    }
    None
}

/// Returns the Markdown body of a skill document without its frontmatter.
pub(super) fn body(text: &str) -> &str {
    frontmatter(text).map_or(text, |(_, body)| body)
}

pub(super) fn parse_document(text: &str) -> Result<Skill, Fault> {
    let (header, _) = frontmatter(text).ok_or_else(|| {
        sailry_protocol::Fault::invalid("skill frontmatter is missing or incomplete")
    })?;
    if header.len() > 16 * 1024 {
        return Err(sailry_protocol::Fault::invalid(
            "skill frontmatter is too large",
        ));
    }
    let options = serde_saphyr::options! {
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        budget: serde_saphyr::budget! { max_depth: 16, max_events: 4096, max_aliases: 64, max_anchors: 64 },
    };
    let value: Value = serde_saphyr::from_str_with_options(header, options)
        .map_err(|_| sailry_protocol::Fault::invalid("invalid skill YAML frontmatter"))?;
    let object = value
        .as_object()
        .ok_or_else(|| sailry_protocol::Fault::invalid("skill frontmatter must be a mapping"))?;
    let name = string(object, "name")?
        .ok_or_else(|| sailry_protocol::Fault::invalid("skill name is required"))?;
    if name.is_empty()
        || name.chars().count() > 64
        || !name
            .chars()
            .all(|value| value == '-' || (value.is_alphanumeric() && !value.is_uppercase()))
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
    {
        return Err(sailry_protocol::Fault::invalid(
            "skill name must match its directory",
        ));
    }
    let description = string(object, "description")?
        .ok_or_else(|| sailry_protocol::Fault::invalid("skill description is required"))?;
    if description.trim().is_empty() || description.chars().count() > 1024 {
        return Err(sailry_protocol::Fault::invalid(
            "skill description is empty or too large",
        ));
    }
    let compatibility = string(object, "compatibility")?;
    if compatibility.is_some_and(|value| value.is_empty() || value.chars().count() > 500) {
        return Err(sailry_protocol::Fault::invalid(
            "skill compatibility is empty or too large",
        ));
    }
    if let Some(tools) = object.get("allowed-tools")
        && !(tools.is_string()
            || tools
                .as_array()
                .is_some_and(|tools| tools.iter().all(Value::is_string)))
    {
        return Err(sailry_protocol::Fault::invalid(
            "skill allowed-tools must be a string or a list of strings",
        ));
    }
    if let Some(metadata) = object.get("metadata")
        && !metadata
            .as_object()
            .is_some_and(|values| values.values().all(Value::is_string))
    {
        return Err(sailry_protocol::Fault::invalid(
            "skill metadata must map strings to strings",
        ));
    }
    let display_locales: std::collections::BTreeMap<_, _> = object
        .get("metadata")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| {
            key.strip_prefix("display-name.").map(|locale| {
                (
                    locale.to_owned(),
                    value.as_str().unwrap_or_default().to_owned(),
                )
            })
        })
        .collect();
    if display_locales.iter().any(|(locale, name)| {
        locale.is_empty()
            || !locale.split('-').all(|part| {
                !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
            || name.trim().is_empty()
            || name.chars().any(char::is_control)
    }) {
        return Err(Fault::invalid("invalid localized skill name"));
    }
    Ok(Skill {
        display_locales,
        name: name.into(),
        display_name: object
            .get("metadata")
            .and_then(|value| value.get("display-name"))
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty() && name.len() <= 256)
            .map(|name| name.trim().to_owned()),
        description: description.into(),
        path: format!("skills/{name}/SKILL.md"),
        license: string(object, "license")?.map(str::to_owned),
        compatibility: compatibility.map(str::to_owned),
    })
}
