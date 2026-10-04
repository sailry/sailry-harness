use crate::tr;
use gpui_kit::SharedString;
use std::collections::BTreeMap;

mod removal;
pub(super) use removal::Removal;
mod project_removal;
pub(super) use project_removal::ProjectRemoval;
mod repository;
pub(crate) use repository::Repository;
mod cli;
pub(crate) use cli::Profile;
pub(crate) mod runtime;
pub(crate) mod tunnels;
pub(crate) mod updates;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(crate) type Key = (usize, usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub host: usize,
    pub project: usize,
    pub worktree: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Project {
    pub host: usize,
    pub name: SharedString,
    pub path: SharedString,
    pub active_worktree: usize,
    pub trusted: bool,
    pub repository: Repository,
    pub appearance: sailry_protocol::projects::Appearance,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Worktree {
    pub project: usize,
    pub branch: SharedString,
    pub path: SharedString,
    pub base_ref: Option<SharedString>,
    pub main: bool,
    pub locked: bool,
    pub dirty: bool,
    pub busy: bool,
}

pub(crate) struct Session {
    pub owner: Owner,
    pub title: SharedString,
    pub pinned: bool,
    pub archived: bool,
}

pub(crate) struct Terminal {
    pub owner: Owner,
    pub title: SharedString,
    pub resets: usize,
    pub profile: Option<Profile>,
}

// Fixture ownership lives here; editor entities and drafts remain presentation state.
// Replace these records with Client projections when the service boundary is connected.
pub(crate) struct State {
    pub projects: BTreeMap<usize, Project>,
    pub worktrees: BTreeMap<usize, Worktree>,
    pub sessions: BTreeMap<Key, Session>,
    pub terminals: BTreeMap<Key, Terminal>,
    pub terminal: Option<Key>,
    pub cli_profiles: BTreeMap<usize, Vec<Profile>>,
    pub tunnels: BTreeMap<usize, tunnels::State>,
    pub updates: BTreeMap<usize, updates::State>,
    pub runtimes: BTreeMap<usize, runtime::Snapshot>,
    selected: BTreeMap<usize, usize>,
    next_session: BTreeMap<usize, usize>,
    next_terminal: BTreeMap<usize, usize>,
    next_worktree: usize,
    next_project: usize,
}

impl Default for State {
    fn default() -> Self {
        let mut state = Self {
            projects: BTreeMap::new(),
            worktrees: BTreeMap::new(),
            sessions: BTreeMap::new(),
            terminals: BTreeMap::new(),
            terminal: None,
            cli_profiles: cli::fixtures(),
            tunnels: tunnels::fixtures(),
            updates: BTreeMap::from([(1, updates::State::sample(0))]),
            runtimes: BTreeMap::new(),
            selected: BTreeMap::new(),
            next_session: BTreeMap::new(),
            next_terminal: BTreeMap::new(),
            next_worktree: 4,
            next_project: 2,
        };
        for host in 0..2 {
            let path: SharedString = if host == 0 {
                "/preview/sailry"
            } else {
                "/preview/remote-workspace"
            }
            .into();
            state.projects.insert(
                host,
                Project {
                    host,
                    name: tr(if host == 0 {
                        "project"
                    } else {
                        "remote_project"
                    }),
                    path: path.clone(),
                    active_worktree: host,
                    trusted: true,
                    repository: Repository::Ready,
                    appearance: Default::default(),
                },
            );
            state.selected.insert(host, host);
            for session in 0..2 {
                let worktree = host + session * 2;
                state.worktrees.insert(
                    worktree,
                    Worktree {
                        project: host,
                        branch: tr(if session == 0 {
                            "composer_branch_main"
                        } else {
                            "composer_branch_preview"
                        }),
                        base_ref: None,
                        main: session == 0,
                        locked: false,
                        dirty: false,
                        busy: false,
                        path: if session == 0 {
                            path.clone()
                        } else {
                            format!("{path}/.worktrees/preview").into()
                        },
                    },
                );
                state.sessions.insert(
                    (host, session),
                    Session {
                        owner: Owner {
                            host,
                            project: host,
                            worktree,
                        },
                        title: tr(if host == 1 {
                            "remote_session"
                        } else if session == 0 {
                            "session"
                        } else {
                            "session_long"
                        }),
                        pinned: false,
                        archived: false,
                    },
                );
            }
            state.next_session.insert(host, 3);
            state.next_terminal.insert(host, 0);
            state.create_terminal(state.owner(host));
        }
        state
    }
}

impl State {
    pub fn save_project(
        &mut self,
        host: usize,
        existing: Option<(usize, &Project)>,
        name: &str,
        path: &str,
    ) -> Result<usize, &'static str> {
        if !self.next_session.contains_key(&host) {
            return Err("project_missing");
        }
        let (name, path) = (name.trim(), path.trim());
        if name.is_empty() || path.is_empty() {
            return Err("project_required");
        }
        let id = existing.map(|(id, _)| id);
        if self.projects.iter().any(|(&key, project)| {
            Some(key) != id
                && project.host == host
                && project.path.trim_end_matches('/') == path.trim_end_matches('/')
        }) {
            return Err("project_duplicate_path");
        }
        if let Some((id, original)) = existing {
            let current = self.projects.get(&id).ok_or("project_missing")?;
            if current.host != host
                || current.name != original.name
                || current.path != original.path
            {
                return Err("project_changed");
            }
            if current.path != path {
                if current.trusted {
                    return Err("project_path_trusted");
                }
                if self
                    .sessions
                    .values()
                    .any(|session| session.owner.project == id)
                    || self
                        .terminals
                        .values()
                        .any(|terminal| terminal.owner.project == id)
                {
                    return Err("project_path_referenced");
                }
                self.worktrees.retain(|_, tree| tree.project != id);
                let worktree = self.root_worktree(id, path);
                let project = self.projects.get_mut(&id).unwrap();
                project.active_worktree = worktree;
                project.path = path.to_owned().into();
                project.repository = Repository::Directory;
            }
            self.projects.get_mut(&id).unwrap().name = name.to_owned().into();
            return Ok(id);
        }
        let id = self
            .next_project
            .max(self.projects.last_key_value().map_or(0, |(&id, _)| id + 1));
        self.next_project = id + 1;
        let worktree = self.root_worktree(id, path);
        self.projects.insert(
            id,
            Project {
                host,
                name: name.to_owned().into(),
                path: path.to_owned().into(),
                active_worktree: worktree,
                trusted: false,
                repository: Repository::Directory,
                appearance: Default::default(),
            },
        );
        Ok(id)
    }

    fn root_worktree(&mut self, project: usize, path: &str) -> usize {
        let id = self
            .next_worktree
            .max(self.worktrees.last_key_value().map_or(0, |(&id, _)| id + 1));
        self.next_worktree = id + 1;
        self.worktrees.insert(
            id,
            Worktree {
                project,
                branch: SharedString::default(),
                path: path.to_owned().into(),
                base_ref: None,
                main: true,
                locked: false,
                dirty: false,
                busy: false,
            },
        );
        id
    }

    pub fn add_worktree(
        &mut self,
        project: usize,
        base_ref: &str,
        branch: &str,
        path: &str,
    ) -> Result<Owner, &'static str> {
        let project_record = self
            .projects
            .get(&project)
            .ok_or("worktree_missing_project")?;
        if let Some(reason) = self.new_worktree_reason(project) {
            return Err(reason);
        }
        let (base_ref, branch, path) = (base_ref.trim(), branch.trim(), path.trim());
        if base_ref.is_empty() || branch.is_empty() || path.is_empty() {
            return Err("worktree_required");
        }
        // Preview validation only: the execution Node must validate Git refs and filesystem paths.
        if self
            .worktrees
            .values()
            .any(|tree| tree.project == project && tree.branch == branch)
        {
            return Err("worktree_duplicate_branch");
        }
        if self.worktrees.values().any(|tree| {
            self.projects[&tree.project].host == project_record.host
                && tree.path.trim_end_matches('/') == path.trim_end_matches('/')
        }) {
            return Err("worktree_duplicate_path");
        }
        let id = self
            .next_worktree
            .max(self.worktrees.last_key_value().map_or(0, |(&id, _)| id + 1));
        self.next_worktree = id + 1;
        let owner = Owner {
            host: project_record.host,
            project,
            worktree: id,
        };
        self.worktrees.insert(
            id,
            Worktree {
                project,
                branch: branch.to_owned().into(),
                path: path.to_owned().into(),
                base_ref: Some(base_ref.to_owned().into()),
                main: false,
                locked: false,
                dirty: false,
                busy: false,
            },
        );
        Ok(owner)
    }

    pub fn owner(&self, host: usize) -> Owner {
        self.selected_owner(host)
            .expect("selected preview project exists")
    }

    pub fn selected_owner(&self, host: usize) -> Option<Owner> {
        self.project_owner(*self.selected.get(&host)?)
    }

    pub fn project_owner(&self, project: usize) -> Option<Owner> {
        let record = self.projects.get(&project)?;
        Some(Owner {
            host: record.host,
            project,
            worktree: record.active_worktree,
        })
    }

    pub fn contains(&self, owner: Owner) -> bool {
        self.projects
            .get(&owner.project)
            .is_some_and(|project| project.host == owner.host)
            && self
                .worktrees
                .get(&owner.worktree)
                .is_some_and(|worktree| worktree.project == owner.project)
    }

    pub fn select(&mut self, owner: Owner) -> bool {
        if !self.contains(owner) {
            return false;
        }
        self.selected.insert(owner.host, owner.project);
        self.projects
            .get_mut(&owner.project)
            .unwrap()
            .active_worktree = owner.worktree;
        true
    }

    pub fn ensure_session(&mut self, key: Key) {
        let owner = self.owner(key.0);
        self.sessions.entry(key).or_insert_with(|| Session {
            owner,
            title: tr("new_session"),
            pinned: false,
            archived: false,
        });
        let next = self.next_session.entry(key.0).or_insert(3);
        *next = (*next).max(key.1 + 1);
    }

    pub fn create_session(&mut self, owner: Owner) -> Key {
        assert!(
            self.contains(owner),
            "session must belong to an existing worktree"
        );
        let next = self.next_session.entry(owner.host).or_insert(3);
        let key = (owner.host, *next);
        *next += 1;
        self.sessions.insert(
            key,
            Session {
                owner,
                title: tr("new_session"),
                pinned: false,
                archived: false,
            },
        );
        key
    }

    pub fn create_terminal(&mut self, owner: Owner) -> Key {
        assert!(
            self.contains(owner),
            "terminal must belong to an existing worktree"
        );
        let next = self.next_terminal.entry(owner.host).or_insert(0);
        let key = (owner.host, *next);
        *next += 1;
        self.terminals.insert(
            key,
            Terminal {
                owner,
                title: format!("{} {}", tr("terminal"), key.1 + 1).into(),
                resets: 0,
                profile: None,
            },
        );
        key
    }
}
