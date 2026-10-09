//! Public model catalog composition shared by the two model-library entry points.
mod managed;
mod requests;
#[cfg(test)]
mod tests;
mod view;
pub(super) use managed::rows as model_rows;

use super::{dictation, knowledge, providers::Binding};
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
}

impl Target {
    fn kind(&self) -> Kind {
        match self {
            Self::Knowledge(_) => Kind::Embedding,
            Self::Speech(_) => Kind::Speech,
        }
    }
    fn available(&self, node: NodeId, cx: &App) -> bool {
        match self {
            Self::Knowledge(panel) => panel.read(cx).admits(node),
            Self::Speech(panel) => panel.read(cx).admits(node),
        }
    }
    fn busy(&self, cx: &App) -> bool {
        match self {
            Self::Knowledge(panel) => panel.read(cx).locked(),
            Self::Speech(_) => false,
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
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ModelKey {
    Embedding(Model),
    Speech(Option<ModelId>),
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

impl Target {
    fn rows(&self, library: bool, cx: &App) -> Vec<crate::settings::entry::Entry> {
        match self {
            Self::Knowledge(panel) => panel.read(cx).model_rows(panel.clone(), library, cx),
            Self::Speech(panel) => panel.read(cx).model_rows(panel.clone(), library, cx),
        }
    }
    fn install(&self, key: ModelKey, window: &mut Window, cx: &mut App) {
        match (self, key) {
            (Self::Knowledge(panel), ModelKey::Embedding(model)) => panel
                .update(cx, |panel, cx| {
                    panel.submit(Command::InstallKnowledgeModel { model }, window, cx)
                }),
            (Self::Speech(panel), ModelKey::Speech(id)) => {
                panel.update(cx, |panel, cx| panel.install(id, window, cx));
            }
            _ => unreachable!("model row belongs to its target"),
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
            _ => unreachable!("recommended speech model is retained"),
        }
    }
}

#[derive(Clone)]
struct HubChoice(Hub);
impl SearchableListItem for HubChoice {
    type Value = Hub;
    fn title(&self) -> SharedString {
        tr(match self.0 {
            Hub::HuggingFace => "model_library_huggingface",
            Hub::ModelScope => "model_library_modelscope",
        })
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
    cursor: Option<String>,
    searched: Option<(Hub, String)>,
    partial: bool,
    loading: bool,
    generation: u64,
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
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let version = cx.new(|cx| SelectState::new(Vec::<VersionChoice>::new(), None, window, cx));
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
                    library.generation += 1;
                    library.loading = false;
                    library.items.clear();
                    library.repository = None;
                    library.definition = None;
                    library.cursor = None;
                    library.searched = None;
                    library.partial = false;
                    library
                        .version
                        .update(cx, |state, cx| state.set_items(Vec::new(), window, cx));
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
                    library.generation += 1;
                    library.loading = false;
                    library.definition = None;
                    library.resolve(window, cx);
                }
            },
        )
        .detach();
        match &target {
            Target::Knowledge(panel) => cx.observe(panel, |_, _, cx| cx.notify()).detach(),
            Target::Speech(panel) => cx.observe(panel, |_, _, cx| cx.notify()).detach(),
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
            cursor: None,
            searched: None,
            partial: false,
            loading: false,
            generation: 0,
        }
    });
    let result = library.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let close = library.clone();
        dialog
            .form_title(tr(match library.read(cx).target.kind() {
                Kind::Embedding => "model_library_embedding",
                Kind::Speech => "model_library_speech",
            }))
            .w((window.viewport_size().width - px(48.)).min(px(680.)))
            .h((window.viewport_size().height - px(96.)).min(px(640.)))
            .overlay_closable(false)
            .on_ok(|_, _, _| false)
            .on_close(move |_, _, cx| {
                close.update(cx, |library, _| library.generation += 1);
            })
            .child(library.clone())
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
        self.target.available(self.binding.client.target(), cx)
    }
    fn download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.available(cx) {
            crate::feedback::info("", tr("knowledge_target_changed").as_ref(), window, cx);
            return;
        }
        if self.target.busy(cx) {
            return;
        }
        if let Some(definition) = &self.definition {
            self.target.download(definition.clone(), window, cx);
        }
    }
    fn failure(&self, window: &mut Window, cx: &mut Context<Self>) {
        crate::feedback::info("", tr("model_library_failed").as_ref(), window, cx);
    }
}
