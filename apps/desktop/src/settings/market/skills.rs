use super::*;
use crate::settings::skills::install;
use sailry_protocol::plugin::{Info, skillcatalog};

fn installed(entry: &skillcatalog::Entry, info: &Info) -> bool {
    if entry.package.as_deref() == Some(&info.summary.name)
        && info.skills.iter().any(|skill| skill.name == entry.name)
    {
        return true;
    }
    match (&entry.source, info.skill.as_ref()) {
        (
            skillcatalog::InstallSource::Git(source),
            Some(sailry_protocol::plugin::skills::Provenance::Git(installed)),
        ) => {
            if let Some((repository, name)) = directory_identity(&source.repository) {
                same_repository(&installed.source.repository, &repository)
                    && (info.skills.iter().any(|skill| skill.name == name)
                        || installed.path.rsplit('/').next() == Some(name.as_str()))
            } else {
                same_repository(&installed.source.repository, &source.repository)
                    && source.path.as_deref() == Some(&installed.path)
            }
        }
        (
            skillcatalog::InstallSource::Registry(source),
            Some(sailry_protocol::plugin::skills::Provenance::Registry { registry }),
        ) => {
            source
                .namespace
                .as_ref()
                .is_none_or(|namespace| *namespace == registry.namespace)
                && source.slug == registry.slug
                && source
                    .version
                    .as_ref()
                    .is_none_or(|version| *version == registry.version)
        }
        _ => false,
    }
}

impl Market {
    pub(super) fn load_skills(&mut self, page: u32, force: bool, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().trim().to_owned();
        let source = self.filter.skills.source();
        let stop = self.begin(Loading::Catalog);
        let cancelled = stop.clone();
        let binding = self.binding.clone();
        let job=binding.runtime.spawn(async move {
            tokio::select! {biased;
                _=cancelled.cancelled()=>None,
                result=binding.client.execute(binding.client.prepare(Command::SearchSkillCatalog {source,query,page,force}))=>Some(result),
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
        source: skillcatalog::InstallSource,
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

    pub(super) fn skill_cards(&self, cx: &mut Context<Self>) -> AnyElement {
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
        let previous = page.page.saturating_sub(1);
        let next = page.next;
        v_flex()
            .gap_3()
            .when(page.query_scope == skillcatalog::QueryScope::Page, |view| {
                view.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("market_page_filter")),
                )
            })
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_3()
                    .when(visible.is_empty(), |view| view.child(self.empty(cx)))
                    .children(visible.into_iter().map(|(entry, installed)| {
                        let title = entry.title(&rust_i18n::locale()).to_owned();
                        let summary = card::content(
                            title.into(),
                            entry
                                .description(&rust_i18n::locale())
                                .unwrap_or_default()
                                .to_owned(),
                            None,
                            Some(self.filter.skills.label()),
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
                            move |state, _, window, cx| {
                                cx.stop_propagation();
                                state.skill_install(source.clone(), window, cx)
                            }
                        }));
                        card::row(
                            format!("market-skill-card-{}", entry.id),
                            summary,
                            action,
                            cx,
                        )
                    })),
            )
            .when(page.page > 1 || next.is_some(), |view| {
                view.child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("market-skills-previous")
                                .ghost()
                                .small()
                                .icon(IconName::ChevronLeft)
                                .accessibility_label(tr("plugins_previous"))
                                .tooltip(tr("plugins_previous"))
                                .disabled(busy || page.page <= 1)
                                .on_click(cx.listener(move |state, _, _, cx| {
                                    state.search(previous, false, cx)
                                })),
                        )
                        .child(
                            Button::new("market-skills-next")
                                .ghost()
                                .small()
                                .icon(IconName::ChevronRight)
                                .accessibility_label(tr("plugins_next"))
                                .tooltip(tr("plugins_next"))
                                .disabled(busy || next.is_none())
                                .on_click(cx.listener(move |state, _, _, cx| {
                                    if let Some(next) = next {
                                        state.search(next, false, cx);
                                    }
                                })),
                        ),
                )
            })
            .into_any_element()
    }
}

fn directory_identity(source: &str) -> Option<(String, String)> {
    let url = url::Url::parse(source).ok()?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("skills.sh" | "www.skills.sh"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || matches!(*part, "." | "..")
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
    {
        return None;
    }
    Some((
        format!("https://github.com/{}/{}", parts[0], parts[1]),
        parts[2].into(),
    ))
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
    fn identifies_directory_origins() {
        assert_eq!(
            directory_identity("https://skills.sh/example/skills/review"),
            Some(("https://github.com/example/skills".into(), "review".into()))
        );
        for value in [
            "https://skills.sh.evil/example/skills/review",
            "http://skills.sh/example/skills/review",
            "https://skills.sh/example/skills/review?token=secret",
            "https://skills.sh/example/skills/%2e%2e",
        ] {
            assert_eq!(directory_identity(value), None);
        }
    }

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
