use super::*;
pub(super) mod cache;
use crate::settings::skills::install;
use gpui_kit::{assets::IconName as Glyph, component::avatar::Avatar};
use sailry_protocol::plugin::{Info, skillcatalog};

pub(super) fn installed(entry: &skillcatalog::Entry, info: &Info) -> bool {
    if entry.package.as_deref() == Some(&info.summary.name)
        && info.skills.iter().any(|skill| skill.name == entry.name)
    {
        return true;
    }
    match (entry.source.as_ref(), info.skill.as_ref()) {
        (
            Some(skillcatalog::InstallSource::Git(source)),
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
            Some(skillcatalog::InstallSource::Registry(source)),
            Some(sailry_protocol::plugin::skills::Provenance::Registry { registry }),
        ) => {
            source
                .namespace
                .as_ref()
                .is_none_or(|namespace| *namespace == registry.namespace)
                && source.slug == registry.slug
        }
        _ => false,
    }
}

impl Market {
    pub(super) fn installed_skill(&self, entry: &skillcatalog::Entry, cx: &App) -> Option<Info> {
        self.owner.upgrade().and_then(|owner| {
            if owner
                .read(cx)
                .provider_link
                .as_ref()
                .is_none_or(|live| live.binding.client.target() != self.binding.client.target())
            {
                return None;
            }
            let metadata = owner.read(cx).plugin_catalog.metadata.as_ref()?;
            let entries = &metadata.read(cx).entries;
            entries
                .values()
                .filter(|info| {
                    owner
                        .read(cx)
                        .plugin_catalog
                        .packages
                        .contains(&info.summary)
                })
                .find(|info| {
                    entry.package.as_deref() == Some(&info.summary.name) && installed(entry, info)
                })
                .or_else(|| {
                    entries.values().find(|info| {
                        owner
                            .read(cx)
                            .plugin_catalog
                            .packages
                            .contains(&info.summary)
                            && installed(entry, info)
                    })
                })
                .cloned()
        })
    }
    pub(super) fn load_skills(&mut self, requested: u32, force: bool, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().trim().to_owned();
        let source = self.filter.skills.source();
        let stop = self.begin(Loading::Catalog);
        let cancelled = stop.clone();
        let binding = self.binding.clone();
        let through = if requested == 1 {
            self.skills.as_ref().map_or(1, |page| page.page)
        } else {
            requested
        };
        let job=binding.runtime.spawn(async move {
            tokio::select! {biased;
                _=cancelled.cancelled()=>None,
                result=refresh(requested, through, |page| {
                    let client = binding.client.clone();
                    let query = query.clone();
                    async move {
                        match client.execute(client.prepare(Command::SearchSkillCatalog {source,query,page,force})).await? {
                            Output::SkillCatalog(page) => Ok(page),
                            _ => Err(sailry_protocol::Fault::new(sailry_protocol::ErrorCode::Internal, "unexpected skill catalog response")),
                        }
                    }
                })=>Some(result.map(Output::SkillCatalog)),
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
                    Ok(Some(Ok(Output::SkillCatalog(mut page)))) => {
                        state.error = page.unavailable.then_some("market_skills_failed");
                        if page.unavailable {
                            state.paging.failed = Some(requested);
                        } else {
                            state.paging.received(requested);
                        }
                        if requested == 1 {
                            if !page.unavailable || state.skills.is_none() {
                                state.skills = Some(page);
                            }
                        } else if !page.unavailable
                            && let Some(current) = &mut state.skills
                        {
                            if page.page > current.page {
                                paging::append(
                                    &mut current.entries,
                                    std::mem::take(&mut page.entries),
                                    |entry| &entry.id,
                                );
                                current.page = page.page;
                                current.next = page.next;
                                current.total = page.total;
                                current.query_scope = page.query_scope;
                            } else {
                                current.next = None;
                            }
                        }
                    }
                    _ => {
                        state.error = Some("market_skills_failed");
                        state.paging.failed = Some(requested);
                    }
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

    pub(super) fn skill_cards(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(page) = &self.skills else {
            return if self.pending.is_some() {
                card::skeleton(cx)
            } else {
                self.empty(cx)
            };
        };
        let mut visible: Vec<_> = page
            .entries
            .iter()
            .filter_map(|entry| {
                let current = self.installed_skill(entry, cx);
                (!self.filter.installed || current.is_some()).then_some((entry, current))
            })
            .collect();
        self.paging.count(visible.len());
        visible.truncate(self.paging.limit);
        let busy = self.busy(cx);
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
            .when(visible.is_empty(), |view| view.child(self.empty(cx)))
            .child(
                div()
                    .w_full()
                    .grid()
                    .grid_cols(2)
                    .gap_3()
                    .children(visible.into_iter().map(|(entry, current)| {
                        let title = entry.title(&rust_i18n::locale()).to_owned();
                        let summary = card::content(
                            title.into(),
                            entry
                                .description(&rust_i18n::locale())
                                .unwrap_or_default()
                                .to_owned(),
                            Some(icon(&entry.id, entry.icon.clone(), cx)),
                            Some(entry.origin.clone().into()),
                            cx,
                        );
                        let update = current.as_ref().is_some_and(|info| {
                            self.updates.read(cx).candidate(&info.summary).is_some()
                        });
                        let operation = card::Action::new(current.is_some(), update);
                        let action = if entry.source.is_some() || current.is_some() {
                            card::action(
                                format!("market-skill-install-{}", entry.id),
                                operation,
                                cx,
                            )
                            .disabled(
                                !self.connected(cx)
                                    || operation == card::Action::Installed
                                    || match operation {
                                        card::Action::Install => busy,
                                        card::Action::Update => {
                                            self.updates.read(cx).busy()
                                                || self.updates.read(cx).selecting()
                                        }
                                        card::Action::Installed => false,
                                    },
                            )
                            .on_click(cx.listener({
                                let source = entry.source.clone();
                                let entry = entry.clone();
                                let package =
                                    current.as_ref().map(|info| info.summary.name.clone());
                                move |state, _, window, cx| {
                                    cx.stop_propagation();
                                    match operation {
                                        card::Action::Update => {
                                            if let Some(package) = &package {
                                                state.update(package, window, cx)
                                            }
                                        }
                                        card::Action::Install => {
                                            if state.installed_skill(&entry, cx).is_none()
                                                && let Some(source) = &source
                                            {
                                                state.skill_install(source.clone(), window, cx)
                                            }
                                        }
                                        card::Action::Installed => (),
                                    }
                                }
                            }))
                            .into_any_element()
                        } else {
                            div().into_any_element()
                        };
                        card::row(
                            format!("market-skill-card-{}", entry.id),
                            summary,
                            action,
                            cx,
                        )
                        .accessibility_label(entry.title(&rust_i18n::locale()).to_owned())
                        .on_click(cx.listener({
                            let entry = entry.clone();
                            move |state, _, window, cx| {
                                let selection = state.begin_details();
                                let market = cx.entity();
                                let entry = entry.clone();
                                window.defer(cx, move |window, cx| {
                                    details::skills::open(market, entry, selection, window, cx)
                                });
                            }
                        }))
                    })),
            )
            .into_any_element()
    }
}

async fn refresh<F, T>(
    requested: u32,
    through: u32,
    mut load: F,
) -> Result<skillcatalog::Page, sailry_protocol::Fault>
where
    F: FnMut(u32) -> T,
    T: std::future::Future<Output = Result<skillcatalog::Page, sailry_protocol::Fault>>,
{
    let mut current = load(requested).await?;
    while !current.unavailable
        && let Some(next) = current
            .next
            .filter(|next| *next > current.page && *next <= through)
    {
        let mut page = load(next).await?;
        if page.unavailable {
            current.unavailable = true;
            break;
        }
        if page.page != next {
            return Err(sailry_protocol::Fault::invalid(
                "skill catalog page did not advance",
            ));
        }
        paging::append(
            &mut current.entries,
            std::mem::take(&mut page.entries),
            |entry| &entry.id,
        );
        current.page = page.page;
        current.next = page.next;
        current.total = page.total;
        current.query_scope = page.query_scope;
    }
    Ok(current)
}

fn icon(id: &str, source: Option<String>, cx: &App) -> AnyElement {
    let foreground = cx.theme().muted_foreground;
    let placeholder = move || {
        Avatar::new()
            .with_size(px(40.))
            .rounded(px(11.))
            .placeholder(Icon::new(Glyph::Box).size(px(22.)).text_color(foreground))
            .into_any_element()
    };
    match source {
        // Kit Avatar's image slot does not fall back after loading fails.
        // GPUI's image loader retains the same Kit placeholder in both states.
        Some(source) => img(source)
            .id(SharedString::from(format!("market-skill-icon-{id}")))
            .size(px(40.))
            .flex_shrink_0()
            .rounded(px(11.))
            .with_loading(placeholder)
            .with_fallback(placeholder)
            .into_any_element(),
        None => placeholder(),
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

    #[test]
    fn registry_identity_survives_version_changes() {
        let mut info: Info = serde_json::from_value(serde_json::json!({
            "summary": {"name":"registry-skill","revision":1,"digest":"captured",
                "settings_revision":1,"enabled":true,"version":null,"description":null},
            "icon":null,"extension":null,"settings":null,
            "skill":{"registry":{"namespace":"example","slug":"review","version":"1.0.0",
                "version_id":"old","content_hash":"hash","package_md5":"md5","archive_sha256":"sha"}}
        })).unwrap();
        let mut entry = skillcatalog::Entry {
            id: "registry-review".into(),
            package: None,
            name: "review".into(),
            display_name: None,
            icon: None,
            display_locales: Default::default(),
            description: None,
            description_locales: Default::default(),
            origin: "skillhub.cn".into(),
            url: None,
            source: Some(skillcatalog::InstallSource::Registry(
                sailry_protocol::plugin::skills::RegistrySource {
                    namespace: Some("example".into()),
                    slug: "review".into(),
                    version: Some("2.0.0".into()),
                },
            )),
            bundled: false,
        };
        assert!(installed(&entry, &info));
        let Some(skillcatalog::InstallSource::Registry(source)) = &mut entry.source else {
            unreachable!()
        };
        source.namespace = Some("another".into());
        assert!(!installed(&entry, &info));
        info.skill = None;
        assert!(!installed(&entry, &info));
    }
}

#[cfg(test)]
mod refresh_tests {
    use super::*;

    #[tokio::test]
    async fn retains_visible_page_boundaries() {
        let calls = std::cell::RefCell::new(Vec::new());
        let page = refresh(1, 3, |page| {
            calls.borrow_mut().push(page);
            std::future::ready(Ok(skillcatalog::Page {
                entries: vec![],
                page,
                next: Some(page + 1),
                total: Some(100),
                query_scope: skillcatalog::QueryScope::Catalog,
                unavailable: false,
            }))
        })
        .await
        .unwrap();
        assert_eq!(*calls.borrow(), [1, 2, 3]);
        assert_eq!(page.page, 3);
        assert_eq!(page.next, Some(4));
    }
}
