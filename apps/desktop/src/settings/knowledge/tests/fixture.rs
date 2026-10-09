use super::super::*;
use crate::settings::Section;
use sailry_client::Client;
use sailry_link::{Admission, Link, NetworkScope, Pending, Subscription, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, Output, Receipt, Request, RequestId, RequestOutcome, Topic,
    Update,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

type Outcomes = Vec<(RequestId, Result<Output, Fault>)>;

pub(in crate::settings) struct Observed {
    inner: Arc<dyn Transport>,
    pub requests: Mutex<Vec<Request>>,
    pub outcomes: Arc<Mutex<Outcomes>>,
    pub lose_reply: AtomicBool,
    lose_inspect: AtomicBool,
    pub hold_install: AtomicBool,
    pub catalog: Mutex<Option<sailry_protocol::models::Definition>>,
    pub catalog_more: AtomicBool,
    pub states: Arc<Mutex<std::collections::BTreeMap<Model, knowledge::ModelState>>>,
    install: Mutex<Option<Held>>,
    cancelled: Mutex<Option<RequestId>>,
}

struct Held {
    id: RequestId,
    sender: tokio::sync::oneshot::Sender<Result<Output, Fault>>,
}

struct ObservedStream {
    inner: Box<dyn Subscription>,
    states: Arc<Mutex<std::collections::BTreeMap<Model, knowledge::ModelState>>>,
}

impl Subscription for ObservedStream {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            let mut update = self.inner.next().await?;
            if let Update::Event(envelope) = &mut update
                && let Event::KnowledgeModelsChanged { models } = &mut envelope.event
            {
                apply_states(models, &self.states.lock().unwrap());
            }
            Ok(update)
        })
    }
}

fn apply_states(
    models: &mut [ModelInfo],
    states: &std::collections::BTreeMap<Model, knowledge::ModelState>,
) {
    for info in models {
        if let Some(state) = states.get(&info.model) {
            info.state = state.clone();
        }
    }
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            Ok(Box::new(ObservedStream {
                inner: self.inner.subscribe(topic).await?,
                states: self.states.clone(),
            }) as Box<dyn Subscription>)
        })
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if let Some(definition) = self.catalog.lock().unwrap().clone() {
                use sailry_protocol::models::{Item, Page, Version};
                let output = match &request.command {
                    Command::SearchModels(search) => {
                        let mut source = definition.source.clone();
                        if search.cursor.as_deref() == Some("second") {
                            source.revision = "c".repeat(40);
                        }
                        Some(Output::ModelSearch(Page {
                            items: vec![Item {
                                source,
                                format: definition.format,
                                name: definition.name.clone(),
                            }],
                            next_cursor: (self.catalog_more.load(Ordering::SeqCst)
                                && search.cursor.is_none())
                            .then(|| "second".into()),
                            partial: false,
                        }))
                    }
                    Command::ReadModelVersions { .. } => {
                        Some(Output::ModelVersions(vec![Version {
                            name: "Pinned fixture".into(),
                            revision: definition.source.revision.clone(),
                        }]))
                    }
                    Command::ResolveModel { .. } => Some(Output::ModelDefinition(definition)),
                    _ => None,
                };
                if let Some(output) = output {
                    self.requests.lock().unwrap().push(request.clone());
                    return Ok(completed(&request, Ok(output)));
                }
            }
            // This held transport proves UI cancellation, not model transfer behavior.
            if self.hold_install.load(Ordering::SeqCst) {
                match &request.command {
                    Command::InstallKnowledgeModel { .. } => {
                        self.requests.lock().unwrap().push(request.clone());
                        let (sender, completion) = tokio::sync::oneshot::channel();
                        *self.install.lock().unwrap() = Some(Held {
                            id: request.id,
                            sender,
                        });
                        return Ok(Admission {
                            receipt: Receipt {
                                id: request.id,
                                durable: true,
                            },
                            completion,
                        });
                    }
                    Command::CancelKnowledgeModel { model } => {
                        self.requests.lock().unwrap().push(request.clone());
                        if let Some(held) = self.install.lock().unwrap().take() {
                            *self.cancelled.lock().unwrap() = Some(held.id);
                            _ = held.sender.send(Err(Fault::new(
                                ErrorCode::Cancelled,
                                "fixture install cancelled",
                            )));
                        }
                        return Ok(completed(
                            &request,
                            Ok(Output::KnowledgeModel { model: *model }),
                        ));
                    }
                    Command::InspectRequest { id, .. }
                        if self.cancelled.lock().unwrap().as_ref() == Some(id) =>
                    {
                        return Ok(completed(
                            &request,
                            Ok(Output::RequestOutcome {
                                id: *id,
                                outcome: RequestOutcome::Completed(Box::new(Err(Fault::new(
                                    ErrorCode::Cancelled,
                                    "fixture install cancelled",
                                )))),
                            }),
                        ));
                    }
                    _ => {}
                }
            }
            if matches!(request.command, Command::InspectRequest { .. })
                && self.lose_inspect.swap(false, Ordering::SeqCst)
            {
                return Err(Fault::new(
                    sailry_protocol::ErrorCode::Unavailable,
                    "fixture lost inspection reply",
                ));
            }
            let tracked = matches!(
                request.command,
                Command::SaveKnowledgeSettings(_) | Command::RegisterKnowledgeModel { .. }
            );
            if tracked {
                self.requests.lock().unwrap().push(request.clone());
            }
            let dropped = tracked && self.lose_reply.swap(false, Ordering::SeqCst);
            let read_models = matches!(request.command, Command::ReadKnowledgeModels);
            let mut admission = self.inner.dispatch(request).await?;
            if read_models {
                let states = self.states.clone();
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let completion = std::mem::replace(&mut admission.completion, receiver);
                tokio::spawn(async move {
                    if let Ok(mut result) = completion.await {
                        if let Ok(Output::KnowledgeModels(models)) = &mut result {
                            apply_states(models, &states.lock().unwrap());
                        }
                        _ = sender.send(result);
                    }
                });
            }
            if tracked {
                let outcomes = self.outcomes.clone();
                let id = admission.receipt.id;
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let completion = std::mem::replace(&mut admission.completion, receiver);
                tokio::spawn(async move {
                    if let Ok(result) = completion.await {
                        outcomes.lock().unwrap().push((id, result.clone()));
                        _ = sender.send(result);
                    }
                });
            }
            if dropped {
                self.lose_inspect.store(true, Ordering::SeqCst);
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let completion = std::mem::replace(&mut admission.completion, receiver);
                tokio::spawn(async move {
                    _ = completion.await;
                    drop(sender);
                });
            }
            Ok(admission)
        })
    }
}

pub(in crate::settings) struct Fixture {
    pub directory: tempfile::TempDir,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub node: Node,
    pub other: Node,
    controller: Link,
    pub transport: Arc<Observed>,
}

impl Fixture {
    pub fn new(remote: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = runtime
            .block_on(Node::start_isolated(directory.path().join("node")))
            .unwrap();
        let other = runtime
            .block_on(Node::start_isolated(directory.path().join("other")))
            .unwrap();
        let controller = runtime
            .block_on(Link::controller(
                directory.path().join("controller"),
                NetworkScope::default(),
            ))
            .unwrap();
        let address = runtime
            .block_on(
                controller
                    .handle()
                    .pair(node.link().invite().unwrap().ticket()),
            )
            .unwrap();
        let inner = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let transport = Arc::new(Observed {
            inner,
            requests: Mutex::default(),
            outcomes: Arc::default(),
            lose_reply: AtomicBool::new(false),
            lose_inspect: AtomicBool::new(false),
            hold_install: AtomicBool::new(false),
            catalog: Mutex::default(),
            catalog_more: AtomicBool::new(false),
            states: Arc::default(),
            install: Mutex::default(),
            cancelled: Mutex::default(),
        });
        Self {
            directory,
            runtime,
            node,
            other,
            controller,
            transport,
        }
    }

    pub fn execute(&self, command: Command) -> Output {
        let client = Client::new(self.node.local());
        self.runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    }

    pub fn settings(&self) -> Settings {
        let Output::KnowledgeSettings(settings) = self.execute(Command::ReadKnowledgeSettings)
        else {
            panic!("knowledge settings expected")
        };
        settings
    }

    pub fn mount<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(crate::preferences::Preferences::open(
                self.directory.path().join("preferences.json"),
            ))
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.select(Section::Knowledge, cx);
                workspace.bind_providers(
                    self.transport.clone(),
                    self.runtime.clone(),
                    "Fixture host".into(),
                    cx,
                );
            });
            owner = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let owner = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(960.), px(1100.)));
        wait(visual, |cx| {
            selected(&owner, cx).is_some_and(|panel| {
                panel.read(cx).saved.is_some() && !panel.read(cx).pending && !panel.read(cx).loading
            })
        });
        (owner, visual)
    }

    pub fn bind(&self, owner: &Entity<Workspace>, visual: &mut VisualTestContext, other: bool) {
        let transport: Arc<dyn Transport> = if other {
            self.other.local()
        } else {
            self.transport.clone()
        };
        visual.update(|_, cx| {
            owner.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    transport,
                    self.runtime.clone(),
                    "Selected host".into(),
                    cx,
                )
            })
        });
        wait(visual, |cx| {
            selected(owner, cx).is_some_and(|panel| {
                panel.read(cx).saved.is_some() && !panel.read(cx).pending && !panel.read(cx).loading
            })
        });
    }

    pub fn close(self, visual: &mut VisualTestContext) {
        visual.update(|window, _| window.remove_window());
        self.runtime.block_on(self.node.shutdown()).unwrap();
        self.runtime.block_on(self.other.shutdown()).unwrap();
        self.runtime.block_on(self.controller.close()).unwrap();
    }
}

fn completed(request: &Request, result: Result<Output, Fault>) -> Admission {
    let (sender, completion) = tokio::sync::oneshot::channel();
    sender.send(result).unwrap();
    Admission {
        receipt: Receipt {
            id: request.id,
            durable: request.command.durable(),
        },
        completion,
    }
}

pub(in crate::settings) fn init(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("en");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
}

pub(in crate::settings) fn selected(owner: &Entity<Workspace>, cx: &App) -> Option<Entity<Panel>> {
    let workspace = owner.read(cx);
    let node = workspace.provider_link.as_ref()?.binding.client.target();
    workspace.knowledge.panels.get(&node).cloned()
}

pub(in crate::settings) fn set_state(
    owner: &Entity<Panel>,
    model: Model,
    state: knowledge::ModelState,
    cx: &mut App,
) {
    owner.update(cx, |panel, cx| {
        panel
            .models
            .iter_mut()
            .find(|info| info.model == model)
            .unwrap()
            .state = state;
        cx.notify();
    });
}

pub(in crate::settings) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

pub(in crate::settings) fn wait(cx: &mut VisualTestContext, ready: impl Fn(&App) -> bool) {
    wait_for(cx, ready, |_, _| "knowledge settings UI deadline".into());
}

pub(in crate::settings) fn wait_for(
    cx: &mut VisualTestContext,
    ready: impl Fn(&App) -> bool,
    diagnostic: impl Fn(&mut Window, &App) -> String,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(cx);
        if cx.update(|_, cx| ready(cx)) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{}",
            cx.update(|window, cx| diagnostic(window, cx))
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(in crate::settings) fn tap(cx: &mut VisualTestContext, selector: &str) {
    use gpui_kit::test::TestWindowExt;

    if cx.update(|window, cx| window.has_active_dialog(cx)) {
        std::thread::sleep(*dialog::ANIMATION_DURATION);
    }
    draw(cx);
    cx.update(|window, cx| {
        let selector = SharedString::from(selector.to_owned());
        assert_ne!(window.find(selector.clone()).disabled(), Some(true));
        window.click(selector, cx);
    });
    draw(cx);
}

pub(in crate::settings) fn input(cx: &mut VisualTestContext, selector: &str, value: &str) {
    tap(cx, selector);
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(value);
    draw(cx);
}

pub(in crate::settings) fn toast(cx: &mut VisualTestContext, key: &str) {
    let expected = crate::tr(key);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        if cx.update(|window, cx| {
            !window.notifications(cx).is_empty()
                && crate::feedback::tests::summary(window, cx).contains(expected.as_ref())
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "expected search toast: {key}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
