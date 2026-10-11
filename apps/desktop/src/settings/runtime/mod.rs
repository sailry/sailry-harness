mod index;
mod requests;
#[cfg(test)]
mod tests;
mod view;
mod watch;

use super::{
    Workspace,
    model_library::{self, ManagedModel, ModelKey, ModelStatus},
    providers::Binding,
};
use gpui_kit::{
    component::{
        input::{InputEvent, InputState},
        searchable_list::SearchableListItem,
        select::{SelectEvent, SelectState},
        *,
    },
    *,
};
use index::Index;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, NodeId, Request,
    models::{Definition, Kind, Source},
    ocr::{Model, Settings, State as Status},
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct State {
    panels: BTreeMap<NodeId, Entity<Panel>>,
    selected: Option<NodeId>,
}

pub(super) struct Panel {
    binding: Binding,
    page: Page,
    active: bool,
    settings: Option<Settings>,
    python: Option<sailry_protocol::python::Settings>,
    index: Entity<InputState>,
    source: Entity<SelectState<Vec<Index>>>,
    model: Entity<SelectState<Vec<Choice>>>,
    models: Vec<Model>,
    tab: usize,
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

#[derive(Clone, Copy)]
enum Page {
    Python,
    Ocr,
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
                    crate::feedback::retry("runtime-retry")
                        .debug_selector(|| "runtime-retry".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |panel, cx| {
                                if panel.request.is_some() {
                                    panel.retry(window, cx);
                                } else if let Some(id) = panel.retry_install.take() {
                                    panel.submit(Command::InstallOcrModel { id }, window, cx);
                                } else {
                                    panel.load(window, cx);
                                }
                            });
                        }))
                })
            },
        );
        let index =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://pypi.org/simple/"));
        cx.subscribe_in(&index, window, |panel: &mut Self, _, event, window, cx| {
            if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. })
                && !panel.locked()
                && panel.custom_index(cx)
            {
                let Some(mut settings) = panel.python.clone() else {
                    return;
                };
                let text = panel.index.read(cx).value().trim().to_owned();
                let value = (!text.is_empty()).then_some(text);
                if settings.index != value {
                    settings.index = value;
                    panel.submit(Command::SavePythonSettings(settings), window, cx);
                }
            }
        })
        .detach();
        let source = Self::index_selector(window, cx);
        let model = cx.new(|cx| {
            SelectState::new(
                vec![Choice {
                    value: None,
                    label: crate::tr("search_driver_auto"),
                }],
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        cx.subscribe_in(&model, window, |panel: &mut Self, _, event, window, cx| {
            if matches!(event, SelectEvent::Confirm(_)) && !panel.locked() {
                let Some(mut settings) = panel.settings.clone() else {
                    return;
                };
                let value = panel.model.read(cx).selected_value().cloned().flatten();
                if value != settings.model {
                    settings.model = value;
                    panel.submit(Command::SaveOcrSettings(settings), window, cx);
                }
            }
        })
        .detach();
        Self {
            binding,
            page: Page::Python,
            active: false,
            settings: None,
            python: None,
            index,
            source,
            model,
            models: vec![],
            tab: 0,
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
        self.pending || self.request.is_some() || !self.active
    }
    pub(in crate::settings) fn admits(&self, node: NodeId) -> bool {
        self.active && self.binding.client.target() == node
    }

    fn choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut choices = vec![Choice {
            value: None,
            label: crate::tr("search_driver_auto"),
        }];
        choices.extend(
            self.models
                .iter()
                .filter(|model| model.state == Status::Installed)
                .map(|model| Choice {
                    value: Some(model.id.clone()),
                    label: model.definition.name.clone().into(),
                }),
        );
        let selected = self
            .settings
            .as_ref()
            .and_then(|settings| settings.model.clone())
            .filter(|id| {
                choices
                    .iter()
                    .any(|choice| choice.value.as_ref() == Some(id))
            });
        self.model.update(cx, |state, cx| {
            state.set_items(choices, window, cx);
            state.set_selected_value(&selected, window, cx);
        });
    }

    pub(in crate::settings) fn model_status(&self, definition: &Definition) -> ModelStatus {
        self.models
            .iter()
            .find(|model| model_library::equivalent(&model.definition, definition))
            .map_or(ModelStatus::Missing, |model| status(&model.state))
    }
    pub(in crate::settings) fn source_status(&self, source: &Source, kind: Kind) -> ModelStatus {
        if source.revision.len() != 40
            || !source.revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return ModelStatus::Missing;
        }
        self.models
            .iter()
            .find(|model| {
                model.definition.source == *source && model.definition.format.kind() == kind
            })
            .map_or(ModelStatus::Missing, |model| status(&model.state))
    }
    pub(in crate::settings) fn definition_for(&self, key: [u8; 32]) -> Option<Definition> {
        self.models
            .iter()
            .find(|model| model.id == identifier(key))
            .map(|model| model.definition.clone())
    }
    pub(in crate::settings) fn model_entries(&self, _: &App) -> Vec<ManagedModel> {
        self.models
            .iter()
            .map(|model| ManagedModel {
                key: ModelKey::Ocr(
                    *blake3::Hash::from_hex(&model.id)
                        .expect("Node model ID is valid")
                        .as_bytes(),
                ),
                name: model.definition.name.clone().into(),
                detail: model.definition.source.repository.clone(),
                status: status(&model.state),
                selected: self
                    .settings
                    .as_ref()
                    .is_some_and(|settings| settings.model.as_ref() == Some(&model.id)),
                selectable: false,
                removable: true,
                cancelling: false,
            })
            .collect()
    }
    pub(in crate::settings) fn install(
        &mut self,
        key: [u8; 32],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit(
            Command::InstallOcrModel {
                id: identifier(key),
            },
            window,
            cx,
        );
    }
    pub(in crate::settings) fn cancel(
        &mut self,
        key: [u8; 32],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_transfer(identifier(key), window, cx);
    }
    pub(in crate::settings) fn remove(
        &mut self,
        key: [u8; 32],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit(
            Command::DeleteOcrModel {
                id: identifier(key),
            },
            window,
            cx,
        );
    }
    fn library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        model_library::open(
            model_library::Target::Ocr(cx.entity()),
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

fn identifier(key: [u8; 32]) -> String {
    blake3::Hash::from(key).to_hex().to_string()
}
fn status(value: &Status) -> ModelStatus {
    match value {
        Status::Missing => ModelStatus::Missing,
        Status::Installed => ModelStatus::Installed,
        Status::Downloading {
            percent,
            bytes_per_second,
        } => ModelStatus::Downloading(sailry_model_assets::Progress {
            percent: f32::from(*percent),
            bytes_per_second: *bytes_per_second as f64,
        }),
    }
}

impl Workspace {
    pub(super) fn runtime(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let binding = self.provider_link.as_ref().map(|live| live.binding.clone());
        let selected = binding.as_ref().map(|binding| binding.client.target());
        if self.runtime.selected != selected {
            self.clear_runtime(cx);
            self.runtime.selected = selected;
        }
        let Some(binding) = binding else {
            return super::group::Group::new("runtime_python")
                .empty(IconName::Inbox, "knowledge_host_required")
                .into_any_element();
        };
        let panel = self
            .runtime
            .panels
            .entry(binding.client.target())
            .or_insert_with(|| cx.new(|cx| Panel::new(binding, window, cx)))
            .clone();
        let page = if self.section == super::Section::Ocr {
            Page::Ocr
        } else {
            Page::Python
        };
        panel.update(cx, |panel, cx| {
            panel.page = page;
            if !panel.active {
                panel.active = true;
                panel.watch(window, cx);
                panel.load(window, cx);
            }
        });
        panel.into_any_element()
    }
    pub(super) fn clear_runtime(&mut self, cx: &mut Context<Self>) {
        if let Some(node) = self.runtime.selected.take()
            && let Some(panel) = self.runtime.panels.get(&node)
        {
            panel.update(cx, |panel, cx| panel.deactivate(cx));
        }
    }
}

fn error_key(code: sailry_protocol::ErrorCode) -> &'static str {
    match code {
        sailry_protocol::ErrorCode::RevisionConflict => "runtime_conflict",
        sailry_protocol::ErrorCode::InvalidRequest => "runtime_invalid",
        sailry_protocol::ErrorCode::PermissionDenied => "runtime_denied",
        sailry_protocol::ErrorCode::OutcomeUnknown => "runtime_unknown",
        _ => "runtime_failed",
    }
}
