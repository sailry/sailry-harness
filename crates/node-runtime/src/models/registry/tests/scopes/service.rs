use super::*;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_protocol::{Command, Output};
use std::sync::mpsc::{RecvTimeoutError, Sender};
use tokio::{
    sync::oneshot,
    task::JoinHandle,
    time::{Duration, timeout},
};

mod decisions;
mod downloads;

struct Fixture {
    _directory: tempfile::TempDir,
    node: crate::Node,
    controller: Link,
    client: Client,
}

impl Fixture {
    async fn start(remote: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let node = crate::Node::start_empty_with_plugin_catalog(
            directory.path().join("node"),
            "http://127.0.0.1:9/",
        )
        .await
        .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        Self {
            _directory: directory,
            node,
            controller,
            client,
        }
    }

    async fn execute(&self, command: Command) -> Output {
        let output = self
            .client
            .execute(self.client.prepare(command))
            .await
            .unwrap();
        if let Output::LocalModelSettings(settings) = &output {
            assert_eq!(self.node.ingress.local_models.settings(), *settings);
        }
        if let Output::LocalModel { id } = &output {
            self.node.ingress.local_models.definition(id).unwrap();
        }
        output
    }

    async fn register(&self, definition: &Definition) -> String {
        let Output::LocalModel { id } = self
            .execute(Command::RegisterLocalModel {
                definition: definition.clone(),
            })
            .await
        else {
            panic!("registered model expected")
        };
        id
    }

    async fn settings(&self) -> Settings {
        let Output::LocalModelSettings(settings) =
            self.execute(Command::ReadLocalModelSettings).await
        else {
            panic!("local model settings expected")
        };
        settings
    }

    async fn embedding(&self) -> String {
        let (record, contents) = record(2, &[100., 0., 1., 0., 0., 1.]);
        let id = self.register(&record.definition).await;
        let path = files::directory(
            &self.node.ingress.local_models.root().unwrap(),
            &id,
            &record.definition,
        )
        .unwrap();
        std::fs::create_dir_all(&path).unwrap();
        for (name, bytes) in contents {
            std::fs::write(path.join(name), bytes).unwrap();
        }
        self.execute(Command::InstallLocalModel { id: id.clone() })
            .await;
        id
    }

    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

async fn active<T: Send + 'static>(
    owner: &Arc<Registry>,
    resource: Resource,
    operation: impl FnOnce(&mut Engine, &CancellationToken) -> Result<T, Fault> + Send + 'static,
) -> (
    JoinHandle<Result<T, Fault>>,
    Sender<()>,
    oneshot::Receiver<()>,
) {
    let (entered, entering) = oneshot::channel();
    let (stopped, stopping) = oneshot::channel();
    let (release, waiting) = std::sync::mpsc::channel();
    let owner = owner.clone();
    let work = tokio::spawn(async move {
        owner
            .execute(
                resource,
                Lane::Interactive,
                &CancellationToken::new(),
                move |engine, cancel| {
                    let _ = entered.send(());
                    loop {
                        if cancel.is_cancelled() {
                            let _ = stopped.send(());
                            return Err(cancelled());
                        }
                        match waiting.recv_timeout(Duration::from_millis(1)) {
                            Ok(()) => break,
                            Err(RecvTimeoutError::Timeout) => {}
                            Err(RecvTimeoutError::Disconnected) => return Err(cancelled()),
                        }
                    }
                    operation(engine, cancel)
                },
            )
            .await
    });
    timeout(Duration::from_secs(10), entering)
        .await
        .unwrap()
        .unwrap();
    (work, release, stopping)
}

fn encode(engine: &mut Engine, _: &CancellationToken) -> Result<Vec<Vec<f32>>, Fault> {
    let Engine::Embedding(engine) = engine else {
        panic!("embedding engine expected")
    };
    engine
        .encode_for(sailry_embedding::Input::Query, &["alpha"])
        .map_err(crate::models::runtime::embedding_error)
}

#[tokio::test]
async fn isolates_inflight_work_across_configuration() {
    for remote in [false, true] {
        let fixture = Fixture::start(remote).await;
        let id = fixture.embedding().await;
        let owner = fixture.node.ingress.local_models.clone();
        let resource = owner.resource(Kind::Embedding, Some(&id), None).unwrap();
        let (work, release, _) = active(&owner, resource, encode).await;
        fixture.register(&ocr().definition).await;
        let previous = fixture.settings().await;
        let mut settings = previous.clone();
        settings.choices.get_mut(&Kind::Ocr).unwrap().enabled = false;
        let Output::LocalModelSettings(mut settings) = fixture
            .execute(Command::SaveLocalModelSettings(settings))
            .await
        else {
            panic!("local model settings expected")
        };
        settings.mirror = Some("https://mirror.example.com/".into());
        fixture
            .execute(Command::SaveLocalModelSettings(settings))
            .await;
        let settings = fixture.settings().await;
        assert_eq!(settings.selected(Kind::Decision), None);
        fixture
            .execute(Command::SaveLocalModelSettings(settings))
            .await;
        assert_eq!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::SaveLocalModelSettings(previous))
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert!(!work.is_finished());
        release.send(()).unwrap();
        assert_eq!(
            timeout(Duration::from_secs(10), work)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            [vec![1., 0.]]
        );

        let resource = owner.resource(Kind::Embedding, Some(&id), None).unwrap();
        let (work, _release, stopped) = active(&owner, resource, encode).await;
        let mut settings = fixture.settings().await;
        settings.choices.get_mut(&Kind::Embedding).unwrap().enabled = false;
        fixture
            .execute(Command::SaveLocalModelSettings(settings))
            .await;
        assert_eq!(
            timeout(Duration::from_secs(2), work)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        timeout(Duration::from_secs(2), stopped)
            .await
            .unwrap()
            .unwrap();
        // A collection override remains valid after the global default is disabled.
        let resource = owner.resource(Kind::Embedding, Some(&id), None).unwrap();
        assert_eq!(
            owner
                .execute(
                    resource,
                    Lane::Interactive,
                    &CancellationToken::new(),
                    encode
                )
                .await
                .unwrap(),
            [vec![1., 0.]]
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn saves_without_touching_model_directories() {
    for remote in [false, true] {
        let fixture = Fixture::start(remote).await;
        let owner = &fixture.node.ingress.local_models;
        let directory = owner.root().unwrap().join("decision");
        std::fs::remove_dir(&directory).unwrap();
        std::fs::write(&directory, b"unavailable directory").unwrap();
        let mut settings = fixture.settings().await;
        settings.choices.get_mut(&Kind::Ocr).unwrap().enabled = false;
        let Output::LocalModelSettings(saved) = fixture
            .execute(Command::SaveLocalModelSettings(settings))
            .await
        else {
            panic!("saved model settings expected")
        };
        assert_eq!(owner.settings(), saved);
        assert_eq!(saved.revision, 1);
        assert!(!saved.choice(Kind::Ocr).enabled);
        std::fs::remove_file(directory).unwrap();
        fixture.close().await;
    }
}
