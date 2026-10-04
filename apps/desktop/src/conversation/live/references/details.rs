//! Mention links follow bound resource routes or shared read-only details.
use super::*;
use crate::ui::details::open as show;

impl View {
    pub(in crate::conversation::live) fn open_reference(
        &mut self,
        reference: Reference,
        turn: Option<TurnId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (kind, detail) = match &reference.target {
            Target::Skill { package, name } => {
                self.open_skill(package, name, window, cx);
                return;
            }
            Target::Plugin(name) => {
                self.open_plugin(name, window, cx);
                return;
            }
            Target::File(path) => {
                if turn.is_some() {
                    if let Some(worktree) = self.reference_worktree(turn) {
                        cx.emit(Event::FileAt(worktree, path.clone()));
                    } else {
                        crate::feedback::error("", &tr("reference_unavailable"), window, cx);
                    }
                } else {
                    cx.emit(Event::File(path.clone()));
                }
                return;
            }
            Target::Directory(path) => {
                if let Some(worktree) = self.reference_worktree(turn) {
                    cx.emit(Event::DirectoryAt(worktree, path.clone()));
                } else {
                    crate::feedback::error("", &tr("reference_unavailable"), window, cx);
                }
                return;
            }
            Target::Session(id) => {
                if self.node.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.sessions.iter().any(|session| session.id == *id)
                }) {
                    cx.emit(Event::Session(*id));
                } else {
                    crate::feedback::error("", &tr("reference_unavailable"), window, cx);
                }
                return;
            }
            Target::Host => {
                cx.emit(Event::HostPage(self.binding.client.target()));
                return;
            }
            Target::Project => {
                let project = if turn.is_some() {
                    self.reference_worktree(turn).and_then(|worktree| {
                        self.node
                            .snapshot
                            .as_ref()?
                            .worktrees
                            .iter()
                            .find(|tree| tree.id == worktree)?
                            .project
                    })
                } else {
                    self.binding.project
                };
                if let Some(project) = project
                    && self.node.snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.projects.iter().any(|item| item.id == project)
                    })
                {
                    cx.emit(Event::ProjectPage(self.binding.client.target(), project));
                } else {
                    crate::feedback::error("", &tr("reference_unavailable"), window, cx);
                }
                return;
            }
            Target::Agent(role) => {
                let Some(profile) = self
                    .reference_roles()
                    .iter()
                    .find(|profile| profile.reference() == *role)
                else {
                    crate::feedback::error("", &tr("reference_unavailable"), window, cx);
                    return;
                };
                (
                    "reference_agents",
                    format!("{}\n\n{}", profile.description, profile.instructions),
                )
            }
            Target::Ssh(id) => {
                let Some(profile) = self
                    .reference_ssh()
                    .into_iter()
                    .find(|profile| profile.id == *id)
                else {
                    crate::feedback::error("", &tr("reference_unavailable"), window, cx);
                    return;
                };
                (
                    "reference_ssh",
                    format!("{}@{}:{}", profile.username, profile.host, profile.port),
                )
            }
            Target::Database {
                connection,
                database,
                table,
            } => {
                let Some(profile) = self
                    .reference_databases()
                    .into_iter()
                    .find(|profile| profile.id == *connection)
                else {
                    crate::feedback::error("", &tr("reference_unavailable"), window, cx);
                    return;
                };
                let mut parts = vec![profile.name.clone()];
                parts.extend(database.clone());
                parts.extend(
                    table
                        .as_ref()
                        .map(|table| format!("{}.{}", table.schema, table.name)),
                );
                ("reference_databases", parts.join(" / "))
            }
            Target::Worktree => {
                let tree = self.reference_worktree(turn).and_then(|worktree| {
                    self.node
                        .snapshot
                        .as_ref()?
                        .worktrees
                        .iter()
                        .find(|item| item.id == worktree)
                });
                let Some(tree) = tree else {
                    crate::feedback::error("", &tr("reference_unavailable"), window, cx);
                    return;
                };
                (
                    "reference_worktree",
                    if turn.is_some() {
                        tree.path.clone()
                    } else {
                        self.binding.branch.to_string()
                    },
                )
            }
        };
        show(reference.label, tr(kind).to_string(), detail, window, cx);
    }

    fn reference_worktree(&self, turn: Option<TurnId>) -> Option<WorktreeId> {
        match turn {
            None => self.binding.worktree,
            Some(turn) => self.turn_worktree(turn).or_else(|| {
                self.outgoing
                    .as_ref()
                    .filter(|message| message.row == turn)?
                    .worktree
            }),
        }
    }

    fn open_plugin(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(expected) = self
            .configuration()
            .snapshot
            .iter()
            .flat_map(|snapshot| &snapshot.plugins)
            .find(|plugin| plugin.name == name)
            .cloned()
        else {
            self.error = Some("reference_stale");
            cx.notify();
            return;
        };
        let client = if self.config_owner == self.binding.client.target() {
            self.binding.client.clone()
        } else {
            self.binding.defaults.clone()
        };
        let label = self
            .hosts
            .iter()
            .find(|(node, _)| *node == client.target())
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| self.binding.host.clone());
        crate::settings::open_plugin_details(
            client,
            self.binding.runtime.clone(),
            label,
            expected,
            window,
            cx,
        );
    }

    pub(super) fn open_skill(
        &mut self,
        name: &str,
        skill: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(package) = self
            .skill_packages()
            .into_iter()
            .find(|package| package.name == name)
        else {
            self.error = Some("reference_stale");
            cx.notify();
            return;
        };
        let client = if self.config_owner == self.binding.client.target() {
            self.binding.client.clone()
        } else {
            self.binding.defaults.clone()
        };
        let expected = package.clone();
        let skill = skill.to_owned();
        let stop = self.stop.child_token();
        let task = self.binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = client.execute(client.prepare(Command::ReadPluginVersion { package })) => Some(result),
            }
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            _ = view.update_in(cx, |view, window, cx| {
                if !view.skill_packages().contains(&expected) {
                    view.error = Some("reference_stale");
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Some(Ok(Output::Plugin(info)))) => {
                        if let Some(skill) = info.skills.into_iter().find(|item| item.name == skill)
                        {
                            show(
                                crate::plugins::metadata::skill_title(&skill),
                                expected.name,
                                skill.description,
                                window,
                                cx,
                            );
                        } else {
                            view.error = Some("reference_stale");
                        }
                    }
                    _ => view.error = Some("plugins_read_failed"),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
