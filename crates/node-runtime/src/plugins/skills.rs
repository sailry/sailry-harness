//! Standard skill metadata; package declarations never grant tool permission.
use cap_std::fs::Dir;
use sailry_protocol::{
    Fault,
    plugin::{Issue, IssueKind, Skill},
};
use serde_json::Value;

use super::{invalid, manifest::string, package::read};

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
        return Err(invalid("skill name must match its directory"));
    }
    Ok(skill)
}

pub(super) fn parse_document(text: &str) -> Result<Skill, Fault> {
    let mut lines = text.split_inclusive('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return Err(invalid("skill frontmatter is required"));
    }
    let mut header = String::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        header.push_str(line);
    }
    if !closed || header.len() > 16 * 1024 {
        return Err(invalid("skill frontmatter is incomplete or too large"));
    }
    let options = serde_saphyr::options! {
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        budget: serde_saphyr::budget! { max_depth: 16, max_events: 4096, max_aliases: 64, max_anchors: 64 },
    };
    let value: Value = serde_saphyr::from_str_with_options(&header, options)
        .map_err(|_| invalid("invalid skill YAML frontmatter"))?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid("skill frontmatter must be a mapping"))?;
    let name = string(object, "name")?.ok_or_else(|| invalid("skill name is required"))?;
    if name.is_empty()
        || name.chars().count() > 64
        || !name
            .chars()
            .all(|value| value == '-' || (value.is_alphanumeric() && !value.is_uppercase()))
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
    {
        return Err(invalid("skill name must match its directory"));
    }
    let description =
        string(object, "description")?.ok_or_else(|| invalid("skill description is required"))?;
    if description.trim().is_empty() || description.chars().count() > 1024 {
        return Err(invalid("skill description is empty or too large"));
    }
    let compatibility = string(object, "compatibility")?;
    if compatibility.is_some_and(|value| value.is_empty() || value.chars().count() > 500) {
        return Err(invalid("skill compatibility is empty or too large"));
    }
    if let Some(tools) = object.get("allowed-tools")
        && !(tools.is_string()
            || tools
                .as_array()
                .is_some_and(|tools| tools.iter().all(Value::is_string)))
    {
        return Err(invalid(
            "skill allowed-tools must be a string or a list of strings",
        ));
    }
    if let Some(metadata) = object.get("metadata")
        && !metadata
            .as_object()
            .is_some_and(|values| values.values().all(Value::is_string))
    {
        return Err(invalid("skill metadata must map strings to strings"));
    }
    Ok(Skill {
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
