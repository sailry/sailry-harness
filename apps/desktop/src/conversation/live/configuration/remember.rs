use super::*;

impl View {
    pub(super) fn remembered_config(&self, cx: &App) -> Option<SessionConfig> {
        let selection = crate::preferences::data(cx).composer?;
        let model = selection.model?;
        let provider = self
            .model_sources()
            .find(|(node, provider)| {
                *node == self.config_owner
                    && *node == model.source
                    && provider.id == model.provider
                    && provider.enabled
                    && provider.models.iter().any(|entry| entry.id == model.model)
            })?
            .1;
        Some(SessionConfig {
            assistant: None,
            resource: None,
            provider: provider.id,
            model: model.model,
            effort: model.effort,
            mode: selection.mode,
            permission: selection.permission,
            credential: provider.credential.clone(),
        })
    }

    pub(in crate::conversation::live) fn remember_config(
        &mut self,
        config: &SessionConfig,
        cx: &mut Context<Self>,
    ) {
        let source = self
            .session
            .as_ref()
            .and_then(|session| session.profile.as_ref())
            .filter(|profile| profile.provider.id == config.provider)
            .map_or(self.config_owner, |profile| profile.source);
        crate::preferences::update(cx, |data| {
            data.composer = Some(crate::preferences::Composer {
                model: Some(crate::preferences::Model {
                    source,
                    provider: config.provider,
                    model: config.model.clone(),
                    effort: config.effort,
                }),
                mode: config.mode,
                permission: config.permission,
            });
        });
        self.error = cx.global::<crate::preferences::Preferences>().error;
    }

    pub(in crate::conversation::live) fn remember_options(&mut self, cx: &mut Context<Self>) {
        crate::preferences::update(cx, |data| {
            let selection = data.composer.get_or_insert_with(Default::default);
            if let Some(mode) = self.draft_mode {
                selection.mode = mode;
            }
            if let Some(permission) = self.draft_permission {
                selection.permission = permission;
            }
        });
        self.error = cx.global::<crate::preferences::Preferences>().error;
    }
}
