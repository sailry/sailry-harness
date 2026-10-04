use super::*;
pub(in crate::conversation::live) use sailry_protocol::conversation::reference::{
    Reference, Target,
};
use sailry_protocol::{EntryKind, role};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    Root,
    Files(String),
    Databases,
    Ssh,
    Database(sailry_protocol::DatabaseId, String, Option<String>),
    Agents,
    Sessions,
    Context,
    Commands,
    Models,
    Reasoning,
}

impl Page {
    pub fn label(&self) -> SharedString {
        tr(match self {
            Self::Root => "reference_title",
            Self::Databases => "reference_databases",
            Self::Ssh => "reference_ssh",
            Self::Database(_, name, database) => {
                return database.as_ref().unwrap_or(name).clone().into();
            }
            Self::Files(path) if !path.is_empty() => return path.clone().into(),
            Self::Files(_) => "reference_files",
            Self::Agents => "reference_agents",
            Self::Sessions => "reference_sessions",
            Self::Context => "reference_context",
            Self::Commands => "composer_commands",
            Self::Models => "composer_select_model",
            Self::Reasoning => "composer_effort",
        })
    }
}

#[derive(Clone)]
pub(super) enum Item {
    Attachment,
    Plugin(Box<sailry_protocol::plugin::Info>),
    Page(Page),
    Reference(Reference),
    Current(Reference),
    Command(super::commands::Choice),
}

impl Item {
    pub fn matches(&self, query: &str) -> bool {
        let alias = match self {
            Self::Attachment => "attachment upload",
            Self::Page(Page::Files(_)) => "files",
            Self::Page(Page::Ssh) => "ssh",
            Self::Page(Page::Databases | Page::Database(..)) => "database",
            Self::Page(Page::Agents) => "agents",
            Self::Page(Page::Sessions) => "sessions",
            Self::Page(Page::Context) => "context",
            Self::Page(Page::Models) => "model",
            Self::Page(Page::Reasoning) => "reasoning",
            _ => "",
        };
        let key = match self {
            Self::Plugin(info) => info.summary.name.clone(),
            Self::Command(super::commands::Choice::Skill(package, skill)) => {
                format!("{}:{}", package.name, skill.name)
            }
            _ => String::new(),
        };
        key.to_lowercase().contains(query)
            || self.label().to_lowercase().contains(query)
            || self.description().to_lowercase().contains(query)
            || (!alias.is_empty() && alias.contains(query))
    }

    pub fn label(&self) -> SharedString {
        match self {
            Self::Attachment => tr("composer_attachment"),
            Self::Plugin(info) => crate::plugins::metadata::title(info).into(),
            Self::Current(_) => tr("reference_select_current"),
            Self::Page(Page::Models) => "/model".into(),
            Self::Page(Page::Reasoning) => "/reasoning".into(),
            Self::Page(page) => page.label(),
            Self::Reference(reference) => reference.label.clone().into(),
            Self::Command(command) => command.label(),
        }
    }

    pub fn emblem(&self, cx: &App) -> AnyElement {
        if let Self::Command(commands::Choice::External(entry)) = self {
            return entry
                .icon()
                .unwrap_or_else(|| self.icon().into())
                .size_4()
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .into_any_element();
        }
        if let Self::Plugin(info) = self {
            return crate::plugins::emblem::render(
                &info.summary.name,
                crate::plugins::emblem::glyph(info),
                info.icon.as_deref(),
                px(16.),
                cx,
            );
        }
        if let Self::Command(commands::Choice::Skill(package, skill)) = self {
            return Icon::default()
                .path(super::inline::icon(&Reference {
                    target: Target::Skill {
                        package: package.name.clone(),
                        name: skill.name.clone(),
                    },
                    label: self.label().to_string(),
                }))
                .size_4()
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .into_any_element();
        }
        Icon::new(self.icon())
            .size_4()
            .flex_shrink_0()
            .text_color(cx.theme().muted_foreground)
            .into_any_element()
    }

    pub fn icon(&self) -> IconName {
        match self {
            Self::Attachment => IconName::Plus,
            Self::Plugin(_)
            | Self::Reference(Reference {
                target: Target::Plugin(_),
                ..
            }) => IconName::BookOpen,
            Self::Reference(Reference {
                target: Target::Skill { .. },
                ..
            }) => IconName::BookOpen,
            Self::Current(_) => IconName::HardDrive,
            Self::Command(command) => command.icon(),
            Self::Page(Page::Databases | Page::Database(..))
            | Self::Reference(Reference {
                target: Target::Database { .. },
                ..
            }) => IconName::HardDrive,
            Self::Page(Page::Files(_))
            | Self::Reference(Reference {
                target: Target::Directory(_) | Target::Project,
                ..
            }) => IconName::Folder,
            Self::Page(Page::Agents | Page::Sessions)
            | Self::Reference(Reference {
                target: Target::Agent(_) | Target::Session(_),
                ..
            }) => IconName::Bot,
            Self::Reference(Reference {
                target: Target::File(_),
                ..
            }) => IconName::File,
            _ => IconName::Network,
        }
    }
}

impl View {
    pub(super) fn reference_catalog(&self, page: &Page, cx: &App) -> Vec<Item> {
        let reference = |target, label: String| Item::Reference(Reference { target, label });
        match page {
            Page::Commands | Page::Models | Page::Reasoning => self.command_catalog(page, cx),
            Page::Database(..) => vec![],
            Page::Ssh => self
                .reference_ssh()
                .into_iter()
                .map(|profile| reference(Target::Ssh(profile.id), profile.name.clone()))
                .collect(),
            Page::Databases => self
                .reference_databases()
                .into_iter()
                .map(|profile| Item::Page(Page::Database(profile.id, profile.name.clone(), None)))
                .collect(),
            Page::Root if !matches!(self.composer_options.mentions, Mentions::Workspace) => {
                vec![Item::Attachment]
            }
            Page::Root => vec![
                Page::Files(String::new()),
                Page::Agents,
                Page::Sessions,
                Page::Context,
                Page::Databases,
                Page::Ssh,
            ]
            .into_iter()
            .map(Item::Page)
            .chain(std::iter::once(Item::Attachment))
            .collect(),
            Page::Files(_) => vec![],
            Page::Agents => self
                .reference_roles()
                .iter()
                .map(|role| reference(Target::Agent(role.reference()), format!("@{}", role.key)))
                .collect(),
            Page::Sessions => self
                .node
                .snapshot
                .iter()
                .flat_map(|snapshot| &snapshot.sessions)
                .filter(|session| {
                    session.project == self.binding.project
                        && Some(session.worktree) == self.binding.worktree
                })
                .map(|session| {
                    reference(
                        Target::Session(session.id),
                        if session.activity.title.is_empty() {
                            session.id.to_string()
                        } else {
                            session.activity.title.clone()
                        },
                    )
                })
                .collect(),
            Page::Context => {
                let mut items = vec![reference(Target::Host, self.binding.host.to_string())];
                if self.binding.project.is_some() {
                    items.push(reference(
                        Target::Project,
                        self.binding.project_name.to_string(),
                    ));
                    items.push(reference(Target::Worktree, self.binding.branch.to_string()));
                }
                items
            }
        }
    }

    pub(super) fn reference_roles(&self) -> &[role::Profile] {
        if let Some(session) = &self.session {
            &session.roles.profiles
        } else {
            let node = if self.config_owner == self.binding.client.target() {
                &self.node
            } else {
                &self.defaults
            };
            node.snapshot
                .as_ref()
                .map_or(&[], |snapshot| &snapshot.roles)
        }
    }

    pub(in crate::conversation::live) fn references_valid(&self, references: &[Reference]) -> bool {
        references.iter().all(|reference| match &reference.target {
            Target::Plugin(package) | Target::Skill { package, .. } => self
                .skill_packages()
                .iter()
                .any(|item| item.name == *package),
            // Cross-Node import remaps fixed-model provider IDs, preserving the role revision.
            Target::Agent(role) => self
                .reference_roles()
                .iter()
                .any(|current| current.reference() == *role),
            Target::Session(id) => self.node.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.sessions.iter().any(|session| {
                    session.id == *id
                        && session.project == self.binding.project
                        && Some(session.worktree) == self.binding.worktree
                })
            }),
            Target::Ssh(id) => self.reference_ssh().iter().any(|profile| profile.id == *id),
            Target::Database { connection, .. } => self
                .reference_databases()
                .iter()
                .any(|profile| profile.id == *connection),
            _ => self.resource.is_none(),
        })
    }
}

pub(super) fn files(directory: &sailry_protocol::Directory) -> Vec<Item> {
    let mut rows = vec![];
    if !directory.path.is_empty() {
        rows.push(path_reference(directory.path.clone(), true));
    }
    for entry in &directory.entries {
        let path = if directory.path.is_empty() {
            entry.name.clone()
        } else {
            format!("{}/{}", directory.path, entry.name)
        };
        match entry.kind {
            EntryKind::Directory => rows.push(Item::Page(Page::Files(path))),
            EntryKind::File => rows.push(path_reference(path, false)),
            _ => {}
        }
    }
    rows
}

fn path_reference(path: String, directory: bool) -> Item {
    Item::Reference(Reference {
        label: path.clone(),
        target: if directory {
            Target::Directory(path)
        } else {
            Target::File(path)
        },
    })
}
