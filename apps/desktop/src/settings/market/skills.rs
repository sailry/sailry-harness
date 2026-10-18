use super::*;
use crate::settings::skills::install;
use sailry_protocol::plugin::{Info, skillcatalog};

fn installed(entry: &skillcatalog::Entry, info: &Info) -> bool {
    (info.summary.name == entry.package
        && info
            .skills
            .iter()
            .any(|skill| skill.path == entry.skill.path))
        || info
            .skill
            .as_ref()
            .and_then(|source| source.git())
            .is_some_and(|source| {
                same_repository(&source.source.repository, &entry.source.repository)
                    && entry
                        .source
                        .path
                        .as_deref()
                        .is_some_and(|path| path == source.path)
            })
}

impl Market {
    pub(super) fn load_skills(&mut self, force: bool, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().trim().to_owned();
        let stop = self.begin(Loading::Catalog);
        let cancelled = stop.clone();
        let binding = self.binding.clone();
        let job=binding.runtime.spawn(async move {
            tokio::select! {biased;
                _=cancelled.cancelled()=>None,
                result=binding.client.execute(binding.client.prepare(Command::SearchSkillCatalog {query,force}))=>Some(result),
            }
        });
        self.task = Some(cx.spawn(async move |state, cx| {
            let result = job.await;
            _ = state.update(cx, |state, cx| {
                if stop.is_cancelled() {
                    return;
                }
                state.pending = None;
                match result {
                    Ok(Some(Ok(Output::SkillCatalog(page)))) => {
                        state.error = page.unavailable.then_some("plugins_market_failed");
                        state.skills = Some(page);
                    }
                    _ => state.error = Some("plugins_market_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn skill_install(
        &self,
        source: sailry_protocol::plugin::skills::Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.connected(cx) || self.busy(cx) {
            return;
        }
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        install::open_catalog(
            self.binding.clone(),
            owner.read(cx).plugin_catalog.packages.clone(),
            source,
            window,
            cx,
        );
    }

    fn skill_link(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.connected(cx) {
            return;
        }
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let channel = match self.filter.skills {
            filter::Skills::Directory => install::Channel::Skills,
            _ => install::Channel::Hub,
        };
        install::open_channel(
            self.binding.clone(),
            owner.read(cx).plugin_catalog.packages.clone(),
            channel,
            window,
            cx,
        );
    }

    pub(super) fn skill_cards(&self, cx: &mut Context<Self>) -> AnyElement {
        if matches!(
            self.filter.skills,
            filter::Skills::Directory | filter::Skills::Hub
        ) {
            if self.filter.installed {
                return self.empty(cx);
            }
            return Button::new("market-skill-link")
                .outline()
                .w_full()
                .h_auto()
                .py_4()
                .label(tr("market_paste_link"))
                .icon(gpui_kit::assets::IconName::Link)
                .disabled(!self.connected(cx))
                .on_click(cx.listener(|state, _, window, cx| state.skill_link(window, cx)))
                .into_any_element();
        }
        let Some(page) = &self.skills else {
            return if self.pending.is_some() {
                card::skeleton(cx)
            } else {
                self.empty(cx)
            };
        };
        let owner = self.owner.upgrade();
        let metadata = owner
            .as_ref()
            .and_then(|owner| owner.read(cx).plugin_catalog.metadata.as_ref());
        let visible: Vec<_> = page
            .entries
            .iter()
            .filter_map(|entry| {
                let installed = metadata.is_some_and(|metadata| {
                    metadata
                        .read(cx)
                        .entries
                        .values()
                        .any(|info| installed(entry, info))
                });
                (!self.filter.installed || installed).then_some((entry, installed))
            })
            .collect();
        let busy = self.busy(cx);
        div()
            .grid()
            .grid_cols(2)
            .gap_3()
            .when(visible.is_empty(), |view| view.child(self.empty(cx)))
            .children(visible.into_iter().map(|(entry, installed)| {
                let title = entry
                    .skill
                    .display_locales
                    .get::<str>(&rust_i18n::locale())
                    .or(entry.skill.display_name.as_ref())
                    .unwrap_or(&entry.skill.name)
                    .clone();
                let summary = card::content(
                    title.into(),
                    entry.skill.description.clone(),
                    None,
                    Some(tr("market_skill_official")),
                    cx,
                );
                let action = card::action(
                    format!("market-skill-install-{}", entry.id),
                    installed,
                    false,
                    cx,
                )
                .disabled(busy || installed)
                .on_click(cx.listener({
                    let source = entry.source.clone();
                    move |state, _, window, cx| state.skill_install(source.clone(), window, cx)
                }));
                card::row(
                    format!("market-skill-card-{}", entry.id),
                    summary,
                    action,
                    cx,
                )
            }))
            .into_any_element()
    }
}

fn same_repository(left: &str, right: &str) -> bool {
    let canonical = |value: &str| {
        value
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_owned()
    };
    canonical(left) == canonical(right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn matches_canonical_installation_origins() {
        assert!(same_repository(
            "https://github.com/example/skills",
            "https://github.com/example/skills.git"
        ));
        assert!(same_repository(
            "https://github.com/example/skills.git/",
            "https://github.com/example/skills"
        ));
        for source in [
            "https://example.com/example/skills.git",
            "https://github.com/example/other.git",
            "https://github.com/example/skills.git-backup",
        ] {
            assert!(!same_repository(
                source,
                "https://github.com/example/skills"
            ));
        }
    }
}
