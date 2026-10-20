//! One Node-scoped owner for local model metadata, selection and installation.
mod requests;
mod view;
mod watch;

use super::{
    Workspace,
    model_library::{self, ManagedModel, ModelKey, ModelStatus},
    providers::Binding,
};
use gpui_kit::{
    component::{
        searchable_list::SearchableListItem,
        select::{SelectEvent, SelectState},
        *,
    },
    *,
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, NodeId, Request,
    models::{
        Availability, Choice as Selection, Definition, Kind, Model, Settings, Source,
        State as Status,
    },
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct State {
    panels: BTreeMap<NodeId, Entity<Panel>>,
    selected: Option<NodeId>,
}

pub(super) struct Panel {
    binding: Binding,
    active: bool,
    kind: Kind,
    settings: Option<Settings>,
    selectors: BTreeMap<Kind, Entity<SelectState<Vec<Choice>>>>,
    models: Vec<Model>,
    availability: BTreeMap<Kind, Availability>,
    tabs: BTreeMap<Kind, usize>,
    pending: bool,
    loading: bool,
    refresh: bool,
    request: Option<Request>,
    stop: Option<CancellationToken>,
    generation: u64,
    error: Option<&'static str>,
    fault: Option<sailry_protocol::Fault>,
    retry_install: Option<String>,
}

#[derive(Clone)]
struct Choice {
    value: Option<String>,
    label: SharedString,
}
impl SearchableListItem for Choice {
    type Value = Option<String>;
    fn title(&self) -> SharedString {
        self.label.clone()
    }
    fn value(&self) -> &Self::Value {
        &self.value
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.cancel();
        }
    }
}

impl Panel {
    fn new(binding: Binding, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::feedback::observe_with(
            window,
            cx,
            |panel: &Self, _| panel.error.into_iter().collect(),
            |panel, key, cx| {
                let owner = cx.weak_entity();
                let notice = panel.fault.as_ref().map_or_else(
                    || notification::Notification::error(crate::tr(key)),
                    |fault| {
                        crate::feedback::diagnostic(fault.message.clone().into())
                            .title(crate::tr(key))
                    },
                );
                notice.action(move |_, _, cx| {
                    let owner = owner.clone();
                    crate::feedback::retry("local-model-retry").on_click(cx.listener(
                        move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |panel, cx| {
                                if panel.request.is_some() {
                                    panel.retry(window, cx);
                                } else if let Some(id) = panel.retry_install.take() {
                                    panel.submit(Command::InstallLocalModel { id }, window, cx);
                                } else {
                                    panel.load(window, cx);
                                }
                            });
                        },
                    ))
                })
            },
        );
        let selectors = [Kind::Embedding, Kind::Ocr, Kind::Decision]
            .into_iter()
            .map(|kind| {
                let selector =
                    cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx));
                cx.subscribe_in(
                    &selector,
                    window,
                    move |panel: &mut Self, _, event, window, cx| {
                        if !matches!(event, SelectEvent::Confirm(_)) || panel.locked() {
                            return;
                        }
                        let Some(mut settings) = panel.settings.clone() else {
                            return;
                        };
                        let model = panel.selectors[&kind]
                            .read(cx)
                            .selected_value()
                            .cloned()
                            .flatten();
                        let previous = settings.choice(kind);
                        let selection = Selection {
                            enabled: if kind == Kind::Embedding {
                                model.is_some()
                            } else {
                                previous.enabled
                            },
                            model,
                        };
                        if previous != selection {
                            settings.choices.insert(kind, selection);
                            panel.submit(Command::SaveLocalModelSettings(settings), window, cx);
                        }
                    },
                )
                .detach();
                (kind, selector)
            })
            .collect();
        Self {
            binding,
            active: false,
            kind: Kind::Embedding,
            settings: None,
            selectors,
            models: Vec::new(),
            availability: BTreeMap::new(),
            tabs: BTreeMap::new(),
            pending: false,
            loading: false,
            refresh: false,
            request: None,
            stop: None,
            generation: 0,
            error: None,
            fault: None,
            retry_install: None,
        }
    }

    pub(in crate::settings) fn locked(&self) -> bool {
        !self.active || self.pending || self.request.is_some()
    }
    pub(in crate::settings) fn available(&self, kind: Kind) -> bool {
        self.availability
            .get(&kind)
            .is_some_and(Availability::available)
    }

    pub(in crate::settings) fn admits(&self, node: NodeId) -> bool {
        self.active && self.binding.client.target() == node
    }

    fn choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (kind, selector) in &self.selectors {
            let selection = self
                .settings
                .as_ref()
                .map(|settings| settings.choice(*kind));
            let mut choices = vec![Choice {
                value: None,
                label: crate::tr(if *kind == Kind::Embedding {
                    "knowledge_keyword_only"
                } else if *kind == Kind::Decision {
                    "local_models_none"
                } else {
                    "search_driver_auto"
                }),
            }];
            choices.extend(
                self.models
                    .iter()
                    .filter(|model| {
                        model.definition.format.kind() == *kind && model.state == Status::Installed
                    })
                    .map(|model| Choice {
                        value: Some(model.id.clone()),
                        label: model.definition.name.clone().into(),
                    }),
            );
            let selected = selection.and_then(|selection| {
                if *kind == Kind::Embedding && !selection.enabled {
                    None
                } else {
                    selection.model
                }
            });
            // Keep a saved missing selection visible without silently changing configuration.
            if let Some(id) = &selected
                && !choices
                    .iter()
                    .any(|choice| choice.value.as_ref() == Some(id))
            {
                choices.push(Choice {
                    value: Some(id.clone()),
                    label: self
                        .models
                        .iter()
                        .find(|model| &model.id == id)
                        .map_or_else(
                            || id.clone().into(),
                            |model| model.definition.name.clone().into(),
                        ),
                });
            }
            selector.update(cx, |state, cx| {
                state.set_items(choices, window, cx);
                state.set_selected_value(&selected, window, cx);
            });
        }
    }

    pub(in crate::settings) fn model_status(&self, definition: &Definition) -> ModelStatus {
        availability(
            self.models
                .iter()
                .filter(|model| model_library::equivalent(&model.definition, definition)),
        )
    }
    pub(in crate::settings) fn source_status(&self, source: &Source, kind: Kind) -> ModelStatus {
        if source.revision.len() != 40
            || !source.revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return ModelStatus::Missing;
        }
        availability(self.models.iter().filter(|model| {
            model.definition.source == *source && model.definition.format.kind() == kind
        }))
    }
    pub(in crate::settings) fn definition_for(&self, id: &str) -> Option<Definition> {
        self.models
            .iter()
            .find(|model| model.id == id)
            .map(|model| model.definition.clone())
    }
    pub(in crate::settings) fn model_entries(&self, kind: Kind) -> Vec<ManagedModel> {
        self.models
            .iter()
            .filter(|model| model.definition.format.kind() == kind)
            .map(|model| ManagedModel {
                key: ModelKey::Node(kind, model.id.clone()),
                name: model.definition.name.clone().into(),
                detail: model.definition.source.repository.clone(),
                status: status(&model.state),
                selected: self
                    .settings
                    .as_ref()
                    .and_then(|settings| settings.selected(kind))
                    == Some(model.id.as_str()),
                selectable: false,
                removable: model.state == Status::Installed,
                cancelling: !self.active,
            })
            .collect()
    }
    pub(in crate::settings) fn install(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit(Command::InstallLocalModel { id }, window, cx);
    }
    pub(in crate::settings) fn cancel(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_transfer(id, window, cx);
    }
    pub(in crate::settings) fn remove(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit(Command::DeleteLocalModel { id }, window, cx);
    }
    fn library(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        model_library::open(
            model_library::Target::Node(cx.entity(), kind),
            self.binding.clone(),
            window,
            cx,
        );
    }
    fn deactivate(&mut self, cx: &mut Context<Self>) {
        self.active = false;
        self.generation += 1;
        if let Some(stop) = self.stop.take() {
            stop.cancel();
        }
        cx.notify();
    }
}

impl Workspace {
    pub(super) fn local_models(
        &mut self,
        kind: Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let binding = self.provider_link.as_ref().map(|live| live.binding.clone());
        let selected = binding.as_ref().map(|binding| binding.client.target());
        if self.models.selected != selected {
            self.clear_models(cx);
            self.models.selected = selected;
        }
        let Some(binding) = binding else {
            return super::group::Group::new("local_models")
                .empty(IconName::Inbox, "knowledge_host_required")
                .into_any_element();
        };
        let panel = self
            .models
            .panels
            .entry(binding.client.target())
            .or_insert_with(|| cx.new(|cx| Panel::new(binding, window, cx)))
            .clone();
        panel.update(cx, |panel, cx| {
            panel.kind = kind;
            if !panel.active {
                panel.active = true;
                panel.watch(window, cx);
                panel.load(window, cx);
            }
        });
        panel.into_any_element()
    }
    pub(super) fn clear_models(&mut self, cx: &mut Context<Self>) {
        if let Some(node) = self.models.selected.take()
            && let Some(panel) = self.models.panels.get(&node)
        {
            panel.update(cx, |panel, cx| panel.deactivate(cx));
        }
    }
}

fn availability<'a>(models: impl Iterator<Item = &'a Model>) -> ModelStatus {
    models
        .max_by_key(|model| match model.state {
            Status::Installed => 2,
            Status::Downloading { .. } => 1,
            Status::Missing => 0,
        })
        .map_or(ModelStatus::Missing, |model| status(&model.state))
}
fn status(value: &Status) -> ModelStatus {
    match value {
        Status::Missing => ModelStatus::Missing,
        Status::Installed => ModelStatus::Installed,
        Status::Downloading {
            percent,
            bytes_per_second,
        } => ModelStatus::Downloading(sailry_model_assets::Progress {
            phase: sailry_model_assets::Phase::Downloading,
            percent: f32::from(*percent),
            bytes_per_second: *bytes_per_second as f64,
        }),
    }
}
fn error_key(code: sailry_protocol::ErrorCode) -> &'static str {
    super::runtime::error_key(code)
}
