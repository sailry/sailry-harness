use super::*;
use gpui_kit::component::{collapsible::Collapsible, dialog::Confirm};
use sailry_protocol::conversation::oauth::Options;

pub(super) struct Fields {
    expanded: bool,
    catalog_version: Entity<InputState>,
    user_agent: Entity<InputState>,
    editor_version: Entity<InputState>,
    editor_plugin_version: Entity<InputState>,
}

impl Fields {
    pub fn new(
        preset: Preset,
        options: Option<&Options>,
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Self {
        let options = options
            .cloned()
            .or_else(|| Options::defaults(preset.authentication()));
        let (catalog_version, user_agent, editor_version, editor_plugin_version) = match options {
            Some(Options::ChatGpt {
                catalog_version,
                user_agent,
            }) => (catalog_version, user_agent, String::new(), String::new()),
            Some(Options::Copilot {
                user_agent,
                editor_version,
                editor_plugin_version,
            }) => (
                String::new(),
                user_agent,
                editor_version,
                editor_plugin_version,
            ),
            None => (String::new(), String::new(), String::new(), String::new()),
        };
        let mut input = |value| cx.new(|cx| InputState::new(window, cx).default_value(value));
        Self {
            expanded: false,
            catalog_version: input(catalog_version),
            user_agent: input(user_agent),
            editor_version: input(editor_version),
            editor_plugin_version: input(editor_plugin_version),
        }
    }

    pub fn options(&self, preset: Preset, cx: &App) -> Result<Option<Options>, &'static str> {
        let read = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        let options = match preset {
            Preset::ChatGpt => Options::ChatGpt {
                catalog_version: read(&self.catalog_version),
                user_agent: read(&self.user_agent),
            },
            Preset::Copilot | Preset::CopilotResponses => Options::Copilot {
                user_agent: read(&self.user_agent),
                editor_version: read(&self.editor_version),
                editor_plugin_version: read(&self.editor_plugin_version),
            },
            _ => return Ok(None),
        };
        options
            .validate(preset.authentication(), preset.api())
            .map_err(|_| "provider_configuration_invalid")?;
        Ok(Some(options))
    }
}

impl Editor {
    pub(super) fn oauth_form(&self, cx: &Context<Self>) -> impl IntoElement {
        let fields: &[(&str, &str, &Entity<InputState>)] = match self.preset {
            Preset::ChatGpt => &[
                (
                    "provider_oauth_catalog_version",
                    "provider-oauth-catalog-version",
                    &self.oauth.catalog_version,
                ),
                (
                    "provider_oauth_user_agent",
                    "provider-oauth-user-agent",
                    &self.oauth.user_agent,
                ),
            ],
            Preset::Copilot | Preset::CopilotResponses => &[
                (
                    "provider_oauth_user_agent",
                    "provider-oauth-user-agent",
                    &self.oauth.user_agent,
                ),
                (
                    "provider_oauth_editor_version",
                    "provider-oauth-editor-version",
                    &self.oauth.editor_version,
                ),
                (
                    "provider_oauth_editor_plugin_version",
                    "provider-oauth-editor-plugin-version",
                    &self.oauth.editor_plugin_version,
                ),
            ],
            _ => &[],
        };
        let form = fields
            .iter()
            .fold(Form::vertical(), |form, &(key, selector, input)| {
                form.child(
                    Field::new().label(tr(key)).child(
                        div()
                            .w_full()
                            .debug_selector(move || selector.into())
                            .child(Input::new(input).disabled(self.pending).aria_label(tr(key))),
                    ),
                )
            });
        Collapsible::new()
            .w_full()
            .flex_shrink_0()
            .open(self.oauth.expanded)
            .gap_3()
            .child(
                Button::new("provider-oauth-advanced")
                    .debug_selector(|| "provider-oauth-advanced".into())
                    .label(tr("provider_oauth_advanced"))
                    .icon(if self.oauth.expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .toggled(self.oauth.expanded)
                    .ghost()
                    .disabled(self.pending)
                    // Dialog consumes Enter as Confirm before native Button key-up.
                    .on_action(cx.listener(|editor, _: &Confirm, _, cx| {
                        cx.stop_propagation();
                        if editor.pending {
                            return;
                        }
                        editor.oauth.expanded = !editor.oauth.expanded;
                        cx.notify();
                    }))
                    .on_click(cx.listener(|editor, _, _, cx| {
                        editor.oauth.expanded = !editor.oauth.expanded;
                        cx.notify();
                    })),
            )
            .content(form)
    }
}

#[cfg(test)]
mod tests;
