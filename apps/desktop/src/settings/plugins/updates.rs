//! Update inspection is cancellable; each explicit installation keeps its stable request.
use super::dialog::upload::{self, Package, Prepared};
use super::*;
use crate::settings::providers::Binding;
use gpui_kit::component::notification::{Notification, NotificationType};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Fault, Output, Request,
    plugin::{Info, Origin, Summary, catalog::Entry as Listing, updates as checks},
};
use std::collections::{BTreeMap, VecDeque};

#[derive(Default)]
pub(super) struct State {
    pub entries: BTreeMap<String, Entry>,
    check: Option<CancellationToken>,
    batch: Option<Batch>,
    task: Option<Task<()>>,
}

pub(super) struct Entry {
    pub current: Summary,
    pub candidate: Option<Candidate>,
    pub manual: bool,
}

pub(super) enum Candidate {
    Node(Info),
    Official(Listing),
    Upload(Package),
}

struct Batch {
    binding: Binding,
    entries: VecDeque<Entry>,
    request: Option<Request>,
    pending: bool,
    completed: usize,
    failed: usize,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(stop) = &self.check {
            stop.cancel();
        }
    }
}

impl State {
    pub(super) fn busy(&self) -> bool {
        self.check.is_some() || self.batch.is_some()
    }
    pub(super) fn checking(&self) -> bool {
        self.check.is_some()
    }
    pub(super) fn applying(&self) -> bool {
        self.batch.as_ref().is_some_and(|batch| batch.pending)
    }
    pub(super) fn retryable(&self) -> bool {
        self.batch.as_ref().is_some_and(|batch| !batch.pending)
    }
    pub(super) fn available(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.candidate.is_some())
            .count()
    }
    pub(super) fn accept(&mut self, packages: &[Summary]) {
        self.entries
            .retain(|_, entry| packages.contains(&entry.current));
    }
}

impl Candidate {
    pub(super) fn version(&self) -> Option<&str> {
        match self {
            Self::Node(info) => info.summary.version.as_deref(),
            Self::Official(entry) => entry.version.as_deref(),
            Self::Upload(package) => package.info.summary.version.as_deref(),
        }
    }

    fn command(&self, current: &Summary) -> Option<Command> {
        let expected_revision = current.revision;
        let name = current.name.clone();
        Some(match self {
            Self::Official(_) => return None,
            Self::Upload(package) => Command::InstallPluginUpload {
                stream: package.upload.stream,
                source: sailry_protocol::plugin::UploadSource::Directory,
                name,
                expected_revision,
            },
            Self::Node(info) => match info.origin.as_ref()? {
                Origin::Bundled => Command::InstallBundledPlugin {
                    name,
                    expected_revision,
                },
                Origin::Worktree { worktree, path } => Command::InstallPlugin {
                    worktree: *worktree,
                    path: path.clone(),
                    name,
                    expected_revision,
                },
                Origin::Online { source, path } if info.skill.is_some() => Command::InstallSkill {
                    source: source.clone(),
                    path: path.clone(),
                    name,
                    expected_revision,
                },
                Origin::Online { source, path } => Command::InstallPluginSource {
                    source: source.clone(),
                    path: path.clone(),
                    name,
                    expected_revision,
                },
                _ => return None,
            },
        })
    }
}

impl Workspace {
    pub(super) fn check_plugin_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(live) = &self.provider_link else {
            return;
        };
        if !live.connected || self.plugin_catalog.busy() {
            return;
        }
        let binding = live.binding.clone();
        let node = binding.client.target();
        let packages = self.plugin_catalog.packages.clone();
        let directories: BTreeMap<_, _> = packages
            .iter()
            .filter_map(|package| {
                crate::preferences::plugins::directory(node, &package.name, &package.digest, cx)
                    .map(|path| (package.name.clone(), path))
            })
            .collect();
        let stop = CancellationToken::new();
        self.plugin_catalog.updates.check = Some(stop.clone());
        let cancelled = stop.clone();
        let job = binding.runtime.clone().spawn(async move {
            let result = tokio::select! {
                biased;
                _ = cancelled.cancelled() => return None,
                result = binding.client.execute(binding.client.prepare(Command::CheckPluginUpdates {
                    packages: packages.iter().map(|package| checks::Request {
                        name: package.name.clone(),
                        expected_revision: package.revision,
                    }).collect(),
                })) => result,
            };
            let batch = match result {
                Ok(Output::PluginUpdates(batch)) => batch,
                Ok(_) => return Some(Err(Fault::new(ErrorCode::Internal, "plugin update reports expected"))),
                Err(error) => return Some(Err(error)),
            };
            let mut entries = BTreeMap::new();
            let mut failed = batch.failures.len();
            for report in batch.reports {
                let name = report.current.name.clone();
                let result = tokio::select! {
                    biased;
                    _ = cancelled.cancelled() => return None,
                    result = inspect(&binding, report, directories.get(&name).cloned(), cancelled.clone()) => result,
                };
                match result {
                    Ok(entry) => { entries.insert(name, entry); }
                    Err(_) => { failed += 1; }
                }
            }
            Some(Ok((entries, failed)))
        });
        self.plugin_catalog.updates.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |owner, window, cx| {
                if stop.is_cancelled()
                    || owner
                        .provider_link
                        .as_ref()
                        .is_none_or(|live| live.binding.client.target() != node)
                {
                    return;
                }
                let state = &mut owner.plugin_catalog.updates;
                state.check = None;
                match result {
                    Ok(Some(Ok((entries, failed)))) => {
                        state.entries = entries;
                        state.accept(&owner.plugin_catalog.packages);
                        let available = state.available();
                        let manual = state.entries.values().filter(|entry| entry.manual).count();
                        let message = if failed > 0 {
                            rust_i18n::t!(
                                "plugins_check_partial",
                                count = available,
                                failed = failed
                            )
                            .into()
                        } else if manual > 0 {
                            rust_i18n::t!(
                                "plugins_check_manual",
                                count = available,
                                manual = manual
                            )
                            .into()
                        } else if available > 0 {
                            rust_i18n::t!("plugins_updates_found", count = available).into()
                        } else {
                            tr("plugins_up_to_date")
                        };
                        notify(window, message, failed > 0, cx);
                    }
                    _ => notify(window, tr("plugins_check_failed"), true, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn update_plugins(
        &mut self,
        name: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.provider_link else {
            return;
        };
        if !live.connected || self.plugin_catalog.busy() {
            return;
        }
        let state = &mut self.plugin_catalog.updates;
        state.accept(&self.plugin_catalog.packages);
        let names: Vec<_> = state
            .entries
            .iter()
            .filter(|(key, entry)| {
                name.is_none_or(|name| key.as_str() == name) && entry.candidate.is_some()
            })
            .map(|(name, _)| name.clone())
            .collect();
        let entries: VecDeque<_> = names
            .into_iter()
            .filter_map(|name| state.entries.remove(&name))
            .collect();
        if entries.is_empty() {
            return;
        }
        state.batch = Some(Batch {
            binding: live.binding.clone(),
            entries,
            request: None,
            pending: false,
            completed: 0,
            failed: 0,
        });
        self.send_plugin_updates(window, cx);
    }

    pub(super) fn retry_plugin_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.plugin_catalog.updates.retryable() {
            self.send_plugin_updates(window, cx);
        } else {
            self.send_plugin_action(cx);
        }
    }

    fn send_plugin_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(batch) = &mut self.plugin_catalog.updates.batch else {
            return;
        };
        if batch.pending {
            return;
        }
        let Some(entry) = batch.entries.front() else {
            return;
        };
        if batch.request.is_none() {
            if let Some(Candidate::Official(listing)) = &entry.candidate {
                let listing = listing.clone();
                self.prepare_plugin_update(listing, window, cx);
                return;
            }
            let Some(command) = entry
                .candidate
                .as_ref()
                .and_then(|candidate| candidate.command(&entry.current))
            else {
                return;
            };
            batch.request = Some(batch.binding.client.prepare(command));
        }
        batch.pending = true;
        let binding = batch.binding.clone();
        let node = binding.client.target();
        let request = batch.request.clone().expect("update request is prepared");
        let id = request.id;
        let job = binding
            .runtime
            .clone()
            .spawn(async move { binding.client.execute(request).await });
        self.plugin_catalog.updates.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |owner, window, cx| {
                let state = &mut owner.plugin_catalog.updates;
                let Some(batch) = &mut state.batch else {
                    return;
                };
                if batch.binding.client.target() != node
                    || batch
                        .request
                        .as_ref()
                        .is_none_or(|request| request.id != id)
                {
                    return;
                }
                batch.pending = false;
                let error = match result {
                    Ok(Ok(Output::Plugin(info))) => {
                        let entry = batch.entries.pop_front().expect("active update");
                        if let Some(Candidate::Upload(package)) = entry.candidate
                            && let Some(path) = package.directory.clone()
                        {
                            crate::preferences::plugins::remember(
                                node,
                                &info.summary.name,
                                &info.summary.digest,
                                info.summary.revision,
                                path,
                                cx,
                            );
                        } else {
                            crate::preferences::plugins::forget(
                                node,
                                &info.summary.name,
                                info.summary.revision,
                                cx,
                            );
                        }
                        batch.completed += 1;
                        batch.request = None;
                        None
                    }
                    Ok(Err(error)) => Some(error),
                    _ => Some(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "plugin update result is unconfirmed",
                    )),
                };
                if let Some(error) = error {
                    let name = &batch.entries.front().expect("active update").current.name;
                    let message = rust_i18n::t!(
                        "plugins_update_error",
                        name = name,
                        error = tr(live::error_key(&error))
                    );
                    notify(window, message.into(), true, cx);
                    if live::uncertain(&error) {
                        cx.notify();
                        return;
                    }
                    let entry = batch.entries.pop_front().expect("active update");
                    if owner.plugin_catalog.packages.contains(&entry.current) {
                        state.entries.insert(entry.current.name.clone(), entry);
                    }
                    batch.failed += 1;
                    batch.request = None;
                }
                owner.advance_plugin_updates(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn prepare_plugin_update(
        &mut self,
        listing: Listing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let batch = self
            .plugin_catalog
            .updates
            .batch
            .as_mut()
            .expect("active updates");
        let current = batch
            .entries
            .front()
            .expect("active update")
            .current
            .clone();
        batch.pending = true;
        let binding = batch.binding.clone();
        let node = binding.client.target();
        let selected = current.clone();
        let job = binding
            .runtime
            .clone()
            .spawn(async move { prepare(&binding, &selected, listing).await });
        self.plugin_catalog.updates.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |owner, window, cx| {
                let state = &mut owner.plugin_catalog.updates;
                let Some(batch) = &mut state.batch else {
                    return;
                };
                if batch.binding.client.target() != node
                    || batch
                        .entries
                        .front()
                        .is_none_or(|entry| entry.current != current)
                {
                    return;
                }
                batch.pending = false;
                match result {
                    Ok(Ok(command)) => batch.request = Some(batch.binding.client.prepare(command)),
                    result => {
                        let error = match result {
                            Ok(Err(error)) => error,
                            _ => Fault::new(
                                ErrorCode::Unavailable,
                                "plugin update inspection failed",
                            ),
                        };
                        let entry = batch.entries.pop_front().expect("active update");
                        let message = rust_i18n::t!(
                            "plugins_update_error",
                            name = entry.current.name,
                            error = tr(live::error_key(&error))
                        );
                        if owner.plugin_catalog.packages.contains(&entry.current) {
                            state.entries.insert(entry.current.name.clone(), entry);
                        }
                        batch.failed += 1;
                        notify(window, message.into(), true, cx);
                    }
                }
                owner.advance_plugin_updates(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn advance_plugin_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = &mut self.plugin_catalog.updates;
        let Some(batch) = &state.batch else { return };
        if batch.entries.is_empty() {
            let message = if batch.failed > 0 {
                rust_i18n::t!(
                    "plugins_update_partial",
                    count = batch.completed,
                    failed = batch.failed
                )
            } else {
                rust_i18n::t!("plugins_updates_applied", count = batch.completed)
            };
            let failed = batch.failed > 0;
            state.batch = None;
            notify(window, message.into(), failed, cx);
        } else {
            self.send_plugin_updates(window, cx);
        }
    }
}

async fn prepare(binding: &Binding, current: &Summary, listing: Listing) -> Result<Command, Fault> {
    let source = sailry_protocol::plugin::skills::Source {
        repository: listing.repository.ok_or_else(|| {
            Fault::new(
                ErrorCode::InvalidRequest,
                "official plugin repository is unavailable",
            )
        })?,
        git_ref: Some("main".into()),
        path: Some(listing.id),
    };
    let Output::PluginSource(selected) = binding
        .client
        .execute(
            binding
                .client
                .prepare(Command::InspectPluginSource { source }),
        )
        .await?
    else {
        return Err(Fault::new(
            ErrorCode::Internal,
            "plugin source inspection expected",
        ));
    };
    if selected.info.summary.name != current.name
        || selected.info.summary.version != listing.version
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "official plugin version changed; check updates again",
        ));
    }
    Ok(Command::InstallPluginSource {
        source: selected.source,
        path: selected.path,
        name: current.name.clone(),
        expected_revision: current.revision,
    })
}

async fn inspect(
    binding: &Binding,
    report: checks::Report,
    directory: Option<std::path::PathBuf>,
    stop: CancellationToken,
) -> Result<Entry, Fault> {
    let current = &report.current;
    if report.manual
        && let Some(path) = directory
    {
        return match upload::prepare(binding.clone(), path, true, stop).await? {
            Prepared::MissingDirectory => Ok(Entry {
                current: report.current,
                candidate: None,
                manual: true,
            }),
            Prepared::Package(package) => {
                let package = *package;
                if package.info.summary.name != current.name {
                    return Err(Fault::new(
                        ErrorCode::InvalidRequest,
                        "plugin update has a different identity",
                    ));
                }
                Ok(Entry {
                    candidate: (package.info.summary.digest != current.digest)
                        .then_some(Candidate::Upload(package)),
                    current: report.current,
                    manual: false,
                })
            }
        };
    }
    Ok(Entry {
        current: report.current,
        candidate: report.available.map(|candidate| match candidate {
            checks::Candidate::Package(info) => Candidate::Node(info),
            checks::Candidate::Official(entry) => Candidate::Official(entry),
        }),
        manual: report.manual,
    })
}

fn notify(window: &mut Window, message: SharedString, error: bool, cx: &mut App) {
    crate::feedback::status(
        window,
        message.clone(),
        if error {
            NotificationType::Error
        } else {
            NotificationType::Success
        },
        if error {
            Notification::error(message)
        } else {
            Notification::success(message)
        }
        .id::<State>(),
        cx,
    );
}
