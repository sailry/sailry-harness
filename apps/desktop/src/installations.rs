//! Presentation of installation requests. Execution and retries stay with their owners.
use crate::tr;
use gpui_kit::component::list::{List, ListState};
use gpui_kit::*;
use sailry_client::{Client, installation::View};
use sailry_link::CancellationToken;
use sailry_protocol::installation::{Resource, State as Status};
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Request, RequestId};
use std::{collections::BTreeMap, rc::Rc, sync::Arc};

mod popover;
pub(crate) use popover::button;
mod models;
mod speech;
pub(crate) use models::{completion as model_result, external, retry_model};
pub(crate) use speech::controller;
pub(crate) use speech::external as external_controller;
mod lifecycle;
mod rows;
pub(crate) use lifecycle::reconcile;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Location {
    Node(NodeId),
    Controller,
}
type Key = (Location, RequestId);
type Retry = Rc<dyn Fn(Attempt, &mut Window, &mut App)>;

pub(crate) struct Capture<'a> {
    pub request: &'a Request,
    pub resource: Option<Resource>,
    pub waiting: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum Attempt {
    New,
    Recover,
}

impl Attempt {
    pub(crate) fn request(self, client: &Client, captured: &Request) -> Request {
        match self {
            Self::New => {
                let mut request = client.prepare(captured.command.clone());
                request.plugin = captured.plugin.clone();
                request
            }
            Self::Recover => captured.clone(),
        }
    }
}

#[derive(Default)]
pub(crate) struct Badge {
    count: usize,
}
impl Global for Badge {}

struct Registry(Entity<Queue>);
impl Global for Registry {}

struct Source {
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    stop: CancellationToken,
    _watch: Task<()>,
}
impl Drop for Source {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

struct Queue {
    entries: BTreeMap<Key, Entry>,
    next_order: u64,
    sources: BTreeMap<NodeId, Source>,
    known: Option<std::collections::BTreeSet<NodeId>>,
    controller: Option<speech::Source>,
    list: Option<Entity<ListState<rows::Rows>>>,
    window: Option<WindowId>,
}

struct Entry {
    order: u64,
    resource: Resource,
    status: Presentation,
    retry: Option<Retry>,
    finished: bool,
    connected: bool,
    dismissing: bool,
}

impl Entry {
    fn actionable(&self) -> bool {
        self.finished && !self.dismissing && self.status.terminal()
    }
}

#[derive(Clone)]
enum Presentation {
    Waiting,
    Starting,
    Observed(Status),
}

impl Presentation {
    fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Observed(Status::Failed { .. } | Status::Uncertain { .. })
        )
    }
}

#[derive(Clone)]
pub(crate) struct Ticket {
    queue: Entity<Queue>,
    key: Key,
}

pub(crate) fn init(cx: &mut App) {
    if !cx.has_global::<Badge>() {
        cx.set_global(Badge::default());
    }
}

pub(crate) fn visible(cx: &App) -> bool {
    cx.has_global::<Badge>() && cx.global::<Badge>().count > 0
}

fn registry(cx: &mut App) -> Entity<Queue> {
    init(cx);
    if !cx.has_global::<Registry>() {
        let queue = cx.new(|_| Queue {
            entries: BTreeMap::new(),
            next_order: 0,
            sources: BTreeMap::new(),
            known: None,
            controller: None,
            list: None,
            window: None,
        });
        cx.set_global(Registry(queue));
    }
    cx.global::<Registry>().0.clone()
}

pub(crate) fn connected(client: Arc<Client>, runtime: Arc<tokio::runtime::Runtime>, cx: &mut App) {
    registry(cx).update(cx, |queue, cx| queue.bind(client, runtime, cx));
}

fn resource(command: &Command) -> Option<Resource> {
    let name = match command {
        Command::InstallPlugin { name, .. }
        | Command::InstallBundledPlugin { name, .. }
        | Command::InstallPluginSource { name, .. }
        | Command::InstallPluginUpload { name, .. }
        | Command::InstallSkill { name, .. }
        | Command::InstallRegistrySkill { name, .. }
        | Command::InstallSkillUpload { name, .. }
        | Command::InstallMcp { name, .. }
        | Command::SetPluginEnabled {
            name,
            enabled: true,
            ..
        } => name,
        _ => return None,
    };
    Some(Resource::Plugin { name: name.clone() })
}

/// Register a captured domain request, not another job or durable admission.
pub(crate) fn track(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    request: &Request,
    waiting: bool,
    retry: impl Fn(Attempt, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Option<Ticket> {
    track_resource(
        client,
        runtime,
        Capture {
            request,
            resource: resource(&request.command),
            waiting,
        },
        retry,
        window,
        cx,
    )
}

/// Model IDs resolve through Node metadata; their command carries no kind.
pub(crate) fn track_resource(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    capture: Capture<'_>,
    retry: impl Fn(Attempt, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Option<Ticket> {
    let Capture {
        request,
        resource,
        waiting,
    } = capture;
    let resource = resource?;
    let queue = registry(cx);
    let key = (Location::Node(client.target()), request.id);
    queue.update(cx, |queue, cx| {
        queue.bind(client, runtime, cx);
        // A new attempt has a new row. Retained failures remain visible until
        // explicitly dismissed; stable-ID recovery updates the existing row.
        let order = if let Some(entry) = queue.entries.get(&key) {
            entry.order
        } else {
            queue.next_order += 1;
            queue.next_order
        };
        queue.entries.insert(
            key,
            Entry {
                order,
                resource,
                status: if waiting {
                    Presentation::Waiting
                } else {
                    Presentation::Starting
                },
                retry: Some(Rc::new(retry)),
                finished: false,
                connected: true,
                dismissing: false,
            },
        );
        queue.changed(cx);
        if !waiting {
            queue.open(window, cx);
        }
    });
    Some(Ticket { queue, key })
}

/// Measured progress of a Node model installation in the shared observation.
pub(crate) fn model_progress(
    node: NodeId,
    id: &str,
    cx: &App,
) -> Option<sailry_protocol::installation::Progress> {
    cx.try_global::<Registry>()?
        .0
        .read(cx)
        .model_progress(node, id)
}

/// Re-renders an observer when installation observations change.
pub(crate) fn observe<T: 'static>(cx: &mut Context<T>) -> Subscription {
    let queue = registry(cx);
    cx.observe(&queue, |_, _, cx| cx.notify())
}

/// Only the original command completion proves success, never feed disappearance.
pub(crate) fn result(
    result: &Result<Result<Output, Fault>, tokio::task::JoinError>,
) -> Result<(), Fault> {
    match result {
        Ok(Ok(Output::Plugin(_))) => Ok(()),
        Ok(Err(error)) => Err(error.clone()),
        _ => Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "installation result is unconfirmed",
        )),
    }
}

pub(crate) fn target(origin: AnyWindowHandle, cx: &App) -> Option<AnyWindowHandle> {
    let windows = cx.windows();
    windows
        .contains(&origin)
        .then_some(origin)
        .or_else(|| cx.active_window())
        .or_else(|| windows.first().copied())
}

impl Ticket {
    pub(crate) fn start(&self, window: &mut Window, cx: &mut App) {
        self.queue.update(cx, |queue, cx| {
            if let Some(entry) = queue.entries.get_mut(&self.key) {
                entry.status = Presentation::Starting;
                entry.finished = false;
            }
            queue.changed(cx);
            queue.open(window, cx);
        });
    }

    pub(crate) fn record(&self, result: Result<(), Fault>, cx: &mut App) {
        self.record_with(result, true, cx);
    }

    /// Domain-confirmed completions normalize transport uncertainty separately.
    pub(crate) fn record_outcome(&self, result: Result<(), Fault>, cx: &mut App) {
        self.record_with(result, false, cx);
    }

    fn record_with(&self, result: Result<(), Fault>, unavailable_uncertain: bool, cx: &mut App) {
        self.queue.update(cx, |queue, cx| {
            if let Some(entry) = queue.entries.get_mut(&self.key) {
                match result {
                    Ok(()) => {
                        queue.entries.remove(&self.key);
                    }
                    Err(error) => {
                        let uncertain = error.code == ErrorCode::OutcomeUnknown
                            || (unavailable_uncertain && error.code == ErrorCode::Unavailable);
                        entry.status = Presentation::Observed(if uncertain {
                            Status::Uncertain { error }
                        } else {
                            Status::Failed { error }
                        });
                        entry.finished = true;
                    }
                }
            }
            queue.changed(cx);
        });
    }

    pub(crate) fn close_empty(&self, window: &mut Window, cx: &mut App) {
        self.queue
            .update(cx, |queue, cx| queue.close_empty(window, cx));
    }
}

impl Queue {
    fn model_progress(
        &self,
        node: NodeId,
        id: &str,
    ) -> Option<sailry_protocol::installation::Progress> {
        self.entries
            .iter()
            .filter(|(key, entry)| {
                key.0 == Location::Node(node)
                    && matches!(&entry.resource, Resource::Model { id: model, .. } if model == id)
            })
            .find_map(|(_, entry)| match &entry.status {
                Presentation::Observed(Status::Running { progress, .. }) => *progress,
                _ => None,
            })
    }

    fn bind(
        &mut self,
        client: Arc<Client>,
        runtime: Arc<tokio::runtime::Runtime>,
        cx: &mut Context<Self>,
    ) {
        let node = client.target();
        if self.sources.contains_key(&node) {
            return;
        }
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let observer = client.clone();
        let (sender, mut receiver) = tokio::sync::watch::channel(View::default());
        let job =
            runtime.spawn(async move { observer.watch_installations(sender, cancelled).await });
        let watch = cx.spawn(async move |queue, cx| {
            while receiver.changed().await.is_ok() {
                let view = receiver.borrow_and_update().clone();
                if queue
                    .update(cx, |queue, cx| {
                        queue.accept(Location::Node(node), view);
                        queue.changed(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
            let _ = job.await;
        });
        self.sources.insert(
            node,
            Source {
                client,
                runtime,
                stop,
                _watch: watch,
            },
        );
    }

    fn accept(&mut self, node: Location, view: View) {
        let observed: std::collections::BTreeSet<_> =
            view.tasks.iter().map(|task| task.request).collect();
        for (key, entry) in &mut self.entries {
            if key.0 == node {
                entry.connected = view.connected;
                if view.connected
                    && entry.retry.is_none()
                    && !entry.finished
                    && !observed.contains(&key.1)
                {
                    // Without the original captured request, absence cannot prove
                    // its outcome or authorize a reconstructed retry.
                    entry.status = Presentation::Observed(Status::Uncertain {
                        error: Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "installation result is unconfirmed",
                        ),
                    });
                    entry.finished = true;
                }
            }
        }
        for task in view.tasks {
            if !self.entries.contains_key(&(node, task.request)) {
                self.next_order += 1;
                self.entries.insert(
                    (node, task.request),
                    Entry {
                        order: self.next_order,
                        resource: task.resource.clone(),
                        status: Presentation::Observed(task.state.clone()),
                        finished: task.state.terminal(),
                        connected: view.connected,
                        dismissing: false,
                        retry: None,
                    },
                );
            }
            if let Some(entry) = self.entries.get_mut(&(node, task.request))
                && !entry.finished
                && entry.resource == task.resource
            {
                if entry.retry.is_none() {
                    entry.finished = task.state.terminal();
                }
                entry.status = Presentation::Observed(task.state);
            }
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.prune();
        cx.global_mut::<Badge>().count = self.entries.len();
        if self.entries.is_empty() {
            self.window = None;
        }
        if let Some(list) = &self.list {
            let rows = rows::snapshot(&self.entries);
            list.update(cx, |list, cx| {
                list.delegate_mut().entries = rows;
                cx.notify();
            });
        }
        cx.notify();
    }

    fn retry(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.entries.get(&key).filter(|entry| entry.actionable()) {
            let Some(retry) = entry.retry.clone() else {
                return;
            };
            let attempt = if matches!(
                entry.status,
                Presentation::Observed(Status::Uncertain { .. })
            ) {
                Attempt::Recover
            } else {
                Attempt::New
            };
            // Domain owners may update this presenter while starting a retry.
            window.defer(cx, move |window, cx| retry(attempt, window, cx));
        }
    }

    fn remove(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        if !self.entries.get(&key).is_some_and(Entry::actionable) {
            return;
        }
        if key.0 == Location::Controller {
            self.remove_controller(key, window, cx);
            return;
        }
        let Location::Node(node) = key.0 else {
            return;
        };
        let Some(source) = self.sources.get(&node) else {
            return;
        };
        let client = source.client.clone();
        let job = source.runtime.spawn(async move {
            client
                .execute(client.prepare(Command::DismissInstallation { request: key.1 }))
                .await
        });
        self.entries
            .get_mut(&key)
            .expect("terminal installation")
            .dismissing = true;
        self.changed(cx);
        let origin = window.window_handle();
        cx.spawn(async move |queue, cx| {
            let result = job.await;
            let removed = matches!(result, Ok(Ok(Output::InstallationDismissed { request })) if request == key.1);
            let _ = queue.update(cx, |queue, cx| {
                if removed { queue.entries.remove(&key); }
                else if let Some(entry) = queue.entries.get_mut(&key) { entry.dismissing = false; }
                queue.changed(cx);
            });
            if let Some(target) = cx.update(|cx| target(origin, cx)) {
                let _ = target.update(cx, |_, window, cx| {
                    let _ = queue.update(cx, |queue, cx| queue.close_empty(window, cx));
                    if !removed {
                        crate::feedback::toast(window, tr("installation_remove_failed"),
                            gpui_kit::component::notification::Notification::error(tr("installation_remove_failed")), cx);
                    }
                });
            }
        }).detach();
    }
}
