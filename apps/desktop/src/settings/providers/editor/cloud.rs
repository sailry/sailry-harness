use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use sailry_protocol::conversation::cloud::Options;

pub(super) struct Fields {
    pub api_version: Entity<InputState>,
    pub region: Entity<InputState>,
    pub project: Entity<InputState>,
    pub location: Entity<InputState>,
}

impl Fields {
    pub fn new(options: Option<&Options>, window: &mut Window, cx: &mut Context<Editor>) -> Self {
        let (mut version, mut region, mut project, mut location) =
            ("2024-10-21", "us-east-1", "", "global");
        match options {
            Some(Options::AzureOpenAi { api_version }) => version = api_version,
            Some(Options::Bedrock { region: value }) => region = value,
            Some(Options::Vertex {
                project: value,
                location: area,
            }) => {
                project = value;
                location = area;
            }
            None => {}
        }
        Self {
            api_version: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(version)
                    .placeholder(tr("form_api_version_hint"))
            }),
            region: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(region)
                    .placeholder(tr("form_region_hint"))
            }),
            project: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(project)
                    .placeholder(tr("form_cloud_project_hint"))
            }),
            location: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(location)
                    .placeholder(tr("form_location_hint"))
            }),
        }
    }

    pub fn options(&self, preset: Preset, cx: &App) -> Result<Option<Options>, &'static str> {
        let read = |input: &Entity<InputState>| {
            let value = input.read(cx).value().trim().to_owned();
            if value.is_empty() {
                Err("provider_cloud_required")
            } else {
                Ok(value)
            }
        };
        Ok(match preset {
            Preset::AzureOpenAi => Some(Options::AzureOpenAi {
                api_version: read(&self.api_version)?,
            }),
            Preset::Bedrock | Preset::BedrockIam => Some(Options::Bedrock {
                region: read(&self.region)?,
            }),
            Preset::Vertex | Preset::VertexAdc => Some(Options::Vertex {
                project: read(&self.project)?,
                location: read(&self.location)?,
            }),
            _ => None,
        })
    }

    pub fn endpoint(&self, preset: Preset, cx: &App) -> Result<String, &'static str> {
        endpoint(self.options(preset, cx)?.as_ref()).ok_or("provider_configuration_invalid")
    }
}

pub(super) fn endpoint(options: Option<&Options>) -> Option<String> {
    match options? {
        Options::Bedrock { region } => {
            Some(format!("https://bedrock-runtime.{region}.amazonaws.com"))
        }
        Options::Vertex { location, .. } if location == "global" => {
            Some("https://aiplatform.googleapis.com".into())
        }
        Options::Vertex { location, .. } => {
            Some(format!("https://{location}-aiplatform.googleapis.com"))
        }
        _ => None,
    }
}

impl Editor {
    pub(super) fn cloud_form(&self, mut form: Form, cx: &Context<Self>) -> Form {
        let fields: &[(&str, &Entity<InputState>)] = match self.preset {
            Preset::AzureOpenAi => &[("provider_api_version", &self.cloud.api_version)],
            Preset::Bedrock | Preset::BedrockIam => &[("provider_region", &self.cloud.region)],
            Preset::Vertex | Preset::VertexAdc => &[
                ("provider_cloud_project", &self.cloud.project),
                ("provider_region", &self.cloud.location),
            ],
            _ => &[],
        };
        for (key, input) in fields {
            form = form.child(
                Field::new()
                    .label(tr(key))
                    .child(Input::new(input).disabled(self.pending).aria_label(tr(key))),
            );
        }
        let options = match self.preset {
            Preset::Bedrock | Preset::BedrockIam => Some([Preset::Bedrock, Preset::BedrockIam]),
            Preset::Vertex | Preset::VertexAdc => Some([Preset::Vertex, Preset::VertexAdc]),
            _ => None,
        };
        if let Some(options) = options {
            form = form.child(
                Field::new().label(tr("provider_cloud_endpoint")).child(
                    Input::new(&self.endpoint)
                        .disabled(self.pending)
                        .aria_label(tr("provider_cloud_endpoint")),
                ),
            );
            let owner = cx.entity();
            let preset = self.preset;
            let control = h_flex().w_full().child(
                Button::new("provider-authentication")
                    .debug_selector(|| "provider-authentication".into())
                    .w_full()
                    .label(tr(identity_key(preset)))
                    .dropdown_caret(true)
                    .disabled(self.editing.is_some() || self.pending)
                    .dropdown_menu(move |menu, _, _| {
                        options.into_iter().fold(menu, |menu, option| {
                            let owner = owner.clone();
                            menu.item(
                                PopupMenuItem::new(tr(identity_key(option)))
                                    .checked(option == preset)
                                    .on_click(move |_, window, cx| {
                                        owner.update(cx, |editor, cx| {
                                            if editor.preset == option {
                                                return;
                                            }
                                            editor.preset = option;
                                            editor.configured = false;
                                            editor.probe = None;
                                            editor.credential.update(cx, |input, cx| {
                                                input.set_value("", window, cx)
                                            });
                                            cx.notify();
                                        })
                                    }),
                            )
                        })
                    }),
            );
            form = form.child(
                Field::new()
                    .label(tr("provider_authentication"))
                    .child(control),
            );
        }
        form
    }
}

fn identity_key(preset: Preset) -> &'static str {
    match preset {
        Preset::BedrockIam => "provider_aws_identity",
        Preset::VertexAdc => "provider_google_identity",
        _ => "provider_api_key",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn draw(visual: &mut VisualTestContext) {
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }

    #[gpui::test]
    fn switch_preserves_models(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let mut editor = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let owner = cx.new(|cx| Workspace::new(window, cx));
            let view = cx.new(|cx| Editor::new(owner, None, window, cx));
            editor = Some(view.clone());
            Root::new(view, window, cx)
        });
        let editor = editor.unwrap();
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            for width in [760., 1280.] {
                let handle = visual.update(|window, cx| {
                    crate::theme::select(Some(mode), window, cx);
                    window.window_handle()
                });
                visual.simulate_window_resize(handle, size(px(width), px(820.)));
                for (keyed, host) in [
                    (Preset::Bedrock, Preset::BedrockIam),
                    (Preset::Vertex, Preset::VertexAdc),
                ] {
                    visual.update(|window, cx| {
                        editor.update(cx, |editor, cx| {
                            editor.select_preset(keyed, window, cx);
                            editor.add_model(window, cx);
                            editor.models[0].id.update(cx, |input, cx| {
                                input.set_value("fixture-model", window, cx)
                            });
                            editor
                                .credential
                                .update(cx, |input, cx| input.set_value("fixture-key", window, cx));
                            editor.cloud.project.update(cx, |input, cx| {
                                input.set_value("fixture-project", window, cx)
                            });
                            editor.scroll.set_offset(point(px(0.), px(-210.)));
                        })
                    });
                    draw(visual);
                    let bounds = visual.debug_bounds("provider-authentication").unwrap();
                    visual.simulate_click(bounds.center(), Modifiers::default());
                    draw(visual);
                    visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
                    visual.simulate_keystrokes("down down enter");
                    draw(visual);
                    editor.read_with(visual, |editor, cx| {
                        assert_eq!(editor.preset, host);
                        assert_eq!(editor.models.len(), 1);
                        assert_eq!(editor.models[0].value(cx).unwrap().id, "fixture-model");
                        assert!(editor.credential.read(cx).value().is_empty());
                        assert!(!editor.configured);
                        assert!(editor.cloud.options(host, cx).unwrap().is_some());
                    });
                }
            }
        }
    }
}
