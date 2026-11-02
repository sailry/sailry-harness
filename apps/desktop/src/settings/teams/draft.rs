//! Role and team drafting rules. Presets, effort selection and key rules are adapted
//! from the delegation plugin's role settings (Sailry ef5e82ea, `desktop/draft.js`).
use crate::tr;
use gpui_kit::SharedString;
use sailry_protocol::{Effort, projects::Appearance, role};
use std::collections::BTreeSet;

/// Complete templates for new roles; editing an existing role replaces only its instructions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Preset {
    Review,
    Test,
    Implement,
    Research,
    Docs,
}

impl Preset {
    pub const ALL: [Self; 5] = [
        Self::Review,
        Self::Test,
        Self::Implement,
        Self::Research,
        Self::Docs,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Review => "role_preset_review",
            Self::Test => "role_preset_test",
            Self::Implement => "role_preset_implement",
            Self::Research => "role_preset_research",
            Self::Docs => "role_preset_docs",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Review => "role_review_description",
            Self::Test => "role_test_description",
            Self::Implement => "role_implement_description",
            Self::Research => "role_research_description",
            Self::Docs => "role_docs_description",
        }
    }

    pub fn instructions(self) -> &'static str {
        match self {
            Self::Review => "role_prompt_review",
            Self::Test => "role_prompt_test",
            Self::Implement => "role_prompt_implement",
            Self::Research => "role_prompt_research",
            Self::Docs => "role_prompt_docs",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Review => "reviewer",
            Self::Test => "verifier",
            Self::Implement => "implementer",
            Self::Research => "research",
            Self::Docs => "writer",
        }
    }

    pub fn appearance(self) -> Appearance {
        let (icon, color) = match self {
            Self::Review => ("code", "blue"),
            Self::Test => ("science", "green"),
            Self::Implement => ("tools", "orange"),
            Self::Research => ("ai", "cyan"),
            Self::Docs => ("writing", "violet"),
        };
        Appearance {
            icon: icon.into(),
            color: color.into(),
        }
    }

    pub fn read_only(self) -> bool {
        matches!(self, Self::Review | Self::Research)
    }

    pub fn max_turns(self) -> Option<u32> {
        (self == Self::Research).then_some(8)
    }

    pub fn skills(self) -> Vec<String> {
        if self == Self::Research {
            vec!["web-search:research".into()]
        } else {
            Vec::new()
        }
    }
}

/// Display name and description, localized while a seeded role keeps the Node's text.
pub(crate) fn label(role: &role::Profile) -> (SharedString, SharedString) {
    let keys = match role::Builtin::find(&role.key) {
        Some(builtin) if role.name == builtin.name && role.description == builtin.description => {
            match builtin.key {
                "explore" => Some((
                    "role_builtin_explore_name",
                    "role_builtin_explore_description",
                )),
                "research" => Some((
                    "role_builtin_research_name",
                    "role_builtin_research_description",
                )),
                _ => None,
            }
        }
        _ => None,
    };
    match keys {
        Some((name, description)) => (tr(name), tr(description)),
        None => (role.name.clone().into(), role.description.clone().into()),
    }
}

/// The standard agent identity for legacy roles without a stored appearance.
pub(super) fn fallback() -> Appearance {
    Appearance {
        icon: "ai".into(),
        color: "none".into(),
    }
}

/// Mirrors the Node's selection-key rule shared by roles and teams.
pub(super) fn valid_key(value: &str) -> bool {
    let key = value.as_bytes();
    key.first().is_some_and(u8::is_ascii_lowercase)
        && key.last().is_some_and(u8::is_ascii_alphanumeric)
        && key
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(byte))
}

/// Derives an unused selection key from a display name for records whose form has no key field.
pub(super) fn key(name: &str, prefix: &str, taken: &BTreeSet<String>) -> String {
    let mut slug = String::new();
    for character in name.trim().chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let base = if slug.starts_with(|character: char| character.is_ascii_lowercase()) {
        slug.to_owned()
    } else if slug.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}-{slug}")
    };
    if !taken.contains(&base) {
        return base;
    }
    (2..)
        .map(|index| format!("{base}-{index}"))
        .find(|candidate| !taken.contains(candidate))
        .expect("an unused numeric suffix exists")
}

/// Keeps a supported effort when the model changes, else uses the model's initial effort.
/// None selects the model's default effort.
pub(super) fn effort(model: &crate::settings::Model, current: Option<Effort>) -> Option<Effort> {
    if !model.reasoning {
        return None;
    }
    if let Some(current) = current.filter(|effort| model.efforts.contains(effort)) {
        return Some(current);
    }
    Some(Effort::initial(&model.efforts, model.default_effort))
        .filter(|effort| *effort != Effort::Default)
}

#[cfg(test)]
mod tests;
