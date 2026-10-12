//! Public model catalog composition shared by the two model-library entry points.
mod details;
mod managed;
mod requests;
#[cfg(test)]
mod tests;
mod view;
pub(super) use managed::rows as model_rows;

use super::{dictation, knowledge, providers::Binding, runtime};
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::{
    component::{
        input::{InputEvent, InputState},
        searchable_list::SearchableListItem,
        select::{SelectEvent, SelectState},
        *,
    },
    *,
};
use sailry_protocol::{
    Command, ModelId, NodeId,
    knowledge::Model,
    models::{Definition, Hub, Item, Kind, Source, Version},
};

#[derive(Clone)]
pub(super) enum Target {
    Knowledge(Entity<knowledge::Panel>),
    Speech(Entity<dictation::Panel>),
    Ocr(Entity<runtime::Panel>),
}

impl Target {
    fn kind(&self) -> Kind {
        match self {
            Self::Knowledge(_) => Kind::Embedding,
            Self::Speech(_) => Kind::Speech,
            Self::Ocr(_) => Kind::Ocr,
        }
    }
    fn available(&self, node: NodeId, cx: &App) -> bool {
        match self {
            Self::Knowledge(panel) => panel.read(cx).admits(node),
            Self::Speech(panel) => panel.read(cx).admits(node),
            Self::Ocr(panel) => panel.read(cx).admits(node),
        }
    }
    fn busy(&self, cx: &App) -> bool {
        match self {
            Self::Knowledge(panel) => panel.read(cx).locked(),
            Self::Speech(_) => false,
            Self::Ocr(panel) => panel.read(cx).locked(),
        }
    }

    fn model_status(&self, definition: &Definition, cx: &App) -> ModelStatus {
        match self {
            Self::Knowledge(panel) => panel.read(cx).model_status(definition),
            Self::Speech(panel) => panel.read(cx).model_status(definition),
            Self::Ocr(panel) => panel.read(cx).model_status(definition),
        }
    }

    fn source_status(&self, source: &Source, cx: &App) -> ModelStatus {
        match self {
            Self::Knowledge(panel) => panel.read(cx).source_status(source, self.kind()),
            Self::Speech(panel) => panel.read(cx).source_status(source, self.kind()),
            Self::Ocr(panel) => panel.read(cx).source_status(source, self.kind()),
        }
    }

    fn download(&self, definition: Definition, window: &mut Window, cx: &mut App) {
        match self {
            Self::Knowledge(panel) => panel.update(cx, |panel, cx| {
                panel.submit(Command::RegisterKnowledgeModel { definition }, window, cx)
            }),
            Self::Speech(panel) => {
                panel.update(cx, |panel, cx| panel.register(definition, window, cx))
            }
            Self::Ocr(panel) => panel.update(cx, |panel, cx| {
                panel.submit(Command::RegisterOcrModel { definition }, window, cx)
            }),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ModelKey {
    Embedding(Model),
    Speech(Option<ModelId>),
    Ocr([u8; 32]),
}

#[derive(Clone, Copy)]
pub(super) enum ModelStatus {
    Missing,
    Downloading(sailry_model_assets::Progress),
    Installed,
    Checking,
    Unavailable,
    Failed,
}

pub(super) struct ManagedModel {
    pub(super) key: ModelKey,
    pub(super) name: SharedString,
    pub(super) detail: String,
    pub(super) status: ModelStatus,
    pub(super) selected: bool,
    pub(super) selectable: bool,
    pub(super) removable: bool,
    pub(super) cancelling: bool,
}

pub(super) fn equivalent(left: &Definition, right: &Definition) -> bool {
    left.source == right.source
        && left.format == right.format
        && left.query_prefix == right.query_prefix
        && left.document_prefix == right.document_prefix
        && left.assets.len() == right.assets.len()
        && left.assets.iter().all(|asset| right.assets.contains(asset))
}

impl Target {
    fn install(&self, key: ModelKey, window: &mut Window, cx: &mut App) {
        match (self, key) {
            (Self::Knowledge(panel), ModelKey::Embedding(model)) => {
                panel.update(cx, |panel, cx| panel.install(model, window, cx))
            }
            (Self::Speech(panel), ModelKey::Speech(Some(id))) => {
                panel.update(cx, |panel, cx| panel.update_model(id, window, cx));
            }
            (Self::Speech(panel), ModelKey::Speech(None)) => {
                panel.update(cx, |panel, cx| panel.install(None, window, cx));
            }
            (Self::Ocr(panel), ModelKey::Ocr(id)) => {
                panel.update(cx, |panel, cx| panel.install(id, window, cx))
            }
            _ => unreachable!("model row belongs to its target"),
        }
    }
    fn update(&self, key: ModelKey, window: &mut Window, cx: &mut App) {
        match (self, key) {
            (Self::Knowledge(panel), ModelKey::Embedding(model)) => {
                panel.update(cx, |panel, cx| panel.install(model, window, cx));
            }
            (Self::Speech(panel), ModelKey::Speech(Some(id))) => {
                panel.update(cx, |panel, cx| panel.update_model(id, window, cx));
            }
            (Self::Ocr(panel), ModelKey::Ocr(id)) => {
                panel.update(cx, |panel, cx| panel.update_model(id, window, cx))
            }
            _ => unreachable!("only registered models can be updated"),
        }
    }
    fn cancel(&self, key: ModelKey, window: &mut Window, cx: &mut App) {
        match (self, key) {
            (Self::Knowledge(panel), ModelKey::Embedding(model)) => {
                panel.update(cx, |panel, cx| panel.cancel(model, window, cx));
            }
            (Self::Speech(panel), ModelKey::Speech(id)) => {
                panel.update(cx, |panel, cx| panel.cancel(id, cx));
            }
            (Self::Ocr(panel), ModelKey::Ocr(id)) => {
                panel.update(cx, |panel, cx| panel.cancel(id, window, cx))
            }
            _ => unreachable!("model row belongs to its target"),
        }
    }
    fn select(&self, key: ModelKey, window: &mut Window, cx: &mut App) {
        match (self, key) {
            (Self::Speech(panel), ModelKey::Speech(id)) => {
                panel.update(cx, |panel, cx| panel.select(id, window, cx));
            }
            _ => unreachable!("only speech rows select controller models"),
        }
    }
    fn remove(&self, key: ModelKey, name: SharedString, window: &mut Window, cx: &mut App) {
        match (self, key) {
            (Self::Knowledge(panel), ModelKey::Embedding(model)) => {
                knowledge::Panel::remove_model(panel.clone(), model, window, cx);
            }
            (Self::Speech(panel), ModelKey::Speech(Some(id))) => {
                let panel = panel.clone();
                let title = tr("knowledge_delete");
                crate::prompts::confirm(
                    &title,
                    &name,
                    tr("knowledge_delete"),
                    window,
                    cx,
                    move |window, cx| panel.update(cx, |panel, cx| panel.remove(id, window, cx)),
                );
            }
            (Self::Ocr(panel), ModelKey::Ocr(id)) => {
                let panel = panel.clone();
                crate::prompts::confirm(
                    &tr("knowledge_delete"),
                    &name,
                    tr("knowledge_delete"),
                    window,
                    cx,
                    move |window, cx| panel.update(cx, |panel, cx| panel.remove(id, window, cx)),
                );
            }
            _ => unreachable!("recommended speech model is retained"),
        }
    }
}

#[derive(Clone)]
struct HubChoice(Hub);
impl SearchableListItem for HubChoice {
    type Value = Hub;
    fn title(&self) -> SharedString {
        match self.0 {
            Hub::HuggingFace => "huggingface.co",
            Hub::ModelScope => "modelscope.cn",
        }
        .into()
    }
    fn value(&self) -> &Hub {
        &self.0
    }
}

#[derive(Clone)]
struct VersionChoice(Version);
impl SearchableListItem for VersionChoice {
    type Value = String;
    fn title(&self) -> SharedString {
        self.0.name.clone().into()
    }
    fn value(&self) -> &String {
        &self.0.revision
    }
}

pub(super) struct Library {
    target: Target,
    binding: Binding,
    query: Entity<InputState>,
    hub: Entity<SelectState<Vec<HubChoice>>>,
    version: Entity<SelectState<Vec<VersionChoice>>>,
    items: Vec<Item>,
    repository: Option<String>,
    definition: Option<Definition>,
    installing: Option<Source>,
    query_prefix: Entity<InputState>,
    document_prefix: Entity<InputState>,
    cursor: Option<String>,
    searched: Option<(Hub, String)>,
    partial: bool,
    loading: bool,
    generation: u64,
    closed: bool,
    read: Option<requests::Read>,
}

pub(super) fn open(
    target: Target,
    binding: Binding,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Library> {
    let library = cx.new(|cx| {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(tr("model_library_query")));
        let hub = cx.new(|cx| {
            SelectState::new(
                vec![HubChoice(Hub::HuggingFace), HubChoice(Hub::ModelScope)],
                Some(IndexPath::new(if target.kind() == Kind::Ocr {
                    1
                } else {
                    0
                })),
                window,
                cx,
            )
        });
        let version = cx.new(|cx| SelectState::new(Vec::<VersionChoice>::new(), None, window, cx));
        let query_prefix = cx.new(|cx| InputState::new(window, cx));
        let document_prefix = cx.new(|cx| InputState::new(window, cx));
        cx.observe(&query_prefix, |_: &mut Library, _, cx| cx.notify())
            .detach();
        cx.observe(&document_prefix, |_: &mut Library, _, cx| cx.notify())
            .detach();
        cx.subscribe_in(
            &query,
            window,
            |library: &mut Library, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    library.search(false, window, cx);
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &hub,
            window,
            |library: &mut Library, _, event, window, cx| {
                if matches!(event, SelectEvent::Confirm(_)) {
                    library.cancel();
                    library.items.clear();
                    library.repository = None;
                    library.definition = None;
                    library.cursor = None;
                    library.searched = None;
                    library.partial = false;
                    library
                        .version
                        .update(cx, |state, cx| state.set_items(Vec::new(), window, cx));
                    library.search(false, window, cx);
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &version,
            window,
            |library: &mut Library, _, event, window, cx| {
                if let SelectEvent::Confirm(Some(_)) = event {
                    library.cancel();
                    library.definition = None;
                    library.resolve(window, cx);
                }
            },
        )
        .detach();
        match &target {
            Target::Knowledge(panel) => cx
                .observe(panel, |library: &mut Library, _, cx| library.changed(cx))
                .detach(),
            Target::Speech(panel) => cx
                .observe(panel, |library: &mut Library, _, cx| library.changed(cx))
                .detach(),
            Target::Ocr(panel) => cx
                .observe(panel, |library: &mut Library, _, cx| library.changed(cx))
                .detach(),
        }
        Library {
            target,
            binding,
            query,
            hub,
            version,
            items: Vec::new(),
            repository: None,
            definition: None,
            installing: None,
            query_prefix,
            document_prefix,
            cursor: None,
            searched: None,
            partial: false,
            loading: true,
            generation: 0,
            closed: false,
            read: None,
        }
    });
    let result = library.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let close = library.clone();
        dialog
            .form_title(tr(match library.read(cx).target.kind() {
                Kind::Embedding => "model_library_embedding",
                Kind::Speech => "model_library_speech",
                Kind::Ocr => "model_library_ocr",
            }))
            .w((window.viewport_size().width - px(48.)).min(px(680.)))
            .overlay_closable(false)
            .on_ok(|_, _, _| false)
            .on_close(move |_, _, cx| {
                close.update(cx, |library, _| {
                    library.closed = true;
                    library.cancel();
                });
            })
            .child(library.clone())
    });
    result.update(cx, |_, cx| {
        // The target's button listener still owns its update while this dialog opens.
        cx.defer_in(window, |library, window, cx| {
            if library.read.is_none() {
                library.loading = false;
                library.search(false, window, cx);
                cx.notify();
            }
        });
    });
    result
}

impl Library {
    fn hub(&self, cx: &App) -> Hub {
        self.hub
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or_default()
    }
    fn available(&self, cx: &App) -> bool {
        !self.closed && self.target.available(self.binding.client.target(), cx)
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        if !self.available(cx) && self.read.is_some() {
            self.cancel();
        }
        cx.notify();
    }
    fn download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.available(cx) {
            crate::feedback::info("", tr("knowledge_target_changed").as_ref(), window, cx);
            return;
        }
        if self.target.busy(cx) {
            return;
        }
        if let Some(definition) = self.draft(cx) {
            if definition.validate().is_err() {
                self.failure(window, cx);
                return;
            }
            if !matches!(
                self.target.model_status(&definition, cx),
                ModelStatus::Installed | ModelStatus::Downloading(_) | ModelStatus::Checking
            ) {
                self.target.download(definition, window, cx);
            }
        }
    }
    fn draft(&self, cx: &App) -> Option<Definition> {
        let mut definition = self.definition.clone()?;
        if definition.format.kind() == Kind::Embedding {
            let prefix = |value: SharedString| (!value.is_empty()).then(|| value.to_string());
            definition.query_prefix = prefix(self.query_prefix.read(cx).value());
            definition.document_prefix = prefix(self.document_prefix.read(cx).value());
        }
        Some(definition)
    }
    fn failure(&self, window: &mut Window, cx: &mut Context<Self>) {
        crate::feedback::info("", tr("model_library_failed").as_ref(), window, cx);
    }
}
