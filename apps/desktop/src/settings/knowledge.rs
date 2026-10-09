mod consent;
mod editor;
pub(super) mod requests;
#[cfg(test)]
pub(super) mod tests;
mod view;
mod watch;

use super::{Workspace, providers::Binding};
use gpui_kit::{
    component::{input::InputState, searchable_list::SearchableListItem, select::SelectState, *},
    *,
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    NodeId, Request, Worktree,
    knowledge::{self, Collection, Model, ModelInfo, Settings},
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
    mirror: Entity<InputState>,
    models: Vec<ModelInfo>,
    collections: Vec<Collection>,
    worktrees: Vec<Worktree>,
    loading: bool,
    refresh: bool,
    rebase: bool,
    pending: bool,
    cancelling: Option<Model>,
    request: Option<Request>,
    error: Option<&'static str>,
    stop: Option<CancellationToken>,
    generation: u64,
    #[cfg(test)]
    editor: Option<WeakEntity<editor::Editor>>,
    #[cfg(test)]
    pub(super) library: Option<WeakEntity<super::model_library::Library>>,
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

fn model_key(model: Model) -> &'static str {
    match model {
        Model::GraniteMulti => "knowledge_model_granite",
        Model::BgeSmallZh => "knowledge_model_bge",
        Model::PotionMulti => "knowledge_model_potion",
        Model::Registered { .. } => "knowledge_models",
    }
}

fn model_label(info: &ModelInfo) -> SharedString {
    match info.model {
        Model::Registered { .. } => format!("{} · {}", info.definition.name, info.revision).into(),
        _ => info.definition.name.clone().into(),
    }
}

fn choices() -> Vec<ModelChoice> {
    Model::ALL
        .into_iter()
        .map(|model| ModelChoice {
            value: Some(model),
            label: crate::tr(model_key(model)),
        })
        .chain(std::iter::once(ModelChoice {
            value: None,
            label: crate::tr("knowledge_keyword_only"),
        }))
        .collect()
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
                    button::Button::new("knowledge-retry")
                        .debug_selector(|| "knowledge-retry".into())
                        .label(crate::tr(if key == "knowledge_unknown" {
                            "knowledge_retry"
                        } else {
                            "knowledge_reload"
                        }))
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
        Self {
            binding,
            active: false,
            saved: None,
            model: cx.new(|cx| SelectState::new(choices(), Some(IndexPath::new(0)), window, cx)),
            mirror: cx.new(|cx| {
                InputState::new(window, cx).placeholder(crate::tr("knowledge_mirror_default"))
            }),
            models: Vec::new(),
            collections: Vec::new(),
            worktrees: Vec::new(),
            loading: false,
            refresh: false,
            rebase: false,
            pending: false,
            cancelling: None,
            request: None,
            error: None,
            stop: None,
            generation: 0,
            #[cfg(test)]
            editor: None,
            #[cfg(test)]
            library: None,
        }
    }

    fn settings(&self, cx: &App) -> Option<Settings> {
        self.saved.as_ref().map(|saved| Settings {
            revision: saved.revision,
            model: self.model.read(cx).selected_value().copied().flatten(),
            mirror: {
                let value = self.mirror.read(cx).value().trim().to_owned();
                (!value.is_empty()).then_some(value)
            },
        })
    }

    fn dirty(&self, cx: &App) -> bool {
        self.settings(cx) != self.saved
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
            self.mirror.update(cx, |mirror, cx| {
                mirror.set_value(settings.mirror.clone().unwrap_or_default(), window, cx)
            });
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
    ) {
        self.models = models;
        let mut options = self
            .models
            .iter()
            .map(|info| ModelChoice {
                value: Some(info.model),
                label: model_label(info),
            })
            .collect::<Vec<_>>();
        if let Some(model) = selected
            && !options.iter().any(|option| option.value == Some(model))
        {
            options.push(ModelChoice {
                value: Some(model),
                label: self.model_name(model),
            });
        }
        options.push(ModelChoice {
            value: None,
            label: crate::tr("knowledge_keyword_only"),
        });
        self.model.update(cx, |state, cx| {
            state.set_items(options, window, cx);
            state.set_selected_value(&selected, window, cx);
        });
    }

    pub(super) fn library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let library = super::model_library::open(
            super::model_library::Target::Knowledge(cx.entity()),
            self.binding.clone(),
            window,
            cx,
        );
        #[cfg(test)]
        {
            self.library = Some(library.downgrade());
        }
        #[cfg(not(test))]
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
            return crate::settings::group::Group::new("knowledge_collections")
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
    pub(super) fn model_rows(
        &self,
        owner: Entity<Self>,
        library: bool,
        cx: &App,
    ) -> Vec<super::entry::Entry> {
        use super::model_library::{ManagedModel, ModelKey, ModelStatus, Target};
        let models = self
            .models
            .iter()
            .map(|info| ManagedModel {
                key: ModelKey::Embedding(info.model),
                name: info.definition.name.clone().into(),
                detail: format!(
                    "{} · {} · {:.1} MiB",
                    info.repository,
                    info.revision,
                    info.size_bytes as f64 / (1024. * 1024.)
                ),
                status: match info.state {
                    knowledge::ModelState::Missing => ModelStatus::Missing,
                    knowledge::ModelState::Downloading {
                        percent,
                        bytes_per_second,
                    } => ModelStatus::Downloading(sailry_model_assets::Progress {
                        percent: f32::from(percent),
                        bytes_per_second: bytes_per_second as f64,
                    }),
                    knowledge::ModelState::Installed => ModelStatus::Installed,
                },
                selected: false,
                selectable: false,
                removable: info.state == knowledge::ModelState::Installed,
                cancelling: !self.active || self.cancelling.is_some(),
            })
            .collect();
        super::model_library::model_rows(
            Target::Knowledge(owner),
            models,
            self.locked(),
            library,
            cx,
        )
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
