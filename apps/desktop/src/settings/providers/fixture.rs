use sailry_client::Client;
use sailry_link::Transport;
use sailry_node_runtime::Node;
use sailry_protocol::{
    conversation::{Model, ModelApi, Provider},
    media::Generation,
    *,
};
use std::sync::Arc;

pub struct Fixture {
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub nodes: Vec<Node>,
    pub transports: Vec<Arc<dyn Transport>>,
    pub providers: Vec<Provider>,
    pub directory: tempfile::TempDir,
}

impl Fixture {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let nodes: Vec<_> = ["local", "remote"]
            .into_iter()
            .map(|name| {
                runtime
                    .block_on(Node::start(directory.path().join(name)))
                    .unwrap()
            })
            .collect();
        let address = runtime
            .block_on(
                nodes[0]
                    .link()
                    .pair(nodes[1].link().invite().unwrap().ticket()),
            )
            .unwrap();
        let transports = vec![nodes[0].local(), nodes[0].link().remote(address)];
        let providers = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let client = Client::new(node.local());
                let provider = Provider {
                    options: None,
                    id: ProviderId::new(),
                    revision: 0,
                    name: format!("Media host {index}"),
                    api: ModelApi::ChatCompletions,
                    authentication: Authentication::ApiKey,
                    endpoint: "http://127.0.0.1:9/v1".into(),
                    enabled: true,
                    credential: None,
                    default_model: "vision".into(),
                    models: ["vision", "image", "video", "text"]
                        .into_iter()
                        .map(|id| Model {
                            id: id.into(),
                            context: 16000,
                            output: 2048,
                            vision: id == "vision",
                            tools: true,
                            reasoning: false,
                            web_search: false,
                            generates: match id {
                                "image" => vec![Generation::Image],
                                "video" => vec![Generation::Video],
                                _ => vec![],
                            },
                            efforts: vec![],
                            custom_efforts: false,
                            default_effort: Effort::Default,
                        })
                        .collect(),
                };
                let Output::Provider(provider) = runtime
                    .block_on(client.execute(client.prepare(Command::PutProvider {
                        provider,
                        expected_revision: 0,
                    })))
                    .unwrap()
                else {
                    panic!("provider expected")
                };
                provider
            })
            .collect();
        Self {
            directory,
            runtime,
            nodes,
            transports,
            providers,
        }
    }

    pub fn close(self) {
        drop(self.transports);
        for node in self.nodes {
            self.runtime.block_on(node.shutdown()).unwrap();
        }
    }
}
