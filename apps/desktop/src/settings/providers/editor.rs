use super::{Preset, Workspace, data::Channel, model::Draft};
use crate::theme::DialogStyle as _;
use crate::tr;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{
    alert::Alert,
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    form::{Field, Form},
    input::{Input, InputState},
    notification::Notification,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::HashSet;
mod catalog;
mod cloud;
mod credential;
mod discovery;
mod kind;
#[cfg(test)]
use super::discovery_server;
mod live;

pub(super) struct Editor {
    pub(super) focus: FocusHandle,
    owner: Entity<Workspace>,
    editing: Option<usize>,
    pub preset: Preset,
    name: Entity<InputState>,
    endpoint: Entity<InputState>,
    credential: Entity<InputState>,
    original_key: Option<sailry_protocol::Secret>,
    key_ready: bool,
    cloud: cloud::Fields,
    configured: bool,
    pub models: Vec<Draft>,
    pub default_model: usize,
    pub error: Option<&'static str>,
    next_model: usize,
    binding: Option<super::live::Binding>,
    provider: Option<sailry_protocol::conversation::Provider>,
    provider_id: sailry_protocol::ProviderId,
    pub(super) pending: bool,
    closed: bool,
    request: Option<sailry_protocol::Request>,
    task: Option<Task<()>>,
    probe: Option<discovery::Probe>,
    scroll: ScrollHandle,
}

pub(super) fn open(
    owner: Entity<Workspace>,
    editing: Option<usize>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    let editor = cx.new(|cx| Editor::new(owner, editing, window, cx));
    let entity = editor.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let width = (window.viewport_size().width - px(48.)).min(px(680.));
        let save = editor.clone();
        let close = editor.clone();
        let cancel = editor.clone();
        let real = editor.read(cx).binding.is_some();
        dialog
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .form_title(
                div()
                    .debug_selector(|| "provider-dialog-title".into())
                    .child(tr(if editing.is_some() {
                        "provider_edit_title"
                    } else {
                        "provider_add_title"
                    })),
            )
            .w(width)
            .max_h(window.viewport_size().height * 0.8)
            .margin_top(window.viewport_size().height * 0.1)
            .on_close(move |_, window, cx| close.update(cx, |editor, cx| editor.close(window, cx)))
            .content({
                let editor = editor.clone();
                move |content, _, _| content.min_h_0().pt_4().child(editor.clone())
            })
            .footer(
                DialogFooter::new()
                    .w_full()
                    .child(
                        Button::new("provider-cancel")
                            .debug_selector(|| "provider-cancel".into())
                            .label(tr("settings_cancel"))
                            .on_click(move |_, window, cx| {
                                cancel.update(cx, |editor, cx| editor.close(window, cx));
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("provider-save")
                            .primary()
                            .debug_selector(|| "provider-save".into())
                            .label(tr(if real {
                                "provider_save"
                            } else {
                                "settings_save_preview"
                            }))
                            .disabled(editor.read(cx).pending)
                            .on_click(move |_, window, cx| {
                                save.update(cx, |editor, cx| editor.save(window, cx))
                            }),
                    ),
            )
    });
    entity
}

impl Editor {
    pub fn new(
        owner: Entity<Workspace>,
        editing: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = &owner.read(cx).providers;
        let binding = owner
            .read(cx)
            .provider_link
            .as_ref()
            .map(|live| live.binding.clone());
        let provider = owner
            .read(cx)
            .provider_link
            .as_ref()
            .and_then(|live| editing.and_then(|id| live.providers.get(&id)))
            .cloned();
        let provider_id = provider
            .as_ref()
            .map_or_else(sailry_protocol::ProviderId::new, |provider| provider.id);
        let initial = editing
            .and_then(|id| store.channels.iter().find(|c| c.id == id))
            .cloned();
        let preset = initial.as_ref().map(|c| c.preset).unwrap_or(Preset::OpenAi);
        let name = initial.as_ref().map(|c| c.name.clone()).unwrap_or_default();
        let endpoint = initial
            .as_ref()
            // The saved endpoint is resolved for execution. A matching regional
            // default remains automatic in the editor; custom overrides stay fixed.
            .filter(|channel| {
                cloud::endpoint(channel.options.as_ref()).as_deref()
                    != Some(channel.endpoint.as_str())
            })
            .map(|c| c.endpoint.clone())
            .unwrap_or_default();
        let configured = initial.as_ref().is_some_and(|c| c.credential_configured);
        let default_model = initial
            .as_ref()
            .and_then(|c| c.models.iter().position(|m| m.id == c.default_model))
            .unwrap_or(0);
        let models: Vec<_> = initial
            .as_ref()
            .map(|c| {
                c.models
                    .iter()
                    .enumerate()
                    .map(|(id, model)| Draft::new(id, model.clone(), window, cx))
                    .collect()
            })
            .unwrap_or_default();
        let next_model = models.len();
        let cloud = cloud::Fields::new(
            initial
                .as_ref()
                .and_then(|channel| channel.options.as_ref()),
            window,
            cx,
        );
        let mut editor = Self {
            focus: cx.focus_handle(),
            cloud,
            owner,
            editing,
            preset,
            configured,
            original_key: None,
            key_ready: true,
            default_model,
            models,
            next_model,
            binding,
            provider,
            provider_id,
            pending: false,
            closed: false,
            request: None,
            task: None,
            probe: None,
            scroll: ScrollHandle::new(),
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(name)
                    .placeholder(tr("provider_name_hint"))
            }),
            endpoint: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(endpoint)
                    .placeholder(tr(preset.endpoint_hint()))
            }),
            credential: cx
                .new(|cx| InputState::new(window, cx).placeholder(tr("provider_key_hint"))),
            error: None,
        };
        editor.load_key(window, cx);
        editor
    }

    fn select_preset(&mut self, preset: Preset, window: &mut Window, cx: &mut Context<Self>) {
        if self.preset == preset {
            return;
        }
        let same_kind = self.preset.kind() == preset.kind();
        self.preset = preset;
        self.probe = None;
        self.models.clear();
        self.default_model = 0;
        self.error = None;
        if same_kind {
            cx.notify();
            return;
        }
        self.configured = false;
        self.original_key = None;
        self.key_ready = true;
        self.endpoint.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_placeholder(tr(preset.endpoint_hint()), window, cx);
        });
        self.credential.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        cx.notify();
    }

    pub fn add_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut draft = Draft::discovered(
            self.next_model,
            sailry_protocol::conversation::discovery::Model {
                id: String::new(),
                context: None,
                output: None,
                capabilities: None,
            },
            None,
            self.preset,
            window,
            cx,
        );
        draft.expanded = true;
        self.next_model += 1;
        self.models.push(draft);
        cx.notify();
    }

    pub fn discover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.binding.is_some() {
            self.discover_live(window, cx);
            return;
        }
        // Explicit preview fixtures only; never discover against an endpoint.
        let default_id = self
            .models
            .get(self.default_model)
            .map(|model| model.id.read(cx).value().to_string());
        self.models
            .retain(|model| !model.id.read(cx).value().trim().is_empty());
        for id in ["preview-text-1", "preview-vision-2"] {
            if !self
                .models
                .iter()
                .any(|model| model.id.read(cx).value().trim() == id)
            {
                let mut model = super::data::Model::example(id);
                model
                    .efforts
                    .retain(|effort| effort.validate(self.preset.api(), model.output).is_ok());
                if model.efforts.is_empty() {
                    model.efforts.push(sailry_protocol::Effort::Default);
                }
                if !model.efforts.contains(&model.default_effort) {
                    model.default_effort = model
                        .efforts
                        .first()
                        .copied()
                        .unwrap_or(sailry_protocol::Effort::Default);
                }
                self.models
                    .push(Draft::new(self.next_model, model, window, cx));
                self.next_model += 1;
            }
        }
        self.default_model = default_id
            .and_then(|id| {
                self.models
                    .iter()
                    .position(|model| model.id.read(cx).value() == id)
            })
            .unwrap_or(0);
        self.error = None;
        cx.notify();
    }

    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.binding.is_some() {
            self.save_live(window, cx);
            return;
        }
        let result = self.channel(cx);
        match result {
            Ok(channel) => {
                self.owner.update(cx, |owner, cx| {
                    owner.providers.category = channel.preset.category();
                    owner.providers.save(self.editing, channel);
                    cx.notify();
                });
                self.close(window, cx);
                window.close_dialog(cx);
            }
            Err(error) => {
                self.report_error(error, window, cx);
                cx.notify();
            }
        }
    }

    pub(super) fn report_error(
        &mut self,
        key: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.error = Some(key);
        crate::feedback::toast(
            window,
            tr(key),
            Notification::error(tr(key)).id::<Self>(),
            cx,
        );
    }

    pub fn channel(&self, cx: &App) -> Result<Channel, &'static str> {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return Err("provider_name_required");
        }
        let endpoint = self.endpoint.read(cx).value().trim().to_owned();
        if self.preset.custom_endpoint()
            && !(endpoint.starts_with("https://")
                || (self.binding.is_some() && endpoint.starts_with("http://")))
        {
            return Err("provider_endpoint_required");
        }
        let configured = !self.credential.read(cx).value().trim().is_empty()
            || (self.configured && (self.binding.is_none() || !self.preset.key_auth()));
        if self.preset.key_auth()
            && !configured
            && !(self.binding.is_some() && self.preset.custom_endpoint() && !self.preset.cloud())
        {
            return Err(if self.binding.is_some() {
                "provider_key_required"
            } else {
                "provider_preview_key_required"
            });
        }
        if self.models.is_empty() && !(self.binding.is_some() && self.preset.oauth()) {
            return Err("provider_model_required");
        }
        let mut models = self
            .models
            .iter()
            .map(|model| {
                let model = model.value(cx)?;
                super::live::model(&model)
                    .validate_reasoning(self.preset.api())
                    .map_err(|_| "provider_effort_invalid")?;
                Ok(model)
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        for model in &mut models {
            if !self.preset.web_search() {
                model.web_search = false;
            }
        }
        let mut ids = HashSet::new();
        if models.iter().any(|model| !ids.insert(&model.id)) {
            return Err("provider_model_duplicate");
        }
        Ok(Channel {
            options: self.cloud.options(self.preset, cx)?,
            id: self.editing.unwrap_or(0),
            name,
            preset: self.preset,
            endpoint,
            credential_configured: configured,
            enabled: true,
            connected: false,
            default_model: models
                .get(self.default_model)
                .or_else(|| models.first())
                .map_or_else(String::new, |model| model.id.clone()),
            models,
        })
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preset = self.preset;
        let real = self.binding.is_some();
        let mut form = Form::vertical()
            .child(
                Field::new().label(tr("settings_name")).child(
                    Input::new(&self.name)
                        .disabled(self.pending)
                        .aria_label(tr("settings_name")),
                ),
            )
            .child(
                Field::new()
                    .label(tr("provider_type"))
                    .child(self.kind_control(cx)),
            );
        if preset.custom_endpoint() {
            form = form.child(
                Field::new().label(tr("provider_endpoint")).child(
                    Input::new(&self.endpoint)
                        .disabled(self.pending)
                        .aria_label(tr("provider_endpoint")),
                ),
            );
        }
        form = self.cloud_form(form, cx);
        if preset.key_auth() {
            form = form.child(
                Field::new().label(tr("provider_api_key")).child(
                    div()
                        .w_full()
                        .debug_selector(|| "provider-api-key".into())
                        .child(
                            Input::new(&self.credential)
                                .disabled(self.pending)
                                .aria_label(tr("provider_api_key")),
                        ),
                ),
            );
        }
        let models = (0..self.models.len())
            .map(|index| self.render_model(index, cx))
            .collect::<Vec<_>>();
        v_flex()
            .id("provider-editor-scroll")
            .track_focus(&self.focus)
            .debug_selector(|| "provider-editor".into())
            .w_full()
            .min_h_0()
            .text_sm()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .gap_4()
            .when(!real, |body| {
                body.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("provider_editor_preview")),
                )
            })
            .child(form)
            .when(
                preset.authentication() == sailry_protocol::Authentication::Host,
                |body| {
                    body.child(
                        Alert::info("cloud-identity-note", tr("provider_host_identity_note"))
                            .small(),
                    )
                },
            )
            .when(preset.oauth() && (!real || !self.configured), |this| {
                this.child(
                    div()
                        .id("provider-oauth-note")
                        .debug_selector(|| "provider-oauth-note".into())
                        .child(Alert::info("oauth-note", tr("provider_oauth_note")).small()),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(div().flex_1().child(tr("provider_models")))
                    .child(
                        Button::new("provider-discover")
                            .loading(self.probe.is_some())
                            .disabled(
                                self.pending
                                    || self.probe.is_some()
                                    || (real && preset.oauth() && !self.configured),
                            )
                            .debug_selector(|| "provider-discover".into())
                            .label(tr(if self.probe.is_some() {
                                "provider_querying"
                            } else {
                                if preset.cloud() {
                                    "provider_metadata_fill"
                                } else {
                                    "provider_discover"
                                }
                            }))
                            .on_click(
                                cx.listener(|editor, _, window, cx| editor.discover(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("provider-add-model")
                            .disabled(self.pending)
                            .primary()
                            .debug_selector(|| "provider-add-model".into())
                            .label(tr("settings_add"))
                            .on_click(
                                cx.listener(|editor, _, window, cx| editor.add_model(window, cx)),
                            ),
                    ),
            )
            .when(self.models.is_empty(), |this| {
                this.child(
                    super::Group::new("provider_models")
                        .heading(false)
                        .empty(IconName::Bot, "provider_models_empty"),
                )
            })
            .children(models)
            .vertical_scrollbar(&self.scroll)
    }
}
