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
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Duration,
};

mod changes;
#[cfg(test)]
mod tests;
mod view;

fn retry_delay(startup: bool, code: ErrorCode, retries: u8) -> Option<Duration> {
    (startup && code == ErrorCode::Busy && retries < 4)
        .then(|| Duration::from_millis(250 * (1 << retries)))
}

fn completion(result: &Result<Output, Fault>, uncertain: bool) -> Result<(), Fault> {
    match result {
        Ok(Output::Plugin(_)) => Ok(()),
        Err(error) => {
            let mut error = error.clone();
            if uncertain {
                error.code = ErrorCode::OutcomeUnknown;
            }
            Err(error)
        }
        _ => Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "plugin update result is unconfirmed",
        )),
    }
}

fn error_key(error: &Fault, uncertain: bool) -> &'static str {
    if uncertain {
        "plugins_unknown"
    } else if error.code == ErrorCode::Unavailable {
        "plugins_failed"
    } else {
        live::error_key(error)
    }
}

async fn query(
    client: &sailry_client::Client,
    request: Request,
    startup: bool,
    cancelled: &CancellationToken,
) -> Option<Result<Output, Fault>> {
    let mut retries = 0;
    loop {
        let result = tokio::select! {
            biased;
            _ = cancelled.cancelled() => return None,
            result = client.execute(request.clone()) => result,
        };
        let delay = result
            .as_ref()
            .err()
            .and_then(|error| retry_delay(startup, error.code, retries));
        let Some(delay) = delay else {
            return Some(result);
        };
        tokio::select! {
            biased;
            _ = cancelled.cancelled() => return None,
            _ = tokio::time::sleep(delay) => {},
        }
        retries += 1;
    }
}

pub(in crate::settings) struct State {
    binding: Binding,
    packages: Vec<Summary>,
    pub(super) entries: BTreeMap<String, Entry>,
    check: Option<CancellationToken>,
    batch: Option<Batch>,
    task: Option<Task<()>>,
    dialog: Option<view::Selection>,
    popup: crate::ui::overlays::Owned,
}

pub(super) struct Entry {
    pub current: Summary,
    pub candidate: Option<Candidate>,
    pub blocked: Option<checks::Blocked>,
    pub manual: bool,
    pub previous: Option<Info>,
}

pub(super) enum Candidate {
    Node(Info),
    Upload(Package),
}

struct Batch {
    binding: Binding,
    entries: VecDeque<Entry>,
    request: Option<Request>,
    plans: BTreeMap<String, (Request, Option<crate::installations::Ticket>)>,
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
    pub(in crate::settings) fn candidate(&self, current: &Summary) -> Option<&Info> {
        let entry = self
            .entries
            .get(&current.name)
            .filter(|entry| entry.current == *current)?;
        match entry.candidate.as_ref()? {
            Candidate::Node(info) => Some(info),
            Candidate::Upload(package) => Some(&package.info),
        }
    }

    pub(in crate::settings) fn blocked(&self, current: &Summary) -> Option<&checks::Blocked> {
        self.entries
            .get(&current.name)
            .filter(|entry| entry.current == *current)?
            .blocked
            .as_ref()
    }

    pub(in crate::settings) fn blocked_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.blocked.is_some())
            .count()
    }

    pub(super) fn added_actions(&self, current: &Info) -> Vec<String> {
        self.candidate(&current.summary)
            .map(|candidate| changes::Changes::between(current, candidate).actions.added)
            .unwrap_or_default()
    }

    pub(in crate::settings) fn has_candidate(&self, name: &str) -> bool {
        self.entries
            .get(name)
            .is_some_and(|entry| entry.candidate.is_some())
    }
    pub(in crate::settings) fn busy(&self) -> bool {
        self.check.is_some() || self.batch.is_some()
    }
    pub(in crate::settings) fn pending_package(&self, name: &str) -> bool {
        self.batch.as_ref().is_some_and(|batch| {
            batch.plans.contains_key(name)
                || batch.entries.iter().any(|entry| entry.current.name == name)
        })
    }
    pub(in crate::settings) fn selecting(&self) -> bool {
        self.dialog.is_some()
    }
    pub(in crate::settings) fn checking(&self) -> bool {
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
        self.packages = packages.to_vec();
        self.entries
            .retain(|_, entry| packages.contains(&entry.current));
        if let Some(dialog) = &mut self.dialog {
            dialog.rows.retain(|name, row| {
                row.status != view::Status::Ready || self.entries.contains_key(name)
            });
        }
    }
}

impl Candidate {
    pub(super) fn version(&self) -> Option<&str> {
        match self {
            Self::Node(info) => info.summary.version.as_deref(),
            Self::Upload(package) => package.info.summary.version.as_deref(),
        }
    }

    fn command(&self, current: &Summary) -> Option<Command> {
        let expected_revision = current.revision;
        let name = current.name.clone();
        Some(match self {
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
                    expected_digest: Some(info.summary.digest.clone()),
                },
                Origin::Registry { source } if info.skill.is_some() => {
                    Command::InstallRegistrySkill {
                        source: source.clone(),
                        name,
                        expected_revision,
                    }
                }
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

#[derive(Default)]
struct Registry {
    nodes: BTreeMap<sailry_protocol::NodeId, Entity<State>>,
    checked: BTreeSet<sailry_protocol::NodeId>,
}
impl Global for Registry {}

pub(in crate::settings) fn bind(binding: Binding, cx: &mut App) -> Entity<State> {
    if !cx.has_global::<Registry>() {
        cx.set_global(Registry::default());
    }
    let node = binding.client.target();
    if let Some(state) = cx.global::<Registry>().nodes.get(&node) {
        return state.clone();
    }
    let state = cx.new(|_| State {
        binding,
        packages: Vec::new(),
        entries: BTreeMap::new(),
        check: None,
        batch: None,
        task: None,
        dialog: None,
        popup: Default::default(),
    });
    cx.global_mut::<Registry>()
        .nodes
        .insert(node, state.clone());
    state
}

pub(crate) fn connected(
    transport: std::sync::Arc<dyn sailry_link::Transport>,
    runtime: std::sync::Arc<tokio::runtime::Runtime>,
    label: SharedString,
    packages: &[Summary],
    window: &mut Window,
    cx: &mut App,
) {
    let state = bind(
        Binding {
            client: std::sync::Arc::new(sailry_client::Client::new(transport)),
            runtime,
            label,
        },
        cx,
    );
    let node = state.read(cx).binding.client.target();
    state.update(cx, |state, cx| {
        state.accept(packages);
        state.present(window, cx);
    });
    if state.read(cx).busy() || !cx.global_mut::<Registry>().checked.insert(node) {
        return;
    }
    state.update(cx, |state, cx| state.check(true, window, cx));
}

impl Workspace {
    pub(in crate::settings) fn check_plugin_updates(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.plugin_catalog.busy(cx)
            || self
                .provider_link
                .as_ref()
                .is_none_or(|live| !live.connected)
        {
            return;
        }
        if let Some(state) = &self.plugin_catalog.updates {
            state.update(cx, |state, cx| state.check(false, window, cx));
        }
    }

    pub(in crate::settings) fn update_plugins(
        &mut self,
        name: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.plugin_catalog.busy(cx)
            || self
                .provider_link
                .as_ref()
                .is_none_or(|live| !live.connected)
        {
            return;
        }
        if let Some(state) = &self.plugin_catalog.updates {
            let packages = name.is_none().then(|| self.installed_plugins(cx));
            state.update(cx, |state, cx| {
                if let Some(packages) = packages {
                    state.apply_packages(&packages, window, cx);
                } else {
                    state.apply(name, window, cx);
                }
            });
        }
    }

    pub(super) fn retry_plugin_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = self.plugin_catalog.updates.clone()
            && state.read(cx).retryable()
        {
            state.update(cx, |state, cx| state.send(window, cx));
        }
    }
}

impl State {
    fn check(&mut self, startup: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() || self.selecting() {
            return;
        }
        let binding = self.binding.clone();
        let node = binding.client.target();
        let packages = self.packages.clone();
        let directories: BTreeMap<_, _> = packages
            .iter()
            .filter_map(|package| {
                crate::preferences::plugins::directory(node, &package.name, &package.digest, cx)
                    .map(|path| (package.name.clone(), path))
            })
            .collect();
        let stop = CancellationToken::new();
        self.check = Some(stop.clone());
        let cancelled = stop.clone();
        let job = binding.runtime.clone().spawn(async move {
            let request = binding.client.prepare(Command::CheckPluginUpdates {
                force: !startup,
                packages: packages.iter().map(|package| checks::Request {
                    name: package.name.clone(),
                    expected_revision: package.revision,
                }).collect(),
            });
            let result = query(&binding.client, request, startup, &cancelled).await?;
            let batch = match result {
                Ok(Output::PluginUpdates(batch)) => batch,
                Ok(_) => return Some(Err(Fault::new(ErrorCode::Internal, "plugin update reports expected"))),
                Err(error) => return Some(Err(error)),
            };
            let mut entries = BTreeMap::new();
            let mut failures = batch.failures;
            for report in batch.reports {
                let name = report.current.name.clone();
                let result = tokio::select! {
                    biased;
                    _ = cancelled.cancelled() => return None,
                    result = inspect(&binding, report, directories.get(&name).cloned(), cancelled.clone()) => result,
                };
                match result {
                    Ok(entry) => { entries.insert(name, entry); }
                    Err(error) => { failures.push(checks::Failure { name, error }); }
                }
            }
            Some(Ok((entries, failures)))
        });
        let origin = window.window_handle();
        self.task = Some(cx.spawn(async move |owner, cx| {
            let result = job.await;
            let target = cx.update(|cx| {
                let windows = cx.windows();
                windows.contains(&origin).then_some(origin)
                    .or_else(|| cx.active_window())
                    .or_else(|| windows.first().copied())
            });
            if let Some(target) = target {
                let _ = target.update(cx, |_, window, cx| owner.update(cx, |owner, cx| {
                if stop.is_cancelled() || owner.binding.client.target() != node {
                    return;
                }
                let state = owner;
                state.check = None;
                match result {
                    Ok(Some(Ok((entries, failures)))) => {
                        let failed = failures.len();
                        state.entries = entries;
                        state.entries.retain(|_, entry| state.packages.contains(&entry.current));
                        if startup {
                            if failed > 0 {
                                eprintln!("sailry-desktop: startup plugin inspection for {node:?} had {failed} failures");
                                for failure in failures {
                                    eprintln!("sailry-desktop: plugin update inspection for {} failed: {}", failure.name, failure.error);
                                }
                            }
                            state.open(window, cx);
                            cx.notify();
                            return;
                        }
                        let available = state.available();
                        let manual = state.entries.values().filter(|entry| entry.manual).count();
                        let blocked = state.blocked_count();
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
                        } else if blocked > 0 {
                            rust_i18n::t!("plugins_updates_require_sailry", count = blocked).into()
                        } else if available > 0 {
                            rust_i18n::t!("plugins_updates_found", count = available).into()
                        } else {
                            tr("plugins_up_to_date")
                        };
                        notify(window, message, failed > 0, cx);
                    }
                    Ok(Some(Err(error))) if startup => eprintln!("sailry-desktop: startup plugin update check for {node:?} failed: {error}"),
                    Err(error) if startup => eprintln!("sailry-desktop: startup plugin update check for {node:?} interrupted: {error}"),
                    _ if startup => eprintln!("sailry-desktop: startup plugin update check for {node:?} failed"),
                    _ => notify(window, tr("plugins_check_failed"), true, cx),
                }
                cx.notify();
                }));
            } else {
                let _ = owner.update(cx, |state, cx| {
                    state.check = None;
                    cx.notify();
                });
            }
        }));
        cx.notify();
    }

    pub(in crate::settings) fn apply(
        &mut self,
        name: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let names = name.map(|name| BTreeSet::from([name.to_owned()]));
        self.apply_selection(names.as_ref(), name.is_some(), window, cx);
    }

    fn apply_packages(
        &mut self,
        packages: &[Summary],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let names = packages
            .iter()
            .map(|package| package.name.clone())
            .collect();
        self.apply_selection(Some(&names), false, window, cx);
    }

    fn apply_selection(
        &mut self,
        selected: Option<&BTreeSet<String>>,
        single: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy() || (self.dialog.is_some() && !self.popup.focused(window, cx)) {
            return;
        }
        let state = &mut *self;
        state
            .entries
            .retain(|_, entry| state.packages.contains(&entry.current));
        let names: Vec<_> = state
            .entries
            .iter()
            .filter(|(key, entry)| {
                selected.is_none_or(|names| names.contains(key.as_str()))
                    && entry.candidate.is_some()
                    && state.dialog.as_ref().is_none_or(|dialog| {
                        dialog.rows.get(*key).is_some_and(|row| {
                            row.selected && (single || row.status == view::Status::Ready)
                        })
                    })
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
        let mut plans = BTreeMap::new();
        for entry in &entries {
            if let Some(command) = entry
                .candidate
                .as_ref()
                .and_then(|candidate| candidate.command(&entry.current))
            {
                let request = state.binding.client.prepare(command);
                plans.insert(entry.current.name.clone(), (request, None));
            }
        }
        state.batch = Some(Batch {
            binding: state.binding.clone(),
            entries,
            request: None,
            plans,
            pending: false,
            completed: 0,
            failed: 0,
        });
        if state.dialog.take().is_some() {
            state.popup.close(window, cx);
        }
        let batch = state.batch.as_mut().expect("update batch is prepared");
        for (name, (request, ticket)) in &mut batch.plans {
            let id = request.id;
            let name = name.clone();
            let retry = cx.entity();
            *ticket = crate::installations::track(
                batch.binding.client.clone(),
                batch.binding.runtime.clone(),
                request,
                true,
                move |_, window, cx| {
                    retry.update(cx, |state, cx| {
                        if state.retryable() {
                            if state
                                .batch
                                .as_ref()
                                .and_then(|batch| batch.request.as_ref())
                                .is_some_and(|request| request.id == id)
                            {
                                state.send(window, cx);
                            }
                        } else {
                            state.apply(Some(&name), window, cx);
                        }
                    })
                },
                window,
                cx,
            );
        }
        self.send(window, cx);
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(batch) = &mut self.batch else {
            return;
        };
        if batch.pending {
            return;
        }
        let Some(entry) = batch.entries.front() else {
            return;
        };
        if batch.request.is_none() {
            batch.request = batch
                .plans
                .get(&entry.current.name)
                .map(|(request, _)| request.clone());
            if batch.request.is_none() {
                return;
            }
        }
        batch.pending = true;
        if let Some(dialog) = &mut self.dialog
            && let Some(row) = dialog.rows.get_mut(&entry.current.name)
        {
            row.status = view::Status::Updating;
        }
        let binding = batch.binding.clone();
        let node = binding.client.target();
        let request = batch.request.clone().expect("update request is prepared");
        let id = request.id;
        let ticket = batch
            .plans
            .get(&entry.current.name)
            .and_then(|(_, ticket)| ticket.clone());
        if let Some(ticket) = &ticket {
            ticket.start(window, cx);
        }
        let job = binding.runtime.clone().spawn(async move {
            let admission = match binding.client.dispatch(request).await {
                Ok(admission) => admission,
                Err(error) => {
                    let uncertain = live::uncertain(&error);
                    return (Err(error), uncertain);
                }
            };
            match admission.completion.await {
                Ok(result) => {
                    let uncertain = result
                        .as_ref()
                        .is_err_and(|error| error.code == ErrorCode::OutcomeUnknown);
                    (result, uncertain)
                }
                Err(_) => (
                    Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "plugin update result is unconfirmed",
                    )),
                    true,
                ),
            }
        });
        let origin = window.window_handle();
        self.task = Some(cx.spawn(async move |owner, cx| {
            let (result, mut uncertain) = job.await.unwrap_or_else(|_| {
                (
                    Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "plugin update result is unconfirmed",
                    )),
                    true,
                )
            });
            if let Some(ticket) = &ticket {
                cx.update(|cx| ticket.record_outcome(completion(&result, uncertain), cx));
            }
            let target = cx.update(|cx| {
                let windows = cx.windows();
                windows
                    .contains(&origin)
                    .then_some(origin)
                    .or_else(|| cx.active_window())
                    .or_else(|| windows.first().copied())
            });
            if let Some(target) = target {
                let _ = target.update(cx, |_, window, cx| {
                    owner.update(cx, |owner, cx| {
                        if let Some(ticket) = &ticket {
                            ticket.close_empty(window, cx);
                        }
                        let state = owner;
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
                            Ok(Output::Plugin(info)) => {
                                let entry = batch.entries.pop_front().expect("active update");
                                batch.plans.remove(&entry.current.name);
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
                                if let Some(dialog) = &mut state.dialog
                                    && let Some(row) = dialog.rows.get_mut(&entry.current.name)
                                {
                                    row.status = view::Status::Succeeded;
                                }
                                batch.completed += 1;
                                batch.request = None;
                                None
                            }
                            Err(error) => Some(error),
                            _ => {
                                uncertain = true;
                                Some(Fault::new(
                                    ErrorCode::OutcomeUnknown,
                                    "plugin update result is unconfirmed",
                                ))
                            }
                        };
                        if let Some(error) = error {
                            let name = &batch.entries.front().expect("active update").current.name;
                            let message = rust_i18n::t!(
                                "plugins_update_error",
                                name = name,
                                error = compatibility::error(
                                    error_key(&error, uncertain),
                                    compatibility::required(&error),
                                    &state.binding,
                                    cx,
                                )
                            );
                            if let Some(dialog) = &mut state.dialog
                                && let Some(row) = dialog.rows.get_mut(name)
                            {
                                row.status = view::Status::Failed;
                            }
                            notify(window, message.into(), true, cx);
                            if uncertain {
                                cx.notify();
                                return;
                            }
                            let entry = batch.entries.pop_front().expect("active update");
                            batch.plans.remove(&entry.current.name);
                            if state.packages.contains(&entry.current) {
                                state.entries.insert(entry.current.name.clone(), entry);
                            }
                            batch.failed += 1;
                            batch.request = None;
                        }
                        state.advance(window, cx);
                        cx.notify();
                    })
                });
            } else {
                let _ = owner.update(cx, |state, cx| {
                    if let Some(batch) = &mut state.batch {
                        batch.pending = false;
                    }
                    cx.notify();
                });
            }
        }));
        cx.notify();
    }

    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = &mut *self;
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
            if let Some(dialog) = &mut state.dialog {
                dialog.locked = false;
                if !failed
                    && dialog
                        .rows
                        .values()
                        .all(|row| !row.selected || row.status == view::Status::Succeeded)
                {
                    state.dialog = None;
                    state.popup.close(window, cx);
                }
            }
            notify(window, message.into(), failed, cx);
        } else {
            self.send(window, cx);
        }
    }
}

fn official_source(listing: &Listing) -> Result<sailry_protocol::plugin::skills::Source, Fault> {
    Ok(sailry_protocol::plugin::skills::Source {
        repository: listing
            .repository
            .clone()
            .ok_or_else(|| Fault::invalid("official plugin repository is unavailable"))?,
        git_ref: Some(listing.commit.clone().unwrap_or_else(|| "main".into())),
        path: Some(listing.id.clone()),
    })
}

async fn prepare(binding: &Binding, current: &Summary, listing: Listing) -> Result<Info, Fault> {
    let source = official_source(&listing)?;
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
        || listing
            .commit
            .as_ref()
            .is_some_and(|commit| *commit != selected.source.commit)
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "official plugin version changed; check updates again",
        ));
    }
    let mut info = selected.info;
    info.origin = Some(Origin::Online {
        source: selected.source,
        path: selected.path,
    });
    Ok(info)
}

async fn inspect(
    binding: &Binding,
    report: checks::Report,
    directory: Option<std::path::PathBuf>,
    stop: CancellationToken,
) -> Result<Entry, Fault> {
    let current = &report.current;
    let mut blocked = report.blocked;
    let (candidate, manual) = if report.manual
        && let Some(path) = directory
    {
        match upload::prepare(binding.clone(), path, true, stop).await? {
            Prepared::MissingDirectory => {
                return Ok(Entry {
                    current: report.current,
                    previous: None,
                    candidate: None,
                    blocked,
                    manual: true,
                });
            }
            Prepared::Package(package) => {
                let package = *package;
                if package.info.summary.name != current.name {
                    return Err(Fault::invalid("plugin update has a different identity"));
                }
                let candidate = changed(current, &package.info.summary).then_some(package);
                let candidate = candidate.and_then(|package| {
                    if let Some(required) = package.info.summary.compatibility.required() {
                        blocked = Some(checks::Blocked {
                            version: package
                                .info
                                .summary
                                .version
                                .clone()
                                .unwrap_or_else(|| "—".into()),
                            required: required.into(),
                            release_notes: package.info.release_notes.clone(),
                        });
                        None
                    } else {
                        Some(Candidate::Upload(package))
                    }
                });
                (candidate, false)
            }
        }
    } else {
        let candidate = match report.available {
            Some(checks::Candidate::Package(info)) => Some(Candidate::Node(*info)),
            Some(checks::Candidate::Official(listing)) => {
                let info = prepare(binding, current, *listing).await?;
                Some(Candidate::Node(info))
            }
            None => None,
        };
        (candidate, report.manual)
    };
    let previous = if candidate.is_some() || blocked.is_some() {
        let Output::Plugin(info) = binding
            .client
            .execute(binding.client.prepare(Command::ReadPluginVersion {
                package: current.reference(),
            }))
            .await?
        else {
            return Err(Fault::new(
                ErrorCode::Internal,
                "installed plugin information expected",
            ));
        };
        Some(info)
    } else {
        None
    };
    Ok(Entry {
        current: report.current,
        previous,
        candidate,
        blocked,
        manual,
    })
}

fn changed(current: &Summary, candidate: &Summary) -> bool {
    let semantic = |value: Option<&str>| value.and_then(|value| semver::Version::parse(value).ok());
    match (
        semantic(current.version.as_deref()),
        semantic(candidate.version.as_deref()),
    ) {
        (Some(installed), Some(available)) => available.cmp_precedence(&installed).is_gt(),
        _ => candidate.digest != current.digest,
    }
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
