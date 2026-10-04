//! Read existing navigation owners, capturing identities for each displayed choice.
use super::*;
use crate::{live::menus, plugins::navigation::SettingsEntry, settings::Section};
use sailry_protocol::{
    NodeId, ProjectId, SessionId, Snapshot,
    connection::Resource,
    plugin::desktop::{SettingsGroup, Surface},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Folder {
    Root,
    Sessions,
    Projects,
    Hosts,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scope {
    Node(NodeId),
    Preview(usize),
}

impl Scope {
    pub fn capture(shell: &Shell) -> Self {
        shell
            .live
            .as_ref()
            .map(|live| Self::Node(live.selected))
            .unwrap_or(Self::Preview(shell.host))
    }

    fn matches(self, shell: &Shell) -> bool {
        Self::capture(shell) == self
    }

    fn snapshot(self, shell: &Shell) -> Option<&Snapshot> {
        let Self::Node(node) = self else { return None };
        let live = shell.live.as_ref()?;
        (live.selected == node && live.view.connected)
            .then_some(live.view.snapshot.as_ref())
            .flatten()
            .filter(|snapshot| snapshot.node == node)
    }
}

#[derive(Clone)]
pub(super) enum Target {
    Folder(Folder),
    Session(SessionId),
    Project(ProjectId),
    Host(NodeId),
    PreviewSession((usize, usize)),
    PreviewProject(usize),
    PreviewHost(usize),
    Feature(Box<rail::Feature>),
    Setting(Section),
    PluginSetting(SettingsEntry),
}

impl Target {
    pub(super) fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Folder(a), Self::Folder(b)) => a == b,
            (Self::Session(a), Self::Session(b)) => a == b,
            (Self::Project(a), Self::Project(b)) => a == b,
            (Self::Host(a), Self::Host(b)) => a == b,
            (Self::PreviewSession(a), Self::PreviewSession(b)) => a == b,
            (Self::PreviewProject(a), Self::PreviewProject(b)) => a == b,
            (Self::PreviewHost(a), Self::PreviewHost(b)) => a == b,
            (Self::Setting(a), Self::Setting(b)) => a == b,
            (Self::PluginSetting(a), Self::PluginSetting(b)) => a == b,
            (Self::Feature(a), Self::Feature(b)) => match (&a.destination, &b.destination) {
                (rail::Destination::Page(a), rail::Destination::Page(b)) => a == b,
                (rail::Destination::Plugin(a), rail::Destination::Plugin(b)) => a == b,
                _ => false,
            },
            _ => false,
        }
    }
}

#[derive(Clone)]
pub(super) struct Choice {
    pub label: SharedString,
    pub icon: Icon,
    pub keywords: Vec<SharedString>,
    pub selected: bool,
    pub target: Target,
}

impl Choice {
    fn new(label: impl Into<SharedString>, icon: impl Into<Icon>, target: Target) -> Self {
        Self {
            label: label.into(),
            icon: icon.into(),
            keywords: Vec::new(),
            selected: false,
            target,
        }
    }
}

#[derive(Clone)]
pub(super) struct Group {
    pub label: Option<SharedString>,
    pub choices: Vec<Choice>,
}

pub(super) fn entries(shell: &Shell, scope: Scope, folder: Folder, cx: &App) -> Vec<Group> {
    if folder == Folder::Root {
        let mut choices: Vec<_> = [
            (Folder::Sessions, "shell_sessions", session_icon()),
            (Folder::Projects, "projects", IconName::Folder.into()),
            (Folder::Hosts, "hosts", IconName::Cpu.into()),
            (Folder::Settings, "settings", IconName::Settings.into()),
        ]
        .into_iter()
        .map(|(folder, label, icon)| Choice::new(tr(label), icon, Target::Folder(folder)))
        .collect();
        if scope.matches(shell) {
            choices.extend(features(shell, cx));
        }
        return vec![Group {
            label: None,
            choices,
        }];
    }
    if !scope.matches(shell) {
        return Vec::new();
    }
    if folder == Folder::Settings {
        return settings(shell, cx);
    }
    let choices = match folder {
        Folder::Sessions => sessions(shell, scope, cx),
        Folder::Projects => projects(shell, scope),
        Folder::Hosts => hosts(shell),
        Folder::Root | Folder::Settings => unreachable!(),
    };
    vec![Group {
        label: None,
        choices,
    }]
}

fn sessions(shell: &Shell, scope: Scope, cx: &App) -> Vec<Choice> {
    match scope {
        Scope::Node(node) => {
            let Some(snapshot) = scope.snapshot(shell) else {
                return Vec::new();
            };
            let recent = crate::preferences::data(cx).recent.unwrap_or_default();
            let mut sessions: Vec<_> = snapshot
                .sessions
                .iter()
                .filter(|session| {
                    !session.archived
                        && session.delegation.is_none()
                        && snapshot.worktrees.iter().any(|tree| {
                            tree.id == session.worktree && tree.project == session.project
                        })
                        && session.project.is_none_or(|id| {
                            snapshot.projects.iter().any(|project| project.id == id)
                        })
                        && session
                            .config
                            .resource
                            .is_none_or(|resource| match resource {
                                Resource::Database(id) => {
                                    snapshot.databases.iter().any(|profile| profile.id == id)
                                }
                                Resource::Ssh(id) => {
                                    snapshot.ssh.iter().any(|profile| profile.id == id)
                                }
                            })
                })
                .collect();
            // Stable sorting preserves the authoritative snapshot order for unvisited sessions.
            sessions.sort_by_key(|session| {
                recent
                    .iter()
                    .position(|visit| visit.node == node && visit.session == session.id)
                    .unwrap_or(usize::MAX)
            });
            sessions
                .into_iter()
                .map(|session| {
                    let mut choice = Choice::new(
                        crate::activity::title(session),
                        session_icon(),
                        Target::Session(session.id),
                    );
                    if let Some(project) = snapshot
                        .projects
                        .iter()
                        .find(|project| Some(project.id) == session.project)
                    {
                        choice.keywords.push(project.name.clone().into());
                    }
                    if let Some(chat) = shell.current_chat() {
                        choice.selected = chat.read(cx).session() == Some(session.id);
                    }
                    choice
                })
                .collect()
        }
        Scope::Preview(host) => {
            let mut sessions: Vec<_> = shell
                .workspace
                .sessions
                .iter()
                .filter(|((node, _), session)| {
                    *node == host && !session.archived && shell.workspace.contains(session.owner)
                })
                .collect();
            sessions.sort_by_key(|(key, _)| {
                shell
                    .sidebar
                    .recent_preview
                    .iter()
                    .position(|visit| visit == *key)
                    .unwrap_or(usize::MAX)
            });
            sessions
                .into_iter()
                .map(|(key, session)| {
                    Choice::new(
                        session.title.clone(),
                        session_icon(),
                        Target::PreviewSession(*key),
                    )
                })
                .collect()
        }
    }
}

fn session_icon() -> Icon {
    Icon::default().path("reicon:messages/chat-round")
}

fn projects(shell: &Shell, scope: Scope) -> Vec<Choice> {
    match scope {
        Scope::Node(_) => scope
            .snapshot(shell)
            .into_iter()
            .flat_map(|snapshot| &snapshot.projects)
            .map(|project| {
                Choice::new(
                    project.name.clone(),
                    IconName::Folder,
                    Target::Project(project.id),
                )
            })
            .collect(),
        Scope::Preview(host) => shell
            .workspace
            .projects
            .iter()
            .filter(|(_, project)| project.host == host)
            .map(|(id, project)| {
                Choice::new(
                    project.name.clone(),
                    IconName::Folder,
                    Target::PreviewProject(*id),
                )
            })
            .collect(),
    }
}

fn hosts(shell: &Shell) -> Vec<Choice> {
    if let Some(live) = &shell.live {
        live.hosts
            .keys()
            .map(|node| {
                let mut choice = Choice::new(live.name(*node), IconName::Cpu, Target::Host(*node));
                choice.selected = live.selected == *node;
                choice
            })
            .collect()
    } else {
        ["local_host_name", "remote_host_name"]
            .into_iter()
            .enumerate()
            .map(|(host, label)| Choice::new(tr(label), IconName::Cpu, Target::PreviewHost(host)))
            .collect()
    }
}

fn features(shell: &Shell, cx: &App) -> Vec<Choice> {
    shell
        .features(cx)
        .into_iter()
        .filter(|feature| match &feature.destination {
            rail::Destination::Page(page) => !matches!(page, Page::Conversation | Page::Activity),
            rail::Destination::Plugin(entry) => {
                entry.navigation.surface == Surface::Workspace
                    && shell.command_navigation_available(entry, cx)
            }
        })
        .map(|feature| {
            let mut choice = Choice::new(
                feature.label.clone(),
                feature.icon.clone(),
                Target::Feature(Box::new(feature.clone())),
            );
            choice.selected = feature.selected(shell, cx);
            choice
        })
        .collect()
}

fn settings(shell: &Shell, cx: &App) -> Vec<Group> {
    let plugins = shell.command_settings_entries(cx);
    [
        SettingsGroup::App,
        SettingsGroup::Ai,
        SettingsGroup::Tools,
        SettingsGroup::System,
    ]
    .into_iter()
    .zip(Section::GROUPS)
    .map(|(group, (label, sections))| {
        let mut choices: Vec<_> = sections
            .iter()
            .map(|section| {
                (
                    section.navigation_order(),
                    Choice::new(tr(section.key()), section.icon(), Target::Setting(*section)),
                )
            })
            .collect();
        choices.extend(
            plugins
                .iter()
                .filter(|entry| entry.placement.group == group)
                .map(|entry| {
                    (
                        entry.placement.order,
                        Choice::new(
                            entry.label.clone(),
                            entry
                                .icon
                                .as_ref()
                                .map(crate::assets::icons::icon)
                                .unwrap_or_else(|| IconName::Settings2.into()),
                            Target::PluginSetting(entry.clone()),
                        ),
                    )
                }),
        );
        choices.sort_by_key(|(order, _)| *order);
        Group {
            label: Some(tr(label)),
            choices: choices.into_iter().map(|(_, choice)| choice).collect(),
        }
    })
    .collect()
}

pub(super) fn available(
    shell: &Shell,
    scope: Scope,
    folder: Folder,
    target: &Target,
    cx: &App,
) -> bool {
    scope.matches(shell)
        && entries(shell, scope, folder, cx)
            .iter()
            .flat_map(|group| &group.choices)
            .any(|choice| choice.target.same(target))
}

pub(super) fn open(
    shell: &mut Shell,
    scope: Scope,
    folder: Folder,
    target: Target,
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    if !available(shell, scope, folder, &target, cx) {
        return;
    }
    match target {
        Target::Session(id) => {
            let Scope::Node(node) = scope else { return };
            shell.live_resource_action(
                &menus::Dispatch {
                    node,
                    target: menus::Target::Session(id),
                    command: menus::Command::Open,
                },
                window,
                cx,
            );
        }
        Target::Project(id) => {
            let Scope::Node(node) = scope else { return };
            shell.live_resource_action(
                &menus::Dispatch {
                    node,
                    target: menus::Target::Project(id),
                    command: menus::Command::Open,
                },
                window,
                cx,
            );
        }
        Target::Host(node) => shell.live_resource_action(
            &menus::Dispatch {
                node,
                target: menus::Target::Host,
                command: menus::Command::Open,
            },
            window,
            cx,
        ),
        Target::PreviewSession(key) => shell.select_session(key, window, cx),
        Target::PreviewProject(id) => shell.select_project(id, window, cx),
        Target::PreviewHost(host) => shell.select_host(host, window, cx),
        Target::Feature(feature) => feature.open(shell, window, cx),
        Target::Setting(section) => {
            shell.navigate(Page::Settings, window, cx);
            shell
                .settings
                .update(cx, |settings, cx| settings.select(section, cx));
        }
        Target::PluginSetting(entry) => {
            shell.navigate(Page::Settings, window, cx);
            shell.settings_target = Some(entry.node);
            shell.settings.update(cx, |settings, cx| {
                settings.open_plugin_settings(&entry.package.name, cx)
            });
        }
        Target::Folder(_) => {}
    }
}
