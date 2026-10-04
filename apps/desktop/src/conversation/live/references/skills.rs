//! Skills share the existing package metadata and ADK load_skill execution path.
use super::*;
use sailry_protocol::plugin;

#[cfg(test)]
mod tests;

impl View {
    pub(super) fn skill_packages(&self) -> Vec<plugin::Reference> {
        if !self.composer_options.skills {
            return Vec::new();
        }
        let installed = self
            .configuration()
            .snapshot
            .iter()
            .flat_map(|snapshot| &snapshot.plugins);
        installed
            .filter(|package| package.enabled)
            .filter_map(|package| {
                match self
                    .session
                    .as_ref()
                    .and_then(|session| session.config.assistant.as_ref())
                {
                    Some(binding) if binding.package.name == package.name => {
                        Some(binding.package.clone())
                    }
                    Some(_) => None,
                    None => Some(package.reference()),
                }
            })
            .collect()
    }

    pub(super) fn load_skills(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.references.stop.cancel();
        self.references.generation += 1;
        let packages = self.skill_packages();
        self.references.loading = !packages.is_empty();
        self.references.error = false;
        self.references.rows = self.reference_catalog(&self.references.page, cx);
        if packages.is_empty() {
            self.reference_rows(window, cx);
            return;
        }
        let client = if self.config_owner == self.binding.client.target() {
            self.binding.client.clone()
        } else {
            self.binding.defaults.clone()
        };
        let stop = self.stop.child_token();
        self.references.stop = stop.clone();
        let generation = self.references.generation;
        let page = self.references.page.clone();
        let task = self.binding.runtime.spawn(async move {
            let mut rows = Vec::new();
            let mut failed = false;
            for package in packages {
                let result = tokio::select! {
                    _ = stop.cancelled() => return None,
                    result = client.execute(client.prepare(Command::ReadPluginVersion { package: package.clone() })) => result,
                };
                match result {
                    Ok(Output::Plugin(info)) => {
                        if info.extension.as_ref().is_some_and(|extension| {
                            extension.scope == plugin::Scope::Desktop
                        }) {
                            continue;
                        }
                        match &page {
                            Page::Root => {
                                if info.skill.is_none() && (!info.skills.is_empty() || !info.mcp.is_empty()
                                    || info.extension.as_ref().is_some_and(|extension| {
                                        !extension.tools.is_empty()
                                            || extension.actions.iter().any(|action| matches!(
                                                action,
                                                plugin::Action::ReadComputer | plugin::Action::ControlComputer
                                            ))
                                    })) {
                                    rows.push(Item::Plugin(Box::new(info)));
                                }
                            }
                            Page::Commands => rows.extend(info.skills.into_iter().map(|skill| {
                                Item::Command(commands::Choice::Skill(package.clone(), Box::new(skill)))
                            })),
                            _ => {}
                        }
                    }
                    _ => failed = true,
                }
            }
            Some((rows, failed))
        });
        self.references.task = Some(cx.spawn_in(window, async move |view, cx| {
            let Ok(Some((rows, failed))) = task.await else {
                return;
            };
            _ = view.update_in(cx, |view, window, cx| {
                if !view.references.open || view.references.generation != generation {
                    return;
                }
                view.references.loading = false;
                view.references.error = failed;
                view.references.rows.extend(rows);
                view.reference_rows(window, cx);
                cx.notify();
            });
        }));
    }
}
