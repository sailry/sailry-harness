use super::group::{Group, Row};
use crate::{dictation, preferences, tr};
use dictation::permission::{self, Status};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        button::Button,
        menu::{DropdownMenu, PopupMenuItem},
        *,
    },
    *,
};
use sailry_link::CancellationToken;

#[cfg(test)]
mod tests;
mod view;

#[derive(Clone, Copy, Default, PartialEq)]
enum State {
    #[default]
    Unavailable,
    Checking,
    Missing,
    Downloading(dictation::model::DownloadProgress),
    Ready,
    Failed,
}

pub(super) struct Panel {
    state: State,
    microphones: Vec<(String, String)>,
    cancel: CancellationToken,
    task: Option<Task<()>>,
    manager: Option<std::sync::Arc<dictation::models::Manager>>,
    binding: Option<super::providers::Binding>,
    watch: Option<Task<()>>,
    seen: std::collections::BTreeMap<sailry_protocol::ModelId, dictation::models::State>,
    resolving: std::collections::BTreeSet<sailry_protocol::ModelId>,
    permission: Status,
    permission_stop: CancellationToken,
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.permission_stop.cancel();
    }
}

#[cfg(test)]
enum Update {
    Progress(dictation::model::DownloadProgress),
    Finished(dictation::Result<()>),
}

impl Panel {
    pub(in crate::settings) fn model_status(
        &self,
        definition: &sailry_protocol::models::Definition,
    ) -> super::model_library::ModelStatus {
        self.manager
            .as_ref()
            .and_then(|manager| {
                manager.entries().into_iter().find(|entry| {
                    super::model_library::equivalent(&entry.record.definition, definition)
                })
            })
            .map_or(super::model_library::ModelStatus::Missing, |entry| {
                model_status(entry.state)
            })
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
        self.manager
            .as_ref()
            .and_then(|manager| {
                manager.entries().into_iter().find(|entry| {
                    entry.record.definition.source == *source
                        && entry.record.definition.format.kind() == kind
                })
            })
            .map_or(super::model_library::ModelStatus::Missing, |entry| {
                model_status(entry.state)
            })
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<preferences::Preferences>(|_, cx| cx.notify())
            .detach();
        Self {
            state: State::Unavailable,
            microphones: vec![],
            cancel: CancellationToken::new(),
            task: None,
            manager: None,
            binding: None,
            watch: None,
            seen: Default::default(),
            resolving: Default::default(),
            permission: Status::Unknown,
            permission_stop: CancellationToken::new(),
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if dictation::directory(cx).is_none() {
            return;
        }
        if dictation::directory(cx).is_some() {
            self.permission = permission::status();
            cx.notify();
        }
        if matches!(self.state, State::Checking | State::Downloading(_)) {
            return;
        }
        let selected = preferences::data(cx).dictation.unwrap_or_default().model;
        let Ok(captured) = dictation::capture(cx) else {
            self.state = State::Missing;
            return;
        };
        self.state = State::Checking;
        let job = cx
            .background_executor()
            .spawn(async move { (captured.ready(), dictation::microphones()) });
        self.task = Some(cx.spawn(async move |owner, cx| {
            let (ready, devices) = job.await;
            _ = owner.update(cx, |this, cx| {
                if preferences::data(cx).dictation.unwrap_or_default().model != selected {
                    return;
                }
                if let Some(manager) = &this.manager {
                    manager.observe(selected, ready);
                }
                this.state = if ready { State::Ready } else { State::Missing };
                this.microphones = devices.unwrap_or_default();
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn authorize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if dictation::directory(cx).is_none() {
            return;
        }
        let owner = cx.entity().downgrade();
        crate::permissions::microphone(
            self.permission_stop.clone(),
            Box::new(move |_, _, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.finish_authorization(permission::status(), cx)
                });
            }),
            window,
            cx,
        );
    }

    fn finish_authorization(&mut self, status: Status, cx: &mut Context<Self>) {
        self.permission = status;
        self.refresh(cx);
        cx.notify();
    }

    pub(super) fn bind(
        &mut self,
        binding: Option<super::providers::Binding>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.binding = binding;
        if self.manager.is_some() || dictation::directory(cx).is_none() {
            return;
        }
        match dictation::Service::models(cx) {
            Ok(manager) => {
                let mut changes = manager.subscribe();
                self.seen = manager
                    .entries()
                    .into_iter()
                    .map(|entry| (entry.record.id, entry.state))
                    .collect();
                self.manager = Some(manager);
                self.watch = Some(cx.spawn_in(window, async move |owner, cx| {
                    while changes.changed().await.is_ok() {
                        if owner
                            .update_in(cx, |panel, window, cx| {
                                let selected =
                                    preferences::data(cx).dictation.unwrap_or_default().model;
                                let state =
                                    panel.manager.as_ref().and_then(|manager| match selected {
                                        Some(id) => manager
                                            .entries()
                                            .into_iter()
                                            .find(|entry| entry.record.id == id)
                                            .map(|entry| entry.state),
                                        None => manager.default_state(),
                                    });
                                if let Some(state) = state {
                                    panel.accept_state(state, window, cx);
                                }
                                for entry in panel
                                    .manager
                                    .as_ref()
                                    .map(|manager| manager.entries())
                                    .unwrap_or_default()
                                {
                                    let previous = panel.seen.insert(entry.record.id, entry.state);
                                    if selected != Some(entry.record.id)
                                        && previous != Some(entry.state)
                                    {
                                        if let dictation::models::State::Failed(error) = entry.state
                                        {
                                            crate::feedback::info(
                                                "",
                                                tr(error).as_ref(),
                                                window,
                                                cx,
                                            );
                                        } else if entry.state == dictation::models::State::Installed
                                            && matches!(
                                                previous,
                                                Some(dictation::models::State::Downloading(_))
                                            )
                                        {
                                            window.push_notification(tr("knowledge_installed"), cx);
                                        }
                                    }
                                }
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }));
                self.refresh(cx);
            }
            Err(error) => crate::feedback::info("", tr(error).as_ref(), window, cx),
        }
    }

    fn accept_state(
        &mut self,
        state: dictation::models::State,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match state {
            dictation::models::State::Missing => self.state = State::Missing,
            dictation::models::State::Downloading(progress) => {
                self.state = State::Downloading(progress)
            }
            dictation::models::State::Installed => {
                if matches!(self.state, State::Downloading(_)) {
                    window.push_notification(tr("knowledge_installed"), cx);
                }
                self.state = State::Ready;
            }
            dictation::models::State::Failed(error) => {
                if self.state != State::Failed {
                    crate::feedback::info("", tr(error).as_ref(), window, cx);
                }
                self.state = State::Failed;
            }
        }
        cx.notify();
    }

    pub(super) fn install(
        &mut self,
        id: Option<sailry_protocol::ModelId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(manager) = &self.manager else {
            return;
        };
        let Some(runtime) = cx
            .try_global::<crate::backend::Services>()
            .map(|services| services.runtime.clone())
            .or_else(|| self.binding.as_ref().map(|binding| binding.runtime.clone()))
        else {
            crate::feedback::info("", tr("dictation_unavailable").as_ref(), window, cx);
            return;
        };
        match manager.start(id, None, &runtime) {
            Ok(cancel) => {
                if id == preferences::data(cx).dictation.unwrap_or_default().model {
                    self.cancel = cancel;
                    self.state = State::Downloading(Default::default());
                }
            }
            Err(error) => crate::feedback::info("", tr(error).as_ref(), window, cx),
        }
        cx.notify();
    }

    pub(in crate::settings) fn update_model(
        &mut self,
        id: sailry_protocol::ModelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(binding) = self.binding.clone() else {
            return;
        };
        let Some(definition) = self.definition_for(Some(id)) else {
            return;
        };
        if !self.resolving.insert(id) {
            return;
        }
        let node = binding.client.target();
        let mut source = definition.source;
        source.revision = "HEAD".into();
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(
                    binding
                        .client
                        .prepare(sailry_protocol::Command::ResolveModel {
                            kind: sailry_protocol::models::Kind::Speech,
                            source,
                        }),
                )
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.resolving.remove(&id);
                if !panel.admits(node) {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Ok(sailry_protocol::Output::ModelDefinition(definition))) => {
                        if matches!(
                            panel.model_status(&definition),
                            super::model_library::ModelStatus::Installed
                        ) {
                            crate::feedback::info(
                                "",
                                tr("knowledge_up_to_date").as_ref(),
                                window,
                                cx,
                            );
                        } else {
                            panel.register(definition, window, cx);
                        }
                    }
                    _ => crate::feedback::info("", tr("model_library_failed").as_ref(), window, cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(binding) = self.binding.clone() {
            super::model_library::open(
                super::model_library::Target::Speech(cx.entity()),
                binding,
                window,
                cx,
            );
        } else {
            crate::feedback::info("", tr("knowledge_host_required").as_ref(), window, cx);
        }
    }

    pub(super) fn admits(&self, node: sailry_protocol::NodeId) -> bool {
        self.manager.is_some()
            && self
                .binding
                .as_ref()
                .is_some_and(|binding| binding.client.target() == node)
    }

    pub(super) fn register(
        &mut self,
        definition: sailry_protocol::models::Definition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .manager
            .as_ref()
            .ok_or("dictation_unavailable")
            .and_then(|manager| manager.register(definition));
        match result {
            Ok(id) => self.install(Some(id), window, cx),
            Err(error) => crate::feedback::info("", tr(error).as_ref(), window, cx),
        }
    }

    pub(super) fn select(
        &mut self,
        id: Option<sailry_protocol::ModelId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(manager) = &self.manager else {
            return;
        };
        if id.is_some_and(|id| {
            !manager.entries().iter().any(|entry| {
                entry.record.id == id && entry.state == dictation::models::State::Installed
            })
        }) {
            crate::feedback::info("", tr("dictation_model_missing").as_ref(), window, cx);
            return;
        }
        preferences::update(cx, |data| data.dictation.get_or_insert_default().model = id);
        self.state = State::Missing;
        self.refresh(cx);
    }

    pub(super) fn remove(
        &mut self,
        id: sailry_protocol::ModelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .manager
            .as_ref()
            .ok_or("dictation_unavailable")
            .and_then(|manager| manager.remove(id));
        match result {
            Ok(()) => {
                if preferences::data(cx).dictation.unwrap_or_default().model == Some(id) {
                    self.select(None, window, cx);
                }
                window.push_notification(tr("knowledge_deleted"), cx);
            }
            Err(error) => crate::feedback::info("", tr(error).as_ref(), window, cx),
        }
        cx.notify();
    }

    #[cfg(test)]
    fn accept(&mut self, update: Update, window: &mut Window, cx: &mut Context<Self>) {
        match update {
            Update::Progress(value) => self.state = State::Downloading(value),
            Update::Finished(Ok(())) => self.state = State::Ready,
            Update::Finished(Err("dictation_cancelled")) => self.state = State::Missing,
            Update::Finished(Err(error)) => {
                self.state = State::Failed;
                crate::feedback::info("", tr(error).as_ref(), window, cx);
            }
        }
        cx.notify();
    }
}

fn language_key(language: preferences::Language) -> &'static str {
    match language {
        preferences::Language::Auto => "dictation_language_auto",
        preferences::Language::Chinese => "dictation_language_chinese",
        preferences::Language::English => "dictation_language_english",
    }
}

impl Panel {
    pub(super) fn cancel(&mut self, id: Option<sailry_protocol::ModelId>, cx: &mut Context<Self>) {
        if let Some(manager) = &self.manager {
            manager.cancel(id);
        } else {
            self.cancel.cancel();
        }
        cx.notify();
    }

    pub(super) fn model_rows(
        &self,
        owner: Entity<Self>,
        library: bool,
        cx: &App,
    ) -> Vec<super::entry::Entry> {
        super::model_library::model_rows(
            super::model_library::Target::Speech(owner),
            self.model_entries(cx),
            false,
            library,
            cx,
        )
    }

    pub(in crate::settings) fn definition_for(
        &self,
        id: Option<sailry_protocol::ModelId>,
    ) -> Option<sailry_protocol::models::Definition> {
        self.manager
            .as_ref()?
            .entries()
            .into_iter()
            .find(|entry| Some(entry.record.id) == id)
            .map(|entry| entry.record.definition)
    }

    pub(in crate::settings) fn model_entries(
        &self,
        cx: &App,
    ) -> Vec<super::model_library::ManagedModel> {
        use super::model_library::{ManagedModel, ModelKey, ModelStatus};
        let selected = preferences::data(cx).dictation.unwrap_or_default().model;
        let current = match self.state {
            State::Missing => ModelStatus::Missing,
            State::Downloading(progress) => ModelStatus::Downloading(progress),
            State::Ready => ModelStatus::Installed,
            State::Checking => ModelStatus::Checking,
            State::Unavailable => ModelStatus::Unavailable,
            State::Failed => ModelStatus::Failed,
        };
        let default = if selected.is_none() {
            current
        } else {
            self.manager
                .as_ref()
                .and_then(|manager| manager.default_state())
                .map_or(ModelStatus::Missing, model_status)
        };
        let mut models = vec![ManagedModel {
            key: ModelKey::Speech(None),
            name: tr("dictation_model_name"),
            detail: String::new(),
            status: default,
            selected: selected.is_none(),
            selectable: true,
            removable: false,
            cancelling: self.manager.as_ref().map_or_else(
                || self.cancel.is_cancelled(),
                |manager| manager.cancelling(None),
            ),
        }];
        for entry in self
            .manager
            .as_ref()
            .map(|manager| manager.entries())
            .unwrap_or_default()
        {
            let id = entry.record.id;
            let definition = entry.record.definition;
            models.push(ManagedModel {
                key: ModelKey::Speech(Some(id)),
                name: definition.name.into(),
                detail: definition.source.repository,
                status: if self.resolving.contains(&id) {
                    ModelStatus::Checking
                } else if selected == Some(id) {
                    current
                } else {
                    model_status(entry.state)
                },
                selected: selected == Some(id),
                selectable: entry.state == dictation::models::State::Installed,
                removable: true,
                cancelling: self
                    .manager
                    .as_ref()
                    .is_some_and(|manager| manager.cancelling(Some(id))),
            });
        }
        models
    }
}

fn model_status(state: dictation::models::State) -> super::model_library::ModelStatus {
    use super::model_library::ModelStatus;
    match state {
        dictation::models::State::Missing => ModelStatus::Missing,
        dictation::models::State::Downloading(progress) => ModelStatus::Downloading(progress),
        dictation::models::State::Installed => ModelStatus::Installed,
        dictation::models::State::Failed(_) => ModelStatus::Failed,
    }
}
