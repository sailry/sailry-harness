use super::*;
use sailry_protocol::conversation::cloud::Options;

pub(super) struct Fields {
    pub api_version: Entity<InputState>,
}

impl Fields {
    pub fn new(options: Option<&Options>, window: &mut Window, cx: &mut Context<Editor>) -> Self {
        let version = match options {
            Some(Options::AzureOpenAi { api_version }) => api_version.as_str(),
            None => "2024-10-21",
        };
        Self {
            api_version: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(version)
                    .placeholder(tr("form_api_version_hint"))
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
            _ => None,
        })
    }
}

impl Editor {
    pub(super) fn cloud_form(&self, mut form: Form) -> Form {
        let fields: &[(&str, &Entity<InputState>)] = match self.preset {
            Preset::AzureOpenAi => &[("provider_api_version", &self.cloud.api_version)],
            _ => &[],
        };
        for (key, input) in fields {
            form = form.child(
                Field::new()
                    .label(tr(key))
                    .child(Input::new(input).disabled(self.pending).aria_label(tr(key))),
            );
        }
        form
    }
}
