mod consent;
pub(super) mod requests;
mod view;
mod watch;

use super::{Workspace, providers::Binding};
use gpui_kit::{
    component::{searchable_list::SearchableListItem, select::SelectState, *},
    *,
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    NodeId, Request,
    knowledge::{self, Model, ModelInfo, Settings},
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
    saved: Option<Settings>,
    model: Entity<SelectState<Vec<ModelChoice>>>,
    models: Vec<ModelInfo>,
    tab: usize,
    draft: bool,
    loading: bool,
    refresh: bool,
    rebase: bool,
    pending: bool,
    cancelling: Option<Model>,
    request: Option<Request>,
    error: Option<&'static str>,
    stop: Option<CancellationToken>,
    generation: u64,
}

#[derive(Clone)]
struct ModelChoice {
    value: Option<Model>,
    label: SharedString,
}

impl SearchableListItem for ModelChoice {
    type Value = Option<Model>;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

fn model_label(info: &ModelInfo) -> SharedString {
    match info.model {
        Model::Registered { .. } => format!("{} · {}", info.definition.name, info.revision).into(),
        _ => info.definition.name.clone().into(),
    }
}

fn choices() -> Vec<ModelChoice> {
    vec![ModelChoice {
        value: None,
        label: crate::tr("knowledge_keyword_only"),
    }]
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
            |_, key, cx| {
                let owner = cx.weak_entity();
                notification::Notification::error(crate::tr(key)).action(move |_, _, cx| {
                    let owner = owner.clone();
                    let action = if key == "knowledge_unknown" {
                        crate::feedback::retry("knowledge-retry")
                    } else {
                        button::Button::new("knowledge-retry").label(crate::tr("knowledge_reload"))
                    };
                    action
                        .debug_selector(|| "knowledge-retry".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |panel, cx| {
                                if key == "knowledge_unknown" {
                                    panel.retry(window, cx);
                                } else {
                                    panel.rebase = true;
                                    panel.load(window, cx);
                                }
                            });
                        }))
                })
            },
        );
        let model = cx.new(|cx| SelectState::new(choices(), Some(IndexPath::new(0)), window, cx));
        cx.subscribe_in(&model, window, |panel: &mut Self, _, event, window, cx| {
            if matches!(event, select::SelectEvent::Confirm(_)) && !panel.locked() {
                panel.draft = true;
                if panel.dirty(cx) {
                    panel.save(window, cx);
                }
            }
        })
        .detach();
        Self {
            binding,
            active: false,
            saved: None,
            model,
            models: Vec::new(),
            tab: 0,
            draft: false,
            loading: false,
            refresh: false,
            rebase: false,
            pending: false,
            cancelling: None,
            request: None,
            error: None,
            stop: None,
            generation: 0,
        }
    }

    fn settings(&self, cx: &App) -> Option<Settings> {
        self.saved.as_ref().map(|saved| Settings {
            revision: saved.revision,
            model: self.model.read(cx).selected_value().copied().flatten(),
            mirror: saved.mirror.clone(),
        })
    }

    fn dirty(&self, cx: &App) -> bool {
        self.draft && self.settings(cx) != self.saved
    }

    pub(super) fn locked(&self) -> bool {
        !self.active || self.pending || self.request.is_some() || self.saved.is_none()
    }

    fn apply(
        &mut self,
        settings: Settings,
        preserve: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !preserve {
            self.model.update(cx, |model, cx| {
                model.set_selected_value(&settings.model, window, cx)
            });
            self.draft = false;
        }
        self.saved = Some(settings);
    }

    fn model_name(&self, model: Model) -> SharedString {
        self.models
            .iter()
            .find(|info| info.model == model)
            .map_or_else(
                || model.key().into_owned().into(),
                |info| info.definition.name.clone().into(),
            )
    }

    fn update_choices(
        &mut self,
        models: Vec<ModelInfo>,
        selected: Option<Model>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Model> {
        self.models = models;
        let mut options = self
            .models
            .iter()
            .filter(|info| info.state == knowledge::ModelState::Installed)
            .map(|info| ModelChoice {
                value: Some(info.model),
                label: model_label(info),
            })
            .collect::<Vec<_>>();
        let selected = selected
            .filter(|model| options.iter().any(|option| option.value == Some(*model)))
            .or_else(|| selected.and_then(|_| options.first().and_then(|option| option.value)));
        options.push(ModelChoice {
            value: None,
            label: crate::tr("knowledge_keyword_only"),
        });
        self.model.update(cx, |state, cx| {
            state.set_items(options, window, cx);
            state.set_selected_value(&selected, window, cx);
        });
        selected
    }

    fn fallback(
        &mut self,
        selected: Option<Model>,
        chosen: Option<Model>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if selected.is_some()
            && chosen.is_some()
            && selected != chosen
            && !self.locked()
            && !self.dirty(cx)
        {
            self.draft = true;
            self.save(window, cx);
        }
    }

    pub(super) fn library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let library = super::model_library::open(
            super::model_library::Target::Knowledge(cx.entity()),
            self.binding.clone(),
            window,
            cx,
        );
        {
            drop(library);
        }
    }

    pub(super) fn admits(&self, node: NodeId) -> bool {
        self.active && self.binding.client.target() == node
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
    pub(super) fn knowledge(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let binding = self.provider_link.as_ref().map(|live| live.binding.clone());
        let selected = binding.as_ref().map(|binding| binding.client.target());
        if self.knowledge.selected != selected {
            self.clear_knowledge(cx);
            self.knowledge.selected = selected;
        }
        let Some(binding) = binding else {
            return crate::settings::group::Group::new("knowledge_models")
                .empty(IconName::Inbox, "knowledge_host_required")
                .into_any_element();
        };
        let panel = self
            .knowledge
            .panels
            .entry(binding.client.target())
            .or_insert_with(|| cx.new(|cx| Panel::new(binding, window, cx)))
            .clone();
        panel.update(cx, |panel, cx| {
            if !panel.active {
                panel.active = true;
                panel.watch(window, cx);
                panel.load(window, cx);
            }
        });
        panel.into_any_element()
    }

    pub(super) fn clear_knowledge(&mut self, cx: &mut Context<Self>) {
        if let Some(node) = self.knowledge.selected.take()
            && let Some(panel) = self.knowledge.panels.get(&node)
        {
            panel.update(cx, |panel, cx| panel.deactivate(cx));
        }
    }
}

impl Panel {
    pub(in crate::settings) fn model_status(
        &self,
        definition: &sailry_protocol::models::Definition,
    ) -> super::model_library::ModelStatus {
        availability(
            self.models
                .iter()
                .filter(|info| super::model_library::equivalent(&info.definition, definition)),
        )
    }

    pub(in crate::settings) fn source_status(
        &self,
        source: &sailry_protocol::models::Source,
        kind: sailry_protocol::models::Kind,
    ) -> super::model_library::ModelStatus {
        if source.revision.len() != 40
            || !source.revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return super::model_library::ModelStatus::Missing;
        }
        availability(self.models.iter().filter(|info| {
            info.definition.source == *source && info.definition.format.kind() == kind
        }))
    }

    pub(super) fn model_rows(
        &self,
        owner: Entity<Self>,
        library: bool,
        cx: &App,
    ) -> Vec<super::entry::Entry> {
        use super::model_library::{ModelStatus, Target};
        let models = self
            .model_entries(cx)
            .into_iter()
            .filter(|model| matches!(model.status, ModelStatus::Installed) == (self.tab == 0))
            .collect();
        super::model_library::model_rows(
            Target::Knowledge(owner),
            models,
            self.locked(),
            library,
            cx,
        )
    }

    pub(in crate::settings) fn definition_for(
        &self,
        model: Model,
    ) -> Option<sailry_protocol::models::Definition> {
        self.models
            .iter()
            .find(|info| info.model == model)
            .map(|info| info.definition.clone())
    }

    pub(in crate::settings) fn model_entries(
        &self,
        _cx: &App,
    ) -> Vec<super::model_library::ManagedModel> {
        use super::model_library::{ManagedModel, ModelKey};
        self.models
            .iter()
            .filter(|info| {
                matches!(info.model, Model::Registered { .. })
                    || !self.models.iter().any(|candidate| {
                        matches!(candidate.model, Model::Registered { .. })
                            && candidate.definition.source.repository
                                == info.definition.source.repository
                            && candidate.definition.source.hub == info.definition.source.hub
                            && (info.state != knowledge::ModelState::Installed
                                || candidate.state == knowledge::ModelState::Installed)
                    })
            })
            .map(|info| ManagedModel {
                key: ModelKey::Embedding(info.model),
                name: info.definition.name.clone().into(),
                detail: match info.model {
                    Model::GraniteMulti | Model::PotionMulti => {
                        crate::tr("knowledge_multilingual").to_string()
                    }
                    Model::BgeSmallZh => crate::tr("knowledge_chinese").to_string(),
                    Model::Registered { .. } => info.repository.clone(),
                },
                status: status(&info.state),
                selected: false,
                selectable: false,
                removable: info.state == knowledge::ModelState::Installed,
                cancelling: !self.active || self.cancelling.is_some(),
            })
            .collect()
    }

    pub(super) fn remove_model(
        owner: Entity<Self>,
        model: Model,
        window: &mut Window,
        cx: &mut App,
    ) {
        consent::model(owner, model, window, cx);
    }
}

fn availability<'a>(
    models: impl Iterator<Item = &'a ModelInfo>,
) -> super::model_library::ModelStatus {
    models
        .max_by_key(|info| match &info.state {
            knowledge::ModelState::Installed => 2,
            knowledge::ModelState::Downloading { .. } => 1,
            knowledge::ModelState::Missing => 0,
        })
        .map_or(super::model_library::ModelStatus::Missing, |info| {
            status(&info.state)
        })
}

fn status(state: &knowledge::ModelState) -> super::model_library::ModelStatus {
    use super::model_library::ModelStatus;
    match state {
        knowledge::ModelState::Missing => ModelStatus::Missing,
        knowledge::ModelState::Downloading {
            percent,
            bytes_per_second,
        } => ModelStatus::Downloading(sailry_model_assets::Progress {
            phase: sailry_model_assets::Phase::Downloading,
            percent: f32::from(*percent),
            bytes_per_second: *bytes_per_second as f64,
        }),
        knowledge::ModelState::Installed => ModelStatus::Installed,
    }
}
