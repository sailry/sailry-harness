//! Captured draft commands are visible source, not hidden message prefixes.
use super::*;
use sailry_protocol::{conversation::Input, plugin::ui::CommandKind};
use serde_json::{Value, json};

impl View {
    pub(super) fn normalized_input(&self, mut input: Input) -> Input {
        if let Some((mode, _)) = references::commands::leading_mode(&input.text)
            && mode == self.current_mode()
        {
            input.text = references::commands::leading(&input.text)
                .unwrap()
                .1
                .to_owned();
        }
        input
    }

    pub(crate) fn composer_options(&self, cx: &App) -> Value {
        let compacting = self.history.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.page.runs.iter().any(|run| {
                run.kind == sailry_protocol::conversation::RunKind::Compaction
                    && matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
            })
        });
        let statistics = self.history.snapshot.as_ref().map(|snapshot| {
            crate::plugins::usage::exact(
                serde_json::to_value(&snapshot.statistics)
                    .expect("serializable conversation statistics"),
            )
        });
        let text = self.input.read(cx).value();
        let command = references::commands::leading(&text).and_then(|(name, arguments, _)| {
            self.references
                .command_keys
                .get(name)
                .filter(|key| self.contributions.read(cx).message_command(key, name, cx))
                .map(|key| json!({"name":name,"arguments":arguments,"package":key.package}))
        });
        let model_tools = self
            .config
            .as_ref()
            .and_then(|config| {
                self.providers()
                    .find(|provider| provider.id == config.provider)?
                    .models
                    .iter()
                    .find(|model| model.id == config.model)
            })
            .is_some_and(|model| model.tools);
        let operations = self.contribution_metadata.read(cx).operations(
            self.binding.project,
            self.resource,
            self.session
                .as_ref()
                .and_then(|session| session.config.assistant.as_ref())
                .or(self.assistant.as_ref()),
        );
        json!({"configuring":self.configuration_blocked(),"model_tools":model_tools,"operations":operations,"command":command,"mode":self.current_mode(),"session":self.session(),"connected":self.connected(),
            "connection":self.node.connection,"model":self.config.as_ref().map(|config| &config.model),"busy":self.busy(),"readonly":self.readonly(),"statistics":statistics,"context_limit":self.context_limit(),
            "compacting":compacting,"sidebar":self.sidebar,"has_messages":!self.rows.is_empty()})
    }

    pub(crate) fn model_context(
        &self,
        owner: &sailry_protocol::plugin::Reference,
        cx: &App,
    ) -> Value {
        let resources = self
            .contribution_metadata
            .read(cx)
            .entries
            .get(&owner.name)
            .filter(|info| info.summary.enabled && info.summary.reference() == *owner)
            .and_then(|info| info.extension.as_ref())
            .map(|extension| extension.model_resources.as_slice())
            .unwrap_or_default();
        let connected = self.connected() && self.config_owner == self.binding.client.target();
        let selected = self.config.as_ref().and_then(|config| {
            self.node
                .snapshot
                .as_ref()?
                .providers
                .iter()
                .find(|provider| provider.id == config.provider)
                .map(|provider| (provider, config.model.as_str()))
        });
        json!({"connected":connected,"provider":if connected {
            selected.and_then(|(provider,model)| crate::plugins::providers::context(provider,model,&owner.name,resources))
        } else {None}})
    }

    pub(crate) fn package_options(
        &self,
        owner: &sailry_protocol::plugin::Reference,
        cx: &App,
    ) -> Value {
        serde_json::to_value(
            self.contribution_metadata.read(cx).options(
                owner,
                self.config
                    .as_ref()
                    .and_then(|config| config.options.get(&owner.name)),
            ),
        )
        .expect("serializable public package options")
    }

    pub(crate) fn set_composer_option(
        &mut self,
        owner: &sailry_protocol::plugin::Reference,
        key: &str,
        value: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.busy() || !self.connected() || self.readonly() {
            return Err("composer is unavailable".into());
        }
        match key {
            "plugin" => {
                if self.configuration_blocked() {
                    return Err("composer configuration is unavailable".into());
                }
                let key = value
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or("plugin option requires a key")?;
                let value = value.get("value").ok_or("plugin option requires a value")?;
                if !self
                    .contribution_metadata
                    .read(cx)
                    .option_declared(owner, key, value)
                {
                    return Err("plugin option is unavailable".into());
                }
                let mut config = self
                    .config
                    .clone()
                    .ok_or("composer configuration is unavailable")?;
                let mut options = self
                    .contribution_metadata
                    .read(cx)
                    .options(owner, config.options.get(&owner.name));
                options.insert(key.to_owned(), value.clone());
                config
                    .options
                    .entry(owner.name.clone())
                    .or_default()
                    .extend(options);
                self.configure(config, window, cx);
                Ok(())
            }
            "mode" => {
                let mode = serde_json::from_value(value).map_err(|_| "invalid composer mode")?;
                self.select_mode(mode, window, cx);
                Ok(())
            }
            "command" => {
                if self.input.update(cx, |input, cx| {
                    input.marked_text_range(window, cx).is_some()
                }) {
                    return Err("composer is composing".into());
                }
                let entries = self.contributions.read(cx).commands(cx);
                let declared = |name: &str| {
                    entries.iter().any(|entry| {
                        entry.key.package == *owner
                            && entry.state.enabled
                            && entry.declaration.command.as_ref().is_some_and(|command| {
                                command.name == name && command.kind == CommandKind::Message
                            })
                    })
                };
                let name = if value.is_null() {
                    None
                } else {
                    let name = value.as_str().ok_or("composer command requires a string")?;
                    if !declared(name) {
                        return Err("composer command is unavailable".into());
                    }
                    Some(name)
                };
                let text = self.input.read(cx).value();
                let body = match references::commands::leading(&text) {
                    Some((current, arguments, _)) if declared(current) => arguments,
                    Some(_) => return Err("draft command belongs to another provider".into()),
                    None if name.is_none() => return Ok(()),
                    None => &text,
                };
                let end = text.len() - body.len();
                let prefix = name.map_or_else(String::new, |name| format!("/{name} "));
                self.input.update(cx, |input, cx| {
                    let selected = input.selected_range();
                    let shift = |offset: usize| offset.saturating_sub(end) + prefix.len();
                    if name.is_some() {
                        input
                            .replace_range_with_token(
                                0..end,
                                references::commands::token(prefix.trim()),
                                window,
                                cx,
                            )
                            .expect("command prefix is a valid input range");
                        input.replace(" ", window, cx);
                    } else {
                        input.set_selected_range(0..end, cx);
                        input.replace("", window, cx);
                    }
                    input.set_selected_range(shift(selected.start)..shift(selected.end), cx);
                });
                cx.notify();
                Ok(())
            }
            _ => Err("unknown composer option".into()),
        }
    }
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn projects_only_current_package_identity() {
        let mut provider:sailry_protocol::conversation::Provider=serde_json::from_value(json!({
            "id":sailry_protocol::ProviderId::new(),"revision":9007199254740993_u64,
            "name":"Private channel","api":"plugin","authentication":"api_key",
            "endpoint":"https://private.example.test",
            "enabled":true,"models":[],"default_model":"old-model",
            "preset":{"source":{"kind":"package","data":{"name":"sample"}},"id":"regional","route":null}
        })).unwrap();
        let context =
            crate::plugins::providers::context(&provider, "selected-model", "sample", &[]).unwrap();
        assert_eq!(context["revision"], "9007199254740993");
        assert_eq!(context["model"], "selected-model");
        assert_eq!(context["preset"]["id"], "regional");
        assert_eq!(context["authentication"], "api_key");
        assert_eq!(context.as_object().unwrap().len(), 5);
        assert!(!context.to_string().contains("private.example"));
        assert!(
            crate::plugins::providers::context(&provider, "selected-model", "other", &[]).is_none()
        );
        provider.enabled = false;
        assert!(
            crate::plugins::providers::context(&provider, "selected-model", "sample", &[])
                .is_none()
        );
        provider.enabled = true;
        provider.preset = None;
        assert!(
            crate::plugins::providers::context(&provider, "selected-model", "sample", &[])
                .is_none()
        );
    }

    #[test]
    fn projects_declared_native_authentication() {
        for (authentication, api) in [
            ("chat_gpt", "responses"),
            ("chat_gpt_oauth", "responses"),
            ("claude_oauth", "anthropic"),
        ] {
            let mut provider: sailry_protocol::conversation::Provider =
                serde_json::from_value(json!({
                    "id":sailry_protocol::ProviderId::new(),"revision":9007199254740993_u64,
                    "name":"Private channel","api":api,"authentication":authentication,
                    "endpoint":"https://private.example.test","enabled":true,
                    "models":[],"default_model":"previous","credential":null
                }))
                .unwrap();
            let resources = vec![serde_json::from_value(json!({
                "authentication":authentication,"requests":{"usage":{"url":"https://example.test/usage"}}
            })).unwrap()];
            let context =
                crate::plugins::providers::context(&provider, "selected", "status", &resources)
                    .unwrap();
            assert_eq!(context["authentication"], authentication);
            assert_eq!(context["revision"], "9007199254740993");
            assert_eq!(context["model"], "selected");
            assert!(context["preset"].is_null());
            assert_eq!(context.as_object().unwrap().len(), 5);
            assert!(!context.to_string().contains("private.example"));
            assert!(
                crate::plugins::providers::context(&provider, "selected", "status", &[]).is_none()
            );
            provider.authentication = sailry_protocol::Authentication::ApiKey;
            assert!(
                crate::plugins::providers::context(&provider, "selected", "status", &resources)
                    .is_none()
            );
            provider.authentication = resources[0].authentication;
            provider.enabled = false;
            assert!(
                crate::plugins::providers::context(&provider, "selected", "status", &resources)
                    .is_none()
            );
        }
    }
}
