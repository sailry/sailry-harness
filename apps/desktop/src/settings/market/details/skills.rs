use super::*;
use crate::settings::{providers::Binding, skills::install};
use sailry_protocol::{
    Fault,
    plugin::{skillcatalog, skills as protocol},
};

pub(super) struct Details {
    target: Target,
    binding: Binding,
    entry: skillcatalog::Entry,
    resolved: Option<Resolved>,
    document: Option<protocol::Document>,
    loading: bool,
    error: Option<&'static str>,
    selection: CancellationToken,
    stop: CancellationToken,
    task: Option<Task<()>>,
    scroll: ScrollHandle,
}

enum Target {
    Catalog(WeakEntity<Market>),
    Installed {
        package: sailry_protocol::plugin::Reference,
        version: Option<String>,
    },
}

#[derive(Clone)]
enum Resolved {
    Git(protocol::Discovery),
    Registry(protocol::RegistryDiscovery),
}

#[derive(Clone)]
pub(in crate::settings::market) struct Snapshot {
    entry: skillcatalog::Entry,
    resolved: Option<Resolved>,
    document: Option<protocol::Document>,
}

impl Drop for Details {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(in crate::settings) fn open(
    market: Entity<Market>,
    entry: skillcatalog::Entry,
    selection_stop: CancellationToken,
    window: &mut Window,
    cx: &mut App,
) {
    if selection_stop.is_cancelled() {
        return;
    }
    let binding = market.read(cx).binding.clone();
    let cached = market
        .read(cx)
        .skill_details
        .iter()
        .find(|cached| cached.entry == entry)
        .cloned();
    let details = cx.new(|cx| {
        crate::feedback::observe(window, cx, |state: &Details, _| {
            state.error.into_iter().collect()
        });
        cx.observe(&market, |_, _, cx| cx.notify()).detach();
        Details {
            target: Target::Catalog(market.downgrade()),
            binding,
            entry,
            resolved: cached.as_ref().and_then(|cached| cached.resolved.clone()),
            document: cached.and_then(|cached| cached.document),
            loading: false,
            error: None,
            selection: selection_stop,
            stop: CancellationToken::new(),
            task: None,
            scroll: ScrollHandle::new(),
        }
    });
    details.update(cx, |state, cx| state.load(cx));
    window.dispatch_action(
        Box::new(super::super::super::OpenDetails(details.into())),
        cx,
    );
}

pub(in crate::settings) fn open_installed(
    owner: Entity<Workspace>,
    binding: Binding,
    info: sailry_protocol::plugin::Info,
    skill: sailry_protocol::plugin::Skill,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(provenance) = info.skill.as_ref() else {
        return;
    };
    let (url, version) = match provenance {
        protocol::Provenance::Git(source) => (Some(source.source.repository.clone()), None),
        protocol::Provenance::Registry { registry } => (
            Some(format!(
                "https://skillhub.cn/skills/{}/{}",
                registry.namespace, registry.slug
            )),
            Some(registry.version.clone()),
        ),
        protocol::Provenance::Imported { .. } => (None, None),
    };
    let entry = skillcatalog::Entry {
        id: format!("{}:{}", info.summary.name, skill.name),
        package: Some(info.summary.name.clone()),
        name: skill.name,
        display_name: skill.display_name,
        icon: None,
        display_locales: skill.display_locales,
        description: Some(skill.description),
        description_locales: Default::default(),
        origin: crate::settings::skills::provenance(&info),
        url,
        source: None,
        bundled: false,
    };
    let package = info.summary.reference();
    let node = binding.client.target();
    let details = cx.new(|cx| {
        crate::feedback::observe(window, cx, |state: &Details, _| {
            state.error.into_iter().collect()
        });
        cx.observe_in(
            &owner,
            window,
            move |details: &mut Details, owner, window, cx| {
                let workspace = owner.read(cx);
                if workspace.section != crate::settings::Section::Skills
                    || workspace
                        .provider_link
                        .as_ref()
                        .is_none_or(|live| live.binding.client.target() != node)
                {
                    details.selection.cancel();
                    details.stop.cancel();
                    window.dispatch_action(
                        Box::new(crate::settings::CloseOwnedDetails(cx.entity_id())),
                        cx,
                    );
                }
            },
        )
        .detach();
        Details {
            target: Target::Installed { package, version },
            binding,
            entry,
            resolved: None,
            document: None,
            loading: false,
            error: None,
            selection: CancellationToken::new(),
            stop: CancellationToken::new(),
            task: None,
            scroll: ScrollHandle::new(),
        }
    });
    details.update(cx, |state, cx| state.load(cx));
    window.dispatch_action(Box::new(crate::settings::OpenDetails(details.into())), cx);
}

fn candidate<'a>(
    entries: &'a [protocol::Candidate],
    name: &str,
) -> Option<&'a protocol::Candidate> {
    if let [candidate] = entries {
        return Some(candidate);
    }
    let mut matching = entries.iter().filter(|candidate| {
        candidate.name == name
            || candidate.skill.name == name
            || candidate.path.rsplit('/').next() == Some(name)
    });
    let selected = matching.next()?;
    matching.next().is_none().then_some(selected)
}

async fn resolve(
    binding: &Binding,
    entry: &skillcatalog::Entry,
) -> Result<(Option<Resolved>, Option<protocol::Document>), Fault> {
    let (resolved, source) = if entry.bundled
        && let Some(package) = &entry.package
    {
        (
            None,
            Some(protocol::DocumentSource::Bundled {
                package: package.clone(),
                skill: entry.name.clone(),
            }),
        )
    } else {
        match &entry.source {
            Some(skillcatalog::InstallSource::Git(source)) => {
                let Output::SkillDiscovery(discovery) = binding
                    .client
                    .execute(binding.client.prepare(Command::DiscoverSkills {
                        source: source.clone(),
                    }))
                    .await?
                else {
                    return Err(Fault::new(
                        sailry_protocol::ErrorCode::Internal,
                        "unexpected skill discovery response",
                    ));
                };
                let source = candidate(&discovery.skills, &entry.name).map(|selected| {
                    protocol::DocumentSource::Git {
                        source: discovery.source.clone(),
                        path: selected.path.clone(),
                    }
                });
                (Some(Resolved::Git(discovery)), source)
            }
            Some(skillcatalog::InstallSource::Registry(source)) => {
                let Output::RegistrySkillDiscovery(discovery) = binding
                    .client
                    .execute(binding.client.prepare(Command::DiscoverRegistrySkill {
                        source: source.clone(),
                    }))
                    .await?
                else {
                    return Err(Fault::new(
                        sailry_protocol::ErrorCode::Internal,
                        "unexpected skill discovery response",
                    ));
                };
                let source = Some(protocol::DocumentSource::Registry(discovery.source.clone()));
                (Some(Resolved::Registry(discovery)), source)
            }
            None => (None, None),
        }
    };
    let document = if let Some(source) = source {
        match binding
            .client
            .execute(
                binding
                    .client
                    .prepare(Command::ReadSkillDocument { source }),
            )
            .await?
        {
            Output::SkillDocument(document) => Some(document),
            _ => {
                return Err(Fault::new(
                    sailry_protocol::ErrorCode::Internal,
                    "unexpected skill document response",
                ));
            }
        }
    } else {
        None
    };
    Ok((resolved, document))
}

async fn read(
    binding: &Binding,
    entry: &skillcatalog::Entry,
    stop: &CancellationToken,
) -> Option<Result<(Option<Resolved>, Option<protocol::Document>), Fault>> {
    tokio::select! {
        biased;
        _ = stop.cancelled() => None,
        result = resolve(binding, entry) => Some(result),
    }
}

impl Details {
    fn market(&self) -> Option<Entity<Market>> {
        match &self.target {
            Target::Catalog(market) => market.upgrade(),
            Target::Installed { .. } => None,
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if self.selection.is_cancelled() {
            return;
        }
        self.stop.cancel();
        self.stop = self.selection.child_token();
        self.loading = true;
        self.error = None;
        let stop = self.stop.clone();
        let cancelled = stop.clone();
        let binding = self.binding.clone();
        let entry = self.entry.clone();
        let runtime = binding.runtime.clone();
        let package = match &self.target {
            Target::Installed { package, .. } => Some(package.clone()),
            Target::Catalog(_) => None,
        };
        let job = runtime.spawn(async move {
            if let Some(package) = package {
                tokio::select! {
                    biased;
                    _ = cancelled.cancelled() => None,
                    result = binding.client.execute(binding.client.prepare(Command::ReadSkillDocument {
                        source: protocol::DocumentSource::Installed { package, skill: entry.name },
                    })) => Some(result.and_then(|output| match output {
                        Output::SkillDocument(document) => Ok((None, Some(document))),
                        _ => Err(Fault::new(sailry_protocol::ErrorCode::Internal, "unexpected skill document response")),
                    })),
                }
            } else {
                read(&binding, &entry, &cancelled).await
            }
        });
        self.task = Some(cx.spawn(async move |state, cx| {
            let result = job.await;
            _ = state.update(cx, |state, cx| {
                if stop.is_cancelled() {
                    return;
                }
                state.loading = false;
                match result {
                    Ok(Some(Ok((resolved, document)))) => {
                        if let Some(market) = state.market() {
                            market.update(cx, |market, _| {
                                market
                                    .skill_details
                                    .retain(|cached| cached.entry != state.entry);
                                if market.skill_details.len() == 12 {
                                    market.skill_details.remove(0);
                                }
                                market.skill_details.push(Snapshot {
                                    entry: state.entry.clone(),
                                    resolved: resolved.clone(),
                                    document: document.clone(),
                                });
                            });
                        }
                        state.resolved = resolved;
                        state.document = document;
                    }
                    Ok(None) => {}
                    Ok(Some(Err(error))) if error.code == sailry_protocol::ErrorCode::Cancelled => {
                    }
                    _ => state.error = Some("skills_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn install(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(market) = self.market() else {
            return;
        };
        let Some(owner) = market.read(cx).owner.upgrade() else {
            return;
        };
        if market.read(cx).installed_skill(&self.entry, cx).is_some() {
            return;
        }
        let known = owner.read(cx).plugin_catalog.packages.clone();
        match &self.resolved {
            Some(Resolved::Git(discovery)) => {
                install::open_discovery(self.binding.clone(), known, discovery.clone(), window, cx)
            }
            Some(Resolved::Registry(discovery)) => install::open_registry_discovery(
                self.binding.clone(),
                known,
                discovery.clone(),
                window,
                cx,
            ),
            None => {
                if let Some(source) = &self.entry.source {
                    install::open_catalog(self.binding.clone(), known, source.clone(), window, cx)
                }
            }
        }
    }

    fn update(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(market) = self.market() else {
            return;
        };
        let Some(current) = market.read(cx).installed_skill(&self.entry, cx) else {
            return;
        };
        market.update(cx, |market, cx| {
            market.update(&current.summary.name, window, cx)
        });
    }
}

impl Render for Details {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self
            .market()
            .and_then(|market| market.read(cx).installed_skill(&self.entry, cx));
        let installed = matches!(self.target, Target::Installed { .. }) || current.is_some();
        let update = self.market().is_some_and(|market| {
            current.as_ref().is_some_and(|current| {
                market
                    .read(cx)
                    .updates
                    .read(cx)
                    .candidate(&current.summary)
                    .is_some()
            })
        });
        let updating = self.market().is_some_and(|market| {
            market.read(cx).updates.read(cx).busy() || market.read(cx).updates.read(cx).selecting()
        });
        let connected = self
            .market()
            .is_some_and(|market| market.read(cx).connected(cx));
        let action = (matches!(self.target, Target::Catalog(_))
            && (self.entry.source.is_some() || installed))
            .then(|| {
                Button::new("skill-details-install")
                    .small()
                    .primary()
                    .label(tr(if update {
                        "market_update"
                    } else {
                        "market_install"
                    }))
                    .disabled(
                        !connected
                            || installed && (!update || updating)
                            || !installed && (self.loading || self.error.is_some()),
                    )
                    .on_click(cx.listener(move |state, _, window, cx| {
                        if update {
                            state.update(window, cx);
                        } else if !installed {
                            state.install(window, cx);
                        }
                    }))
            });
        let summary = super::summary(
            self.document
                .as_ref()
                .map(|document| document.skill.display_name(&rust_i18n::locale()).to_owned())
                .unwrap_or_else(|| self.entry.title(&rust_i18n::locale()).to_owned())
                .into(),
            if installed {
                format!("{} · {}", self.entry.origin, tr("market_installed"))
            } else {
                self.entry.origin.clone()
            },
            self.document
                .as_ref()
                .map(|document| document.skill.description.clone())
                .or_else(|| {
                    self.entry
                        .description(&rust_i18n::locale())
                        .map(str::to_owned)
                }),
            Some(
                Icon::new(IconName::BookOpen)
                    .size(px(40.))
                    .into_any_element(),
            ),
            action,
            cx,
        );
        let mut content = v_flex().w_full().min_w_0().gap_5();
        let mut information = vec![
            rows::row(
                "skill-source".into(),
                IconName::Link,
                tr("market_details_source"),
                None,
                self.entry.origin.clone(),
                cx,
            )
            .into_any_element(),
        ];
        let version = match (&self.target, &self.resolved) {
            (Target::Installed { version, .. }, _) => version.as_deref(),
            (_, Some(Resolved::Registry(discovery))) => Some(discovery.source.version.as_str()),
            _ => None,
        };
        if let Some(version) = version {
            information.push(
                rows::row(
                    "skill-version".into(),
                    IconName::Tag,
                    tr("market_details_version"),
                    None,
                    version.to_owned(),
                    cx,
                )
                .into_any_element(),
            );
        }
        if let Some(url) = &self.entry.url {
            information.push(rows::homepage("skill-homepage", url, cx));
        }
        let information = rows::information(information, cx);

        if let Some(document) = &self.document {
            content = content.child(crate::content::markdown::View::markdown(
                "skill-document",
                document.content.clone(),
            ));
        } else if let Some(Resolved::Git(discovery)) = &self.resolved {
            let choices = discovery
                .skills
                .iter()
                .map(|candidate| {
                    rows::row(
                        candidate.path.clone(),
                        IconName::BookOpen,
                        candidate
                            .skill
                            .display_name(&rust_i18n::locale())
                            .to_owned(),
                        Some(candidate.path.clone()),
                        div(),
                        cx,
                    )
                    .into_any_element()
                })
                .collect();
            content = content.child(rows::section(
                "market_details_contents",
                Some(discovery.skills.len()),
                choices,
                cx,
            ));
        }
        super::panel(
            summary,
            information,
            content.into_any_element(),
            &self.scroll,
            cx,
        )
    }
}

#[cfg(test)]
mod tests;
