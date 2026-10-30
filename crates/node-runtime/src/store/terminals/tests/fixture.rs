use super::super::*;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use std::{sync::Arc, time::Duration};

pub(super) struct Fixture {
    pub(super) directory: tempfile::TempDir,
    pub(super) node: crate::Node,
    pub(super) controller: Link,
    pub(super) client: Arc<Client>,
    pub(super) worktrees: Vec<WorktreeId>,
}

impl Fixture {
    pub(super) async fn new(remote: bool, shell: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let node = crate::Node::start_empty_with_environment(
            directory.path().join("node"),
            crate::EnvironmentFixture {
                home: home.clone(),
                shell: "/usr/bin/false".into(),
                environment: [
                    ("PATH".into(), "/usr/bin:/bin".into()),
                    ("LANG".into(), "C".into()),
                ]
                .into(),
                timeout: Duration::from_secs(5),
            },
        )
        .await
        .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        client
            .execute(
                client.prepare(Command::SaveTerminalSettings(terminal::Settings {
                    shell: shell.into(),
                    environment: [("HOME".into(), home.to_string_lossy().into_owned())].into(),
                    ..Default::default()
                })),
            )
            .await
            .unwrap();
        let mut worktrees = Vec::new();
        for name in ["first", "second"] {
            let root = directory.path().join(name);
            std::fs::create_dir(&root).unwrap();
            let Output::Project(project) = client
                .execute(client.prepare(Command::RegisterProject {
                    name: name.into(),
                    path: root.to_string_lossy().into_owned(),
                }))
                .await
                .unwrap()
            else {
                panic!("project expected")
            };
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            worktrees.push(
                snapshot
                    .worktrees
                    .iter()
                    .find(|worktree| worktree.project == Some(project.id))
                    .unwrap()
                    .id,
            );
        }
        Self {
            directory,
            node,
            controller,
            client,
            worktrees,
        }
    }

    /// Sends input, retrying only a full queue that rejected it before the PTY owner.
    pub(super) async fn input(&self, info: &terminal::Info, input: terminal::Input) {
        let request = self.client.prepare(Command::InputTerminal {
            terminal: info.id,
            revision: info.revision,
            input,
        });
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match self.client.execute(request.clone()).await {
                    Ok(_) => break,
                    Err(error)
                        if error.code == ErrorCode::Busy
                            && error.message == "terminal command queue is full" =>
                    {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(error) => panic!("terminal input failed: {error:?}"),
                }
            }
        })
        .await
        .expect("terminal input deadline");
    }

    pub(super) async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

pub(super) fn launch(worktree: WorktreeId) -> terminal::Launch {
    let black = terminal::Rgb {
        red: 0,
        green: 0,
        blue: 0,
    };
    terminal::Launch {
        worktree,
        access: sailry_protocol::process::Access::Unrestricted,
        viewport: terminal::Viewport {
            columns: 80,
            rows: 24,
            pixel_width: 640,
            pixel_height: 384,
        },
        appearance: terminal::Appearance {
            foreground: terminal::Rgb {
                red: 240,
                green: 240,
                blue: 240,
            },
            background: black,
            palette: [black; 16],
            color_scheme: terminal::ColorScheme::Dark,
        },
    }
}
