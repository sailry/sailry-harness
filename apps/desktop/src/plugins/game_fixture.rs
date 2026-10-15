use super::fixture::{Fixture, copy_package};
use sailry_protocol::{Command, Output};
use std::path::Path;

impl Fixture {
    pub fn game_plugin(
        &self,
        name: &str,
        model_fields: &[&str],
    ) -> (sailry_protocol::plugin::Info, crate::agent_fixture::Server) {
        self.game_plugin_with(name, model_fields, |_| {})
    }

    pub fn game_plugin_with(
        &self,
        name: &str,
        model_fields: &[&str],
        customize: impl FnOnce(&Path),
    ) -> (sailry_protocol::plugin::Info, crate::agent_fixture::Server) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins")
            .join(name);
        let root = self.directory.path().join("project").join(name);
        copy_package(&source, &root);
        customize(&root);
        let Output::Plugin(package) = self.execute(Command::InstallPlugin {
            worktree: self.session.worktree,
            path: name.into(),
            name: name.into(),
            expected_revision: 0,
            expected_digest: None,
        }) else {
            panic!("plugin expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let (server, model) = self.model_server();
        let Output::PluginSettings(state) = self.execute(Command::SavePluginSettings {
            package: package.summary.reference(),
            values: model_fields
                .iter()
                .map(|field| ((*field).into(), serde_json::Value::String(model.clone())))
                .chain(
                    package
                        .settings
                        .as_ref()
                        .into_iter()
                        .flat_map(|schema| &schema.properties)
                        .filter_map(|(name, field)| {
                            field
                                .default
                                .as_ref()
                                .filter(|value| value.is_boolean())
                                .map(|value| (name.clone(), value.clone()))
                        }),
                )
                .collect(),
            secrets: Default::default(),
        }) else {
            panic!("settings expected")
        };
        let Output::Plugin(package) = self.execute(Command::ReadPlugin {
            name: state.package.name,
        }) else {
            panic!("plugin expected")
        };
        (package, server)
    }

    fn model_server(&self) -> (crate::agent_fixture::Server, String) {
        let server = self
            .runtime
            .block_on(crate::agent_fixture::Server::markdown_after(
                "{\"move\":0}".into(),
                std::time::Duration::ZERO,
            ));
        let Output::Providers(providers) = self.execute(Command::ListProviders) else {
            panic!("providers expected")
        };
        let mut provider = providers[0].clone();
        let revision = provider.revision;
        provider.endpoint = server.endpoint.clone();
        provider.name = "Game fixture".into();
        provider.models[0].reasoning = true;
        provider.models[0].efforts = vec![
            sailry_protocol::Effort::Disabled,
            sailry_protocol::Effort::High,
        ];
        provider.models[0].default_effort = sailry_protocol::Effort::High;
        let model = format!("{}/{}", provider.id, provider.default_model);
        self.execute(Command::PutProvider {
            provider,
            expected_revision: revision,
        });
        (server, model)
    }
}
