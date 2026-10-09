use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    knowledge::*,
    plugin::{self, Action, Context},
    *,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

mod catalog;
mod citations;
mod configuration;
mod context;
mod keywords;
#[cfg(feature = "test-support")]
mod performance;
mod registered;
mod runtime;
mod sources;
mod storage;
mod transactions;
mod worker;

struct Fixture {
    temp: tempfile::TempDir,
    root: PathBuf,
    node: Node,
    controller: Link,
    worktree: WorktreeId,
}

impl Fixture {
    async fn start() -> Self {
        Self::open(None).await
    }

    async fn open(cache: Option<&Path>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir(&root).unwrap();
        if let Some(cache) = cache {
            let descriptor = sailry_embedding::ModelId::PotionMulti.descriptor();
            let directory = temp
                .path()
                .join("node/models/embedding/potion-multi")
                .join(descriptor.revision);
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&directory).unwrap();
            for asset in descriptor.assets {
                let source = cache.join(asset.name);
                let metadata = fs::symlink_metadata(&source).unwrap();
                assert!(metadata.is_file() && !metadata.is_symlink());
                assert_eq!(metadata.len(), asset.bytes);
                fs::copy(source, directory.join(asset.name)).unwrap();
            }
        }
        let node = Node::start_with_plugin_catalog(temp.path().join("node"), "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(node.local());
        execute(
            &client,
            Command::RegisterProject {
                name: "Knowledge fixture".into(),
                path: root.to_str().unwrap().into(),
            },
        )
        .await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let worktree = snapshot.worktrees[0].id;
        let Output::KnowledgeSettings(mut settings) =
            execute(&client, Command::ReadKnowledgeSettings).await
        else {
            panic!("settings expected")
        };
        settings.model = None;
        execute(&client, Command::SaveKnowledgeSettings(settings)).await;
        let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
            .await
            .unwrap();
        controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        Self {
            temp,
            root,
            node,
            controller,
            worktree,
        }
    }

    fn clients(&self) -> [Client; 2] {
        [
            Client::new(self.node.local()),
            Client::new(self.controller.handle().remote(self.node.link().address())),
        ]
    }

    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

fn definition(source: Source) -> Definition {
    Definition {
        name: "Reference".into(),
        source,
        model: None,
        globs: vec![],
        tags: vec![],
        shared_with: vec![],
    }
}

fn saved(output: Output) -> Collection {
    let Output::KnowledgeCollection(collection) = output else {
        panic!("collection expected")
    };
    collection
}

async fn list(client: &Client) -> Vec<Collection> {
    let Output::KnowledgeCollections(collections) =
        execute(client, Command::ListKnowledgeCollections).await
    else {
        panic!("collections expected")
    };
    collections
}

fn query(text: &str, id: KnowledgeId) -> Query {
    Query {
        query: text.into(),
        collections: vec![id],
        mode: Mode::Hybrid,
        tags: vec![],
        path: None,
        modified_after_ms: None,
        modified_before_ms: None,
        limit: 5,
        offset: 0,
    }
}

async fn search(client: &Client, context: Option<&Context>, query: Query) -> Page {
    let mut request = client.prepare(Command::QueryKnowledge(query));
    if let Some(context) = context {
        request = request.with_plugin(context.clone());
    }
    let admission = client.dispatch(request.clone()).await.unwrap();
    assert!(!admission.receipt.durable);
    let Output::KnowledgeSearch(page) = admission.completion.await.unwrap().unwrap() else {
        panic!("search expected")
    };
    assert!(page.untrusted);
    assert_eq!(
        client.outcome(&request).await.unwrap_err().code,
        ErrorCode::InvalidRequest,
    );
    page
}

async fn found(client: &Client, context: Option<&Context>, text: &str, id: KnowledgeId) -> Page {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let page = search(client, context, query(text, id)).await;
            if !page.hits.is_empty() && !page.indexing {
                assert_eq!(page.mode, Mode::Keyword);
                assert_eq!(page.model, None);
                return page;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("collection indexing completed")
}

async fn install(
    client: &Client,
    root: &Path,
    worktree: WorktreeId,
    name: &str,
    revision: u64,
    actions: &[Action],
) -> Context {
    let package = root.join(name);
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("plugin.json"),
        serde_json::json!({
            "$schema": plugin::schema::AGENT_PLUGINS_ID,
            "name":name,"version":"1.0.0","description":"Knowledge fixture",
            "extensions":{"dev.sailry.platform":{"api_version":"v1","actions":actions}}
        })
        .to_string(),
    )
    .unwrap();
    let Output::Plugin(info) = execute(
        client,
        Command::InstallPlugin {
            worktree,
            path: name.into(),
            name: name.into(),
            expected_revision: revision,
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    assert_eq!(info.extension.unwrap().actions, actions);
    Context {
        package: info.summary.reference(),
        worktree: Some(worktree),
        session: None,
        turn: None,
        invocation: None,
        surface: Default::default(),
    }
}

async fn run(client: &Client, context: &Context, command: Command) -> Output {
    client
        .execute(client.prepare(command).with_plugin(context.clone()))
        .await
        .unwrap()
}
