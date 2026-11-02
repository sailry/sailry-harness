use serde::{Deserialize, Serialize};

/// Registered project resources only, without sessions or private scratch directories.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub projects: Vec<crate::Project>,
    pub worktrees: Vec<crate::Worktree>,
}

pub const ICONS: &[&str] = &[
    "folder",
    "code",
    "terminal",
    "globe",
    "server",
    "database",
    "package",
    "game",
    "book",
    "finance",
    "device",
    "education",
    "writing",
    "tags",
    "music",
    "media",
    "design",
    "health",
    "nature",
    "business",
    "analytics",
    "profile",
    "fitness",
    "law",
    "audio",
    "travel",
    "tools",
    "science",
    "ai",
    "favorite",
];
pub const COLORS: &[&str] = &[
    "none", "blue", "indigo", "violet", "magenta", "red", "orange", "amber", "green", "teal",
    "cyan",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Appearance {
    pub icon: String,
    pub color: String,
}

impl Appearance {
    /// A random icon with a visible color, so unconfigured resources stay distinguishable.
    pub fn random() -> Self {
        let bytes = uuid::Uuid::new_v4().into_bytes();
        let colors = &COLORS[1..];
        Self {
            icon: ICONS[usize::from(bytes[0]) % ICONS.len()].into(),
            color: colors[usize::from(bytes[1]) % colors.len()].into(),
        }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            icon: "folder".into(),
            color: "none".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Local,
    Clone { url: String, branch: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub name: String,
    /// Absolute directory on the execution Node, including the new clone name.
    pub path: String,
    pub appearance: Appearance,
    pub source: Source,
}
