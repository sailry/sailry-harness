use crate::{preview, settings::Role, tr, workspace};
use gpui_kit::{SharedString, component::IconName};
use std::ops::Range;

pub(super) type Key = (usize, usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    File(String),
    Directory(String),
    Agent(Box<Role>),
    Session(Key),
    Host,
    Project,
    Worktree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reference {
    pub owner: workspace::Owner,
    pub label: SharedString,
    pub kind: Kind,
}

impl Reference {
    pub(super) fn same_target(&self, other: &Self) -> bool {
        self.owner == other.owner && self.kind == other.kind
    }

    pub fn icon(&self) -> IconName {
        match self.kind {
            Kind::File(_) => IconName::File,
            Kind::Directory(_) | Kind::Project => IconName::Folder,
            Kind::Agent(_) => IconName::Bot,
            Kind::Session(_) => IconName::Bot,
            Kind::Host => IconName::Network,
            Kind::Worktree => IconName::Network,
        }
    }

    pub(super) fn valid(
        &self,
        owner: workspace::Owner,
        workspace: &workspace::State,
        roles: &[Role],
    ) -> bool {
        if self.owner != owner || !workspace.contains(owner) {
            return false;
        }
        match &self.kind {
            Kind::File(path) => preview::FILES.contains(&path.as_str()),
            Kind::Directory(path) => preview::FILES
                .iter()
                .any(|file| file.starts_with(&format!("{path}/"))),
            Kind::Agent(role) => roles.contains(role),
            Kind::Session(key) => workspace
                .sessions
                .get(key)
                .is_some_and(|session| session.owner == owner && !session.archived),
            Kind::Host | Kind::Project | Kind::Worktree => true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    Root,
    Files(String),
    Agents,
    Sessions,
    Context,
}

impl Page {
    pub fn label(&self) -> SharedString {
        tr(match self {
            Self::Root => "reference_title",
            Self::Files(path) if !path.is_empty() => return path.clone().into(),
            Self::Files(_) => "reference_files",
            Self::Agents => "reference_agents",
            Self::Sessions => "reference_sessions",
            Self::Context => "reference_context",
        })
    }
}

#[derive(Clone)]
pub(super) enum Item {
    Page(Page, IconName, &'static str),
    Reference(Reference, SharedString),
}

impl Item {
    pub fn label(&self) -> SharedString {
        match self {
            Self::Page(page, ..) => page.label(),
            Self::Reference(reference, _) => reference.label.clone(),
        }
    }

    pub fn description(&self) -> SharedString {
        match self {
            Self::Page(_, _, key) => tr(key),
            Self::Reference(_, description) => description.clone(),
        }
    }

    pub fn icon(&self) -> IconName {
        match self {
            Self::Page(_, icon, _) => icon.clone(),
            Self::Reference(reference, _) => reference.icon(),
        }
    }
}

pub(super) fn items(
    page: &Page,
    owner: workspace::Owner,
    workspace: &workspace::State,
    roles: &[Role],
    query: &str,
) -> Vec<Item> {
    if !workspace.contains(owner) {
        return Vec::new();
    }
    let reference = |label: SharedString, kind, description: SharedString| {
        Item::Reference(Reference { owner, label, kind }, description)
    };
    let rows = match page {
        Page::Root => vec![
            Item::Page(
                Page::Files(String::new()),
                IconName::Folder,
                "reference_files_description",
            ),
            Item::Page(Page::Agents, IconName::Bot, "reference_agents_description"),
            Item::Page(
                Page::Sessions,
                IconName::Bot,
                "reference_sessions_description",
            ),
            Item::Page(
                Page::Context,
                IconName::Network,
                "reference_context_description",
            ),
        ],
        Page::Files(path) => {
            let mut rows = Vec::new();
            if !path.is_empty() {
                rows.push(reference(
                    path.clone().into(),
                    Kind::Directory(path.clone()),
                    tr("reference_directory"),
                ));
            }
            let prefix = if path.is_empty() {
                String::new()
            } else {
                format!("{path}/")
            };
            let mut directories = std::collections::BTreeSet::new();
            for file in preview::FILES.iter() {
                let Some(relative) = file.strip_prefix(&prefix) else {
                    continue;
                };
                if let Some((directory, _)) = relative.split_once('/') {
                    let directory = format!("{prefix}{directory}");
                    if directories.insert(directory.clone()) {
                        rows.push(Item::Page(
                            Page::Files(directory),
                            IconName::Folder,
                            "reference_files_description",
                        ));
                    }
                } else {
                    rows.push(reference(
                        (*file).into(),
                        Kind::File((*file).into()),
                        tr("reference_file"),
                    ));
                }
            }
            rows
        }
        Page::Agents => roles
            .iter()
            .map(|role| {
                reference(
                    format!("@{}", role.id).into(),
                    Kind::Agent(Box::new(role.clone())),
                    role.description.clone().into(),
                )
            })
            .collect(),
        Page::Sessions => workspace
            .sessions
            .iter()
            .filter(|(_, session)| session.owner == owner && !session.archived)
            .map(|(key, session)| {
                reference(
                    session.title.clone(),
                    Kind::Session(*key),
                    tr("reference_sessions_description"),
                )
            })
            .collect(),
        Page::Context => vec![
            reference(
                tr(if owner.host == 0 {
                    "local_host_name"
                } else {
                    "remote_host_name"
                }),
                Kind::Host,
                tr("reference_host"),
            ),
            reference(
                workspace.projects[&owner.project].name.clone(),
                Kind::Project,
                tr("reference_project"),
            ),
            reference(
                workspace.branch_label(owner),
                Kind::Worktree,
                tr("reference_worktree"),
            ),
        ],
    };
    let query = query.to_lowercase();
    rows.into_iter()
        .filter(|item| {
            let aliases = match item {
                Item::Page(Page::Files(_), ..) => "files",
                Item::Page(Page::Agents, ..) => "agents",
                Item::Page(Page::Sessions, ..) => "sessions",
                Item::Page(Page::Context, ..) => "context",
                _ => "",
            };
            format!("{} {} {aliases}", item.label(), item.description())
                .to_lowercase()
                .contains(&query)
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::conversation) struct Trigger {
    pub text: SharedString,
    pub range: Range<usize>,
    pub query: String,
}

impl Trigger {
    pub fn parse(text: SharedString, selection: Range<usize>) -> Option<Self> {
        Self::token(text, selection, '@')
    }

    pub fn command(text: SharedString, selection: Range<usize>) -> Option<Self> {
        let trigger = Self::token(text, selection, '/')?;
        (!trigger.query.contains('/')).then_some(trigger)
    }

    fn token(text: SharedString, selection: Range<usize>, marker: char) -> Option<Self> {
        if !selection.is_empty() || !text.is_char_boundary(selection.start) {
            return None;
        }
        let prefix = text.get(..selection.start)?;
        let start = prefix.rfind(marker)?;
        if prefix[..start]
            .chars()
            .next_back()
            .is_some_and(|ch| !ch.is_whitespace())
        {
            return None;
        }
        let query = &prefix[start + 1..];
        if query.chars().any(|ch| ch.is_whitespace() || ch == '@')
            || text[selection.end..]
                .chars()
                .next()
                .is_some_and(|ch| !ch.is_whitespace())
        {
            return None;
        }
        Some(Self {
            query: query.into(),
            text,
            range: start..selection.end,
        })
    }
}
