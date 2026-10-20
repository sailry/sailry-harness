use super::*;
mod remember;

impl View {
    pub(super) fn available_features(
        &self,
    ) -> Vec<sailry_protocol::conversation::features::Feature> {
        self.config
            .as_ref()
            .and_then(|config| {
                self.providers()
                    .find(|provider| provider.id == config.provider)?
                    .models
                    .iter()
                    .find(|model| model.id == config.model)
            })
            .and_then(|model| model.capabilities.as_ref())
            .map(|value| value.features.clone())
            .unwrap_or_default()
    }

    pub(super) fn select_feature(
        &mut self,
        id: &str,
        value: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.configuration_blocked() || (self.session.is_some() && !self.connected()) {
            return;
        }
        if !self.available_features().iter().any(|feature| {
            feature.id == id
                && value.is_none_or(|value| feature.options.iter().any(|option| option.id == value))
        }) {
            return;
        }
        if let Some(mut config) = self.config.clone() {
            match value {
                Some(value) => {
                    config.features.insert(id.to_owned(), value.to_owned());
                }
                None => {
                    config.features.remove(id);
                }
            }
            self.configure(config, window, cx);
        }
    }

    pub(crate) fn available_efforts(&self) -> Vec<Effort> {
        self.config
            .as_ref()
            .and_then(|config| {
                self.providers()
                    .find(|provider| provider.id == config.provider)?
                    .models
                    .iter()
                    .find(|model| model.id == config.model && model.reasoning)
            })
            .map(|model| crate::reasoning::choices(&model.efforts))
            .unwrap_or_default()
    }

    pub(super) fn select_effort(
        &mut self,
        effort: Effort,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.configuration_blocked()
            || (self.session.is_some() && !self.connected())
            || !self.available_efforts().contains(&effort)
        {
            return;
        }
        if let Some(mut config) = self.config.clone() {
            config.effort = effort;
            self.configure(config, window, cx);
        }
    }

    pub(super) fn configuration(&self) -> &NodeView {
        if self.config_owner != self.binding.client.target() {
            &self.defaults
        } else {
            &self.node
        }
    }

    pub(super) fn model_sources(&self) -> impl Iterator<Item = (NodeId, &Provider)> {
        let target = self.binding.client.target();
        let source = self.binding.defaults.target();
        self.session
            .iter()
            .filter_map(|session| session.profile.as_ref())
            .map(move |profile| (target, &profile.provider))
            .chain(
                self.defaults
                    .snapshot
                    .iter()
                    .filter(move |_| self.session.is_none() && source != target)
                    .flat_map(|snapshot| &snapshot.providers)
                    .map(move |provider| (source, provider)),
            )
            .chain(
                self.node
                    .snapshot
                    .iter()
                    .flat_map(|snapshot| &snapshot.providers)
                    .map(move |provider| (target, provider)),
            )
    }

    pub(super) fn source_connected(&self, node: NodeId) -> bool {
        if node == self.binding.client.target() {
            self.node.connected
        } else {
            node == self.binding.defaults.target() && self.defaults.connected
        }
    }

    pub(super) fn can_pick_model(&self) -> bool {
        self.node.connected || (self.session.is_none() && self.defaults.connected)
    }

    pub(super) fn select_model(
        &mut self,
        selected: &super::super::models::Selection,
        effort: Effort,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly() || self.configuration_blocked() {
            return;
        }
        let choice = self
            .model_sources()
            .find(|(node, provider)| {
                self.provider_ids.get(&(*node, provider.id)) == Some(&selected.channel)
            })
            .filter(|(node, provider)| {
                self.source_connected(*node)
                    && provider.enabled
                    && provider
                        .models
                        .iter()
                        .any(|model| model.id == selected.model)
            })
            .map(|(node, provider)| {
                (
                    node,
                    SessionConfig {
                        assistant: None,
                        resource: None,
                        options: self
                            .config
                            .as_ref()
                            .map(|config| config.options.clone())
                            .unwrap_or_default(),
                        provider: provider.id,
                        model: selected.model.clone(),
                        credential: provider.credential.clone(),
                        features: super::model_controls::features::retain(
                            provider
                                .models
                                .iter()
                                .find(|model| model.id == selected.model)
                                .and_then(|model| model.capabilities.as_ref())
                                .map(|value| value.features.as_slice())
                                .unwrap_or_default(),
                            &self
                                .config
                                .as_ref()
                                .map(|config| config.features.clone())
                                .unwrap_or_default(),
                        ),
                        effort,
                        mode: self.config.as_ref().map_or(
                            self.draft_mode.unwrap_or(sailry_protocol::WorkMode::Code),
                            |config| config.mode,
                        ),
                        permission: self.config.as_ref().map_or(
                            self.draft_permission
                                .unwrap_or(sailry_protocol::Permission::Ask),
                            |config| config.permission,
                        ),
                    },
                )
            });
        if let Some((node, config)) = choice {
            if self.session.is_none() {
                self.config_owner = node;
            }
            self.configure(config, window, cx);
        }
    }

    pub(super) fn providers(&self) -> impl Iterator<Item = &Provider> {
        self.session
            .iter()
            .filter_map(|session| session.profile.as_ref())
            .map(|profile| &profile.provider)
            .chain(
                self.configuration()
                    .snapshot
                    .iter()
                    .flat_map(|snapshot| &snapshot.providers),
            )
    }

    pub(super) fn refresh_config(&mut self, cx: &App) {
        let providers: Vec<_> = self
            .model_sources()
            .map(|(node, provider)| (node, provider.id))
            .collect();
        for id in providers {
            let next = self.provider_ids.len();
            self.provider_ids.entry(id).or_insert(next);
        }
        if let Some(session) = &self.session {
            self.config_owner = self.binding.client.target();
            if !self.config_attempt() {
                self.config = Some(session.config.clone());
            }
        } else if self.config.is_none() {
            self.config = self.remembered_config(cx).or_else(|| {
                self.configuration().snapshot.as_ref().and_then(|snapshot| {
                    snapshot
                        .defaults
                        .config
                        .clone()
                        .filter(|config| {
                            snapshot.providers.iter().any(|provider| {
                                provider.enabled
                                    && provider.id == config.provider
                                    && provider.models.iter().any(|model| model.id == config.model)
                            })
                        })
                        .or_else(|| {
                            snapshot
                                .providers
                                .iter()
                                .find(|provider| provider.enabled && !provider.models.is_empty())
                                .map(|provider| SessionConfig {
                                    assistant: None,
                                    resource: None,
                                    options: Default::default(),
                                    provider: provider.id,
                                    model: provider.default_model.clone(),
                                    credential: provider.credential.clone(),
                                    features: Default::default(),
                                    mode: sailry_protocol::WorkMode::Code,
                                    permission: sailry_protocol::Permission::Ask,
                                    effort: provider
                                        .models
                                        .iter()
                                        .find(|model| model.id == provider.default_model)
                                        .map_or(Effort::Default, |model| {
                                            Effort::initial(&model.efforts, model.default_effort)
                                        }),
                                })
                        })
                })
            });
            if let Some(config) = &mut self.config {
                config.assistant = self.assistant.clone();
                config.resource = self.resource;
                if let Some(mode) = self.draft_mode {
                    config.mode = mode;
                }
                if let Some(permission) = self.draft_permission {
                    config.permission = permission;
                }
                if self.resource.is_some() {
                    config.mode = sailry_protocol::WorkMode::Code;
                    config.permission = self
                        .draft_permission
                        .unwrap_or(sailry_protocol::Permission::Ask);
                }
            }
        }
        if self.session.is_none() {
            let replacement = self.config.as_ref().and_then(|config| {
                self.providers()
                    .find(|provider| provider.id == config.provider)?
                    .models
                    .iter()
                    .find(|model| model.id == config.model)
                    .filter(|model| {
                        model.reasoning
                            && (config.effort == Effort::Default
                                || !model.efforts.contains(&config.effort))
                    })
                    .map(|model| Effort::initial(&model.efforts, model.default_effort))
            });
            if let (Some(config), Some(effort)) = (&mut self.config, replacement) {
                config.effort = effort;
            }
            let features = self.available_features();
            if let Some(config) = &mut self.config {
                config.features =
                    super::model_controls::features::retain(&features, &config.features);
            }
        }
    }

    pub(super) fn settings_node(&self) -> NodeId {
        self.config_owner
    }
}
