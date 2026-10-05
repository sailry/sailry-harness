use super::*;
use crate::provider_fixture;
use sailry_protocol::{Authentication, RoleId, role};
use std::sync::atomic::AtomicU8;

pub(super) struct Fixture {
    pub(super) runtime: Arc<tokio::runtime::Runtime>,
    pub(super) source: Option<Node>,
    pub(super) target: Node,
    pub(super) controller: Link,
    pub(super) transport: Arc<dyn Transport>,
    pub(super) binding: Binding,
    pub(super) provider: Provider,
    pub(super) source_provider: Provider,
    pub(super) source_server: provider_fixture::Server,
    pub(super) target_server: provider_fixture::Server,
    _directory: tempfile::TempDir,
}

impl Fixture {
    pub(super) fn new(remote: bool) -> Self {
        Self::with_models(
            remote,
            vec![Model {
                id: "fixture".into(),
                context: 8192,
                output: 512,
                vision: false,
                tools: false,
                reasoning: false,
                web_search: false,
                generates: vec![],
                efforts: vec![],
                custom_efforts: false,
                default_effort: Effort::Default,
            }],
        )
    }

    pub(super) fn empty(remote: bool) -> Self {
        Self::with_models(remote, Vec::new())
    }

    fn with_models(remote: bool, models: Vec<Model>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let source_server = runtime.block_on(provider_fixture::server(Arc::new(AtomicU8::new(1))));
        let target_server = runtime.block_on(provider_fixture::server(Arc::new(AtomicU8::new(1))));
        let source = runtime
            .block_on(Node::start_with_authorization(
                directory.path().join("source"),
                &source_server.endpoint,
            ))
            .unwrap();
        let target = runtime
            .block_on(Node::start_with_authorization(
                directory.path().join("target"),
                &target_server.endpoint,
            ))
            .unwrap();
        runtime
            .block_on(source.link().pair(target.link().invite().unwrap().ticket()))
            .unwrap();
        let controller = runtime
            .block_on(Link::controller(
                directory.path().join("controller"),
                NetworkScope::default(),
            ))
            .unwrap();
        let address = runtime
            .block_on(
                controller
                    .handle()
                    .pair(target.link().invite().unwrap().ticket()),
            )
            .unwrap();
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            target.local()
        };
        let client = Arc::new(Client::new(transport.clone()));
        let origin = Arc::new(Client::new(source.local()));
        let mut draft = provider_fixture::provider(Authentication::ChatGpt, ModelApi::Responses);
        draft.models = models;
        let source_provider = runtime.block_on(provider_fixture::connect(&origin, draft.clone()));
        // Matching IDs and model names must remain distinct across Node catalogs.
        let provider = runtime.block_on(provider_fixture::connect(&client, draft));
        for (client, key) in [(&origin, "source-role"), (&client, "target-role")] {
            runtime
                .block_on(client.execute(client.prepare(Command::PutRole {
                    role: role::Profile {
                        appearance: None,
                        id: RoleId::new(),
                        revision: 0,
                        key: key.into(),
                        name: key.into(),
                        description: "Configuration fixture".into(),
                        model: None,
                        max_turns: None,
                        skills: vec![],
                        instructions: "Use the fixture".into(),
                    },
                    expected_revision: 0,
                })))
                .unwrap();
        }
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let Output::Project(project) = runtime
            .block_on(client.execute(client.prepare(Command::RegisterProject {
                name: "Account fixture".into(),
                path: root.to_str().unwrap().into(),
            })))
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let binding = Binding {
            client,
            defaults: origin,
            runtime: runtime.clone(),
            project: Some(project.id),
            worktree: Some(snapshot.worktrees[0].id),
            host: "Execution host".into(),
            project_name: "Account fixture".into(),
            branch: "main".into(),
        };
        Self {
            runtime,
            source: Some(source),
            target,
            controller,
            transport,
            binding,
            provider,
            source_provider,
            source_server,
            target_server,
            _directory: directory,
        }
    }

    pub(super) fn execute(&self, command: Command) -> Output {
        let client = &self.binding.client;
        self.runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    }

    pub(super) fn assert_execution(&self, session: &Session, count: usize) {
        assert!(session.profile.is_none());
        assert_eq!(session.config.provider, self.provider.id);
        assert_eq!(session.config.credential, self.provider.credential);
        assert_ne!(session.config.credential, self.source_provider.credential);
        assert_eq!(session.roles.profiles.len(), 1);
        assert_eq!(session.roles.profiles[0].key, "target-role");
        let Output::Providers(providers) = self.execute(Command::ListProviders) else {
            panic!("providers expected")
        };
        assert_eq!(providers.as_slice(), std::slice::from_ref(&self.provider));
        assert!(
            !self
                .source_server
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request.path == "/responses")
        );
        let requests = self.target_server.requests.lock().unwrap();
        let generations: Vec<_> = requests
            .iter()
            .filter(|request| request.path == "/responses")
            .collect();
        assert_eq!(generations.len(), count);
        for request in generations {
            assert_eq!(request.headers["chatgpt-account-id"], "desktop-fixture");
            assert!(request.headers["authorization"].starts_with("Bearer e30."));
            let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
            assert_eq!(body["model"], "fixture");
            assert_eq!(body["store"], false);
            assert!(!request.body.contains("desktop-refresh-secret"));
        }
    }
}
