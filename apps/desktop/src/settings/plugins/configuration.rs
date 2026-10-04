//! A configuration draft belongs to the Node and package captured when it opens.
use super::*;
use crate::settings::providers::Binding;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Output, Request,
    plugin::{Info, Summary, settings::State},
};
mod fields;
mod submit;
mod tabs;
#[cfg(test)]
mod tests;
mod view;
use fields::Setting;

pub(in crate::settings) struct Editor {
    embedded: bool,
    tab: usize,
    panel: bool,
    binding: Binding,
    expected: Summary,
    info: Option<Info>,
    fields: Vec<Entity<Setting>>,
    models: sailry_protocol::plugin::models::Catalog,
    observers: Vec<Subscription>,
    loading: bool,
    error: Option<&'static str>,
    detail: Option<String>,
    details: bool,
    changed: bool,
    request: Option<Request>,
    pending: bool,
    closed: bool,
    stop: CancellationToken,
    task: Option<Task<()>>,
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(in crate::settings) fn open(
    owner: Entity<Workspace>,
    expected: Summary,
    window: &mut Window,
    cx: &mut App,
) {
    if !owner
        .read(cx)
        .plugin_catalog
        .metadata
        .as_ref()
        .is_some_and(|metadata| {
            metadata
                .read(cx)
                .entries
                .get(&expected.name)
                .is_some_and(|info| {
                    info.settings
                        .as_ref()
                        .is_some_and(|schema| !schema.properties.is_empty())
                })
        })
    {
        return;
    }
    let Some(live) = &owner.read(cx).provider_link else {
        return;
    };
    if !live.connected {
        return;
    }
    mount(live.binding.clone(), expected, window, cx);
}

pub(in crate::settings) fn mount(
    binding: Binding,
    expected: Summary,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let editor = page(binding, expected, window, cx);
    editor.update(cx, |editor, _| editor.embedded = false);
    view::open(editor.clone(), window, cx);
    editor
}

pub(in crate::settings) fn panel(
    binding: Binding,
    expected: Summary,
    window: &mut Window,
    cx: &mut App,
) {
    let editor = mount(binding, expected, window, cx);
    editor.update(cx, |editor, _| editor.panel = true);
}

pub(in crate::settings) fn page(
    binding: Binding,
    expected: Summary,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let editor = cx.new(|_| Editor {
        embedded: true,
        tab: 0,
        panel: false,
        binding,
        expected,
        info: None,
        fields: Vec::new(),
        models: Default::default(),
        observers: Vec::new(),
        loading: false,
        error: None,
        detail: None,
        details: false,
        changed: false,
        request: None,
        pending: false,
        closed: false,
        stop: CancellationToken::new(),
        task: None,
    });
    editor.update(cx, |_, cx| {
        crate::feedback::observe(window, cx, |view, _| view.error.into_iter().collect());
    });
    editor.update(cx, |editor, cx| editor.load(false, window, cx));
    editor
}

impl Editor {
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.closed = true;
        self.stop.cancel();
        self.request = None;
        for field in &self.fields {
            field.update(cx, |field, cx| field.clear_secret(window, cx));
        }
    }

    fn load(&mut self, reload: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.pending || self.request.is_some() {
            return;
        }
        self.stop.cancel();
        self.stop = CancellationToken::new();
        self.loading = true;
        self.error = None;
        self.detail = None;
        self.lock(true, cx);
        let binding = self.binding.clone();
        let expected = self.expected.clone();
        let stop = self.stop.clone();
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = async {
                    let Output::Plugin(info) = binding.client.execute(binding.client.prepare(Command::ReadPlugin { name: expected.name.clone() })).await? else {
                        return Err(sailry_protocol::Fault::new(ErrorCode::Internal, "plugin information expected"));
                    };
                    if !reload && info.summary.reference() != expected.reference() {
                        return Err(sailry_protocol::Fault::new(ErrorCode::RevisionConflict, "plugin configuration changed"));
                    }
                    if !info.settings.as_ref().is_some_and(|schema| !schema.properties.is_empty()) {
                        return Err(sailry_protocol::Fault::new(ErrorCode::NotConfigured, "plugin has no configurable settings"));
                    }
                    let Output::PluginSettings(state) = binding.client.execute(binding.client.prepare(Command::ReadPluginSettings { package: info.summary.reference() })).await? else {
                        return Err(sailry_protocol::Fault::new(ErrorCode::Internal, "plugin settings expected"));
                    };
                    let models = if info.settings.as_ref().is_some_and(|schema| schema.properties.values().any(|field| field.model)) {
                        let Output::PluginModels(models) = binding.client.execute(binding.client.prepare(Command::ListPluginModels)).await? else {
                            return Err(sailry_protocol::Fault::new(ErrorCode::Internal, "plugin models expected"));
                        };
                        models
                    } else { Default::default() };
                    Ok((info, state, models))
                } => Some(result),
            }
        });
        let stop = self.stop.clone();
        self.task = Some(cx.spawn_in(window, async move |editor, cx| {
            let result = job.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                if editor.closed || stop.is_cancelled() {
                    return;
                }
                editor.loading = false;
                editor.lock(false, cx);
                match result {
                    Ok(Some(Ok((info, state, models)))) => {
                        editor.accept(info, state, &models, window, cx)
                    }
                    Ok(Some(Err(error))) => {
                        if error.code == ErrorCode::NotConfigured {
                            for field in std::mem::take(&mut editor.fields) {
                                field.update(cx, |field, cx| field.clear_secret(window, cx));
                            }
                            editor.info = None;
                        }
                        editor.failure(error);
                    }
                    _ => editor.error = Some("plugins_settings_read_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn accept(
        &mut self,
        info: Info,
        state: State,
        models: &sailry_protocol::plugin::models::Catalog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(schema) = &info.settings else {
            self.error = Some("plugins_settings_none");
            return;
        };
        let labels = schema
            .locales
            .get::<str>(&rust_i18n::locale())
            .cloned()
            .unwrap_or_default();
        self.changed = self
            .info
            .as_ref()
            .is_some_and(|previous| previous.settings != info.settings);
        let mut previous = std::mem::take(&mut self.fields);
        for (name, spec) in &schema.properties {
            let mut spec = spec.clone();
            if let Some(title) = labels.get(name) {
                spec.title = Some(title.clone());
            }
            let existing = previous
                .iter()
                .position(|field| field.read(cx).dirty && field.read(cx).compatible(name, &spec));
            let field = if let Some(index) = existing {
                let field = previous.remove(index);
                field.update(cx, |field, cx| {
                    field.refresh(
                        spec.clone(),
                        schema.required.contains(name),
                        fields::Data {
                            state: &state,
                            models,
                            labels: &labels,
                        },
                        window,
                        cx,
                    )
                });
                field
            } else {
                cx.new(|cx| {
                    Setting::new(
                        name.clone(),
                        spec.clone(),
                        schema.required.contains(name),
                        fields::Data {
                            state: &state,
                            models,
                            labels: &labels,
                        },
                        window,
                        cx,
                    )
                })
            };
            field.update(cx, |field, _| field.panel = self.panel);
            self.fields.push(field);
        }
        for field in previous {
            field.update(cx, |field, cx| field.clear_secret(window, cx));
        }
        self.models = models.clone();
        self.observers = self
            .fields
            .to_vec()
            .iter()
            .map(|field| {
                cx.observe_in(field, window, |editor, field, window, cx| {
                    if field.read(cx).model() {
                        editor.sync_efforts(window, cx);
                    }
                    cx.notify();
                })
            })
            .collect();
        self.sync_efforts(window, cx);
        self.expected = info.summary.clone();
        self.info = Some(info);
    }

    fn sync_efforts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for field in &self.fields {
            let Some(target) = field.read(cx).linked().map(str::to_owned) else {
                continue;
            };
            let selected = self
                .fields
                .iter()
                .find(|field| field.read(cx).name == target)
                .and_then(|field| match field.read(cx).value(cx).ok()? {
                    fields::Value::Public(Some(serde_json::Value::String(value))) => Some(value),
                    _ => None,
                });
            field.update(cx, |field, cx| {
                field.sync_effort(selected.as_deref(), &self.models, window, cx)
            });
        }
    }

    fn lock(&self, locked: bool, cx: &mut App) {
        for field in &self.fields {
            field.update(cx, |field, cx| {
                field.locked = locked;
                cx.notify();
            });
        }
    }

    fn failure(&mut self, error: sailry_protocol::Fault) {
        self.error = Some(match error.code {
            ErrorCode::RevisionConflict => "plugins_settings_conflict",
            ErrorCode::NotConfigured
                if self
                    .info
                    .as_ref()
                    .and_then(|info| info.settings.as_ref())
                    .is_some_and(|schema| schema.properties.values().any(|field| field.model)) =>
            {
                "plugins_settings_model_unavailable"
            }
            ErrorCode::NotConfigured => "plugins_settings_none",
            ErrorCode::InvalidRequest => "plugins_settings_invalid",
            _ => live::error_key(&error),
        });
        self.detail = Some(error.message);
    }
}
