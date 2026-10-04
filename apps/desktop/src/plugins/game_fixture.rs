use super::fixture::{Fixture, copy_package};
use sailry_protocol::{Command, Output};
use std::path::Path;

impl Fixture {
    pub fn game_endpoint(&self, endpoint: &str) {
        let Output::Providers(mut providers) = self.execute(Command::ListProviders) else {
            panic!("providers expected");
        };
        let mut provider = providers.remove(0);
        let revision = provider.revision;
        provider.endpoint = endpoint.into();
        self.execute(Command::PutProvider {
            provider,
            expected_revision: revision,
        });
    }

    pub fn game(
        &self,
        configured: bool,
    ) -> (sailry_protocol::plugin::Info, crate::agent_fixture::Server) {
        let root = self.directory.path().join("project/doudizhu");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/doudizhu");
        copy_package(&source, &root);
        // Anchor native Kit controls so test interactions follow their layout.
        let view = root.join("dev.sailry.platform/desktop/view.js");
        let content = std::fs::read_to_string(&view).unwrap();
        let content = format!("import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}", content
                    .replace(".id(\"ddz-plays\")", ".id(\"ddz-plays\").child(div().absolute().top(0).child(TestAnchor.new(\"ddz-plays-top\"))).child(div().absolute().bottom(0).child(TestAnchor.new(\"ddz-plays-bottom\")))")
                    .replace(".id(\"ddz-last-play\")", ".id(\"ddz-last-play\").child(div().absolute().bottom(0).child(TestAnchor.new(\"ddz-last-play-bottom\")))")
                    .replace(".id(\"ddz-hand-region\")", ".id(\"ddz-hand-region\").relative().child(div().absolute().top(0).child(TestAnchor.new(\"ddz-hand-top\")))")
                    .replace("control.label(label)", "control.child(TestAnchor.new(id).child(div().child(label)))")
                    .replace("control.primary().label(label)", "control.primary().child(TestAnchor.new(id).child(div().child(label)))")
                    .replace(".child(new Icon(\"icons/rotate-cw.svg\").size(\"small\"))", ".child(TestAnchor.new(\"ddz-retry\").child(new Icon(\"icons/rotate-cw.svg\").size(\"small\")))"));
        std::fs::write(view, content).unwrap();
        let table = root.join("dev.sailry.platform/desktop/table.js");
        let content = std::fs::read_to_string(&table).unwrap();
        std::fs::write(
            table,
            format!(
                "import {{ Anchor as TestAnchor }} from \"sailry/test\";\n{}",
                content.replace(
                    ".on_click(click).child(content)",
                    ".on_click(click).child(TestAnchor.new(`card-${value}`).child(content))"
                )
            ),
        )
        .unwrap();
        let Output::Plugin(package) = self.execute(Command::InstallPlugin {
            worktree: self.session.worktree,
            path: "doudizhu".into(),
            name: "doudizhu".into(),
            expected_revision: 0,
        }) else {
            panic!("plugin expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let (server, model) = self.model_server();
        if !configured {
            return (package, server);
        }
        let Output::PluginSettings(state) = self.execute(Command::SavePluginSettings {
            package: package.summary.reference(),
            values: [
                (
                    "player_1_model".into(),
                    serde_json::Value::String(model.clone()),
                ),
                ("player_2_model".into(), serde_json::Value::String(model)),
                ("player_1_enabled".into(), serde_json::Value::Bool(true)),
                ("player_2_enabled".into(), serde_json::Value::Bool(true)),
            ]
            .into(),
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
